//! USB carrier — libusb-equivalent transport over the Linux **usbdevfs** ioctl
//! interface (`/dev/bus/usb`), recovered from `pvcam_usb.*.umd` (RE §4quinquies).
//! Commands ride vendor control transfers on EP0; image data streams over a bulk
//! IN endpoint.
//!
//! Rather than pull in the `rusb`/libusb dependency, this talks usbdevfs directly
//! with `ioctl` — the same kernel interface libusb itself uses. Real code is gated
//! behind `feature = "pvcam-native-usb"` (Linux); without it, [`UsbBus::open`]
//! returns an error so default builds need no `libc`.

use super::transport::{Notification, PvcamBus};
use crate::error::{MmError, MmResult};

/// Teledyne Photometrics USB vendor id (udev rule `70-pvcam-drv-usb.rules`).
pub const USB_VID: u16 = 0x1F12;

/// Vendor control-transfer setup packets (recovered from `Camera::_write`/`_read`).
pub mod ctrl {
    /// Write command: host→device, vendor, device recipient.
    pub const WRITE_REQUEST_TYPE: u8 = 0x40;
    pub const WRITE_REQUEST: u8 = 0xD4;
    /// Read response: device→host, vendor, device recipient.
    pub const READ_REQUEST_TYPE: u8 = 0xC0;
    pub const READ_REQUEST: u8 = 0xD5;
}

#[cfg(all(target_os = "linux", feature = "pvcam-native-usb"))]
mod imp {
    use super::*;
    use crate::adapters::pvcam_native::protocol::decode_response;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    const IOC_READ: u32 = 2;
    const IOC_WRITE: u32 = 1;
    const fn ioc(dir: u32, ty: u32, nr: u32, size: u32) -> libc::c_ulong {
        ((dir << 30) | (ty << 8) | nr | (size << 16)) as libc::c_ulong
    }

    #[repr(C)]
    struct CtrlTransfer {
        b_request_type: u8,
        b_request: u8,
        w_value: u16,
        w_index: u16,
        w_length: u16,
        timeout: u32,
        data: *mut libc::c_void,
    }
    #[repr(C)]
    struct BulkTransfer {
        ep: u32,
        len: u32,
        timeout: u32,
        data: *mut libc::c_void,
    }

    fn io_control() -> libc::c_ulong {
        ioc(IOC_READ | IOC_WRITE, b'U' as u32, 0, std::mem::size_of::<CtrlTransfer>() as u32)
    }
    fn io_bulk() -> libc::c_ulong {
        ioc(IOC_READ | IOC_WRITE, b'U' as u32, 2, std::mem::size_of::<BulkTransfer>() as u32)
    }
    fn io_claim_interface() -> libc::c_ulong {
        ioc(IOC_READ, b'U' as u32, 15, 4)
    }

    fn errno_err(ctx: &str) -> MmError {
        MmError::LocallyDefined(format!("pvcam_usb {ctx}: {}", std::io::Error::last_os_error()))
    }

    /// Scan `/dev/bus/usb/*/*` for a device with `idVendor == USB_VID`.
    /// Returns (node path, bulk-IN endpoint address).
    fn find_device() -> MmResult<(String, u8)> {
        let busdir = std::path::Path::new("/dev/bus/usb");
        let buses = std::fs::read_dir(busdir)
            .map_err(|e| MmError::LocallyDefined(format!("pvcam_usb: {e}")))?;
        for bus in buses.flatten() {
            let devs = match std::fs::read_dir(bus.path()) {
                Ok(d) => d,
                Err(_) => continue,
            };
            for dev in devs.flatten() {
                let path = dev.path();
                let blob = match std::fs::read(&path) {
                    Ok(b) if b.len() >= 18 => b,
                    _ => continue,
                };
                // Device descriptor: idVendor is LE16 at offset 8.
                let vid = u16::from_le_bytes([blob[8], blob[9]]);
                if vid != USB_VID {
                    continue;
                }
                let ep = parse_bulk_in_ep(&blob).unwrap_or(0x81);
                return Ok((path.to_string_lossy().into_owned(), ep));
            }
        }
        Err(MmError::LocallyDefined(format!(
            "no Photometrics USB camera found (VID 0x{USB_VID:04X})"
        )))
    }

    /// Walk the concatenated descriptors for the first bulk IN endpoint.
    fn parse_bulk_in_ep(blob: &[u8]) -> Option<u8> {
        let mut i = 0usize;
        while i + 2 <= blob.len() {
            let b_len = blob[i] as usize;
            let b_type = blob[i + 1];
            if b_len < 2 || i + b_len > blob.len() {
                break;
            }
            // ENDPOINT descriptor = 0x05; bEndpointAddress@2, bmAttributes@3.
            if b_type == 0x05 && b_len >= 4 {
                let addr = blob[i + 2];
                let attr = blob[i + 3];
                if addr & 0x80 != 0 && attr & 0x03 == 0x02 {
                    return Some(addr);
                }
            }
            i += b_len;
        }
        None
    }

    pub struct UsbBus {
        fd: OwnedFd,
        bulk_in_ep: u8,
        frame_bytes: u32,
        frame_buf: Vec<u8>,
        got_frame: bool,
    }

    impl UsbBus {
        pub fn open(_index: usize) -> MmResult<Self> {
            let (path, ep) = find_device()?;
            let cpath = std::ffi::CString::new(path).unwrap();
            // SAFETY: valid path; O_RDWR|O_CLOEXEC.
            let raw = unsafe { libc::open(cpath.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
            if raw < 0 {
                return Err(errno_err("open"));
            }
            // SAFETY: fresh owned fd.
            let fd = unsafe { OwnedFd::from_raw_fd(raw) };
            // Claim interface 0.
            let iface: u32 = 0;
            // SAFETY: CLAIMINTERFACE takes a pointer to the interface number.
            if unsafe { libc::ioctl(fd.as_raw_fd(), io_claim_interface(), &iface as *const _) } < 0 {
                return Err(errno_err("CLAIMINTERFACE"));
            }
            Ok(Self { fd, bulk_in_ep: ep, frame_bytes: 0, frame_buf: Vec::new(), got_frame: false })
        }

        fn control(&self, rt: u8, req: u8, value: u16, data: *mut u8, len: u16) -> MmResult<usize> {
            let mut xfer = CtrlTransfer {
                b_request_type: rt,
                b_request: req,
                w_value: value,
                w_index: 0,
                w_length: len,
                timeout: 5000,
                data: data.cast(),
            };
            // SAFETY: usbdevfs CONTROL ioctl; data buffer valid for `len` bytes.
            let rc = unsafe { libc::ioctl(self.fd.as_raw_fd(), io_control(), &mut xfer as *mut _) };
            if rc < 0 {
                return Err(errno_err("USBDEVFS_CONTROL"));
            }
            Ok(rc as usize)
        }
    }

    impl PvcamBus for UsbBus {
        fn write_read(&mut self, frame: &[u8], resp: &mut [u8]) -> MmResult<usize> {
            // WRITE: vendor OUT control, bRequest 0xD4, wValue = length.
            self.control(
                ctrl::WRITE_REQUEST_TYPE,
                ctrl::WRITE_REQUEST,
                frame.len() as u16,
                frame.as_ptr() as *mut u8,
                frame.len() as u16,
            )?;
            // READ: vendor IN control, bRequest 0xD5.
            let mut raw = [0u8; 4160];
            let n = self.control(
                ctrl::READ_REQUEST_TYPE,
                ctrl::READ_REQUEST,
                0,
                raw.as_mut_ptr(),
                raw.len() as u16,
            )?;
            let payload = decode_response(&raw[..n]);
            let m = payload.len().min(resp.len());
            resp[..m].copy_from_slice(&payload[..m]);
            Ok(m)
        }

        fn arm_acquisition(&mut self, frame_bytes: u32, _total: u64, _circ: bool) -> MmResult<()> {
            // The camera-side PREP_TRANSFER / start commands go through write_read;
            // here we just size the per-frame bulk buffer.
            self.frame_bytes = frame_bytes;
            self.frame_buf = vec![0u8; frame_bytes as usize];
            self.got_frame = false;
            Ok(())
        }

        fn poll_notification(&mut self) -> MmResult<Option<Notification>> {
            if self.frame_bytes == 0 {
                return Ok(None);
            }
            // Bulk IN read of one frame's worth of pixel data.
            let mut xfer = BulkTransfer {
                ep: self.bulk_in_ep as u32,
                len: self.frame_bytes,
                timeout: 5000,
                data: self.frame_buf.as_mut_ptr().cast(),
            };
            // SAFETY: usbdevfs BULK ioctl; buffer sized to frame_bytes.
            let rc = unsafe { libc::ioctl(self.fd.as_raw_fd(), io_bulk(), &mut xfer as *mut _) };
            if rc < 0 {
                return Err(errno_err("USBDEVFS_BULK"));
            }
            if rc as u32 >= self.frame_bytes {
                self.got_frame = true;
                Ok(Some(Notification { eof_count: 1, ..Default::default() }))
            } else {
                Ok(None)
            }
        }

        fn read_frame(&mut self, dst: &mut [u8]) -> MmResult<()> {
            if !self.got_frame {
                return Err(MmError::LocallyDefined("pvcam_usb: no frame received".into()));
            }
            let n = self.frame_buf.len().min(dst.len());
            dst[..n].copy_from_slice(&self.frame_buf[..n]);
            Ok(())
        }

        fn stop_acquisition(&mut self) -> MmResult<()> {
            // Camera-side stop (STOP_CCS/abort) goes through write_read; drop buffers.
            self.frame_bytes = 0;
            self.frame_buf = Vec::new();
            self.got_frame = false;
            Ok(())
        }
    }
}

#[cfg(not(all(target_os = "linux", feature = "pvcam-native-usb")))]
mod imp {
    use super::*;

    /// Stub used when the `pvcam-native-usb` feature (Linux) is off.
    pub struct UsbBus {
        _priv: (),
    }
    impl UsbBus {
        pub fn open(index: usize) -> MmResult<Self> {
            let _ = index;
            Err(MmError::LocallyDefined(
                "pvcam-native-usb feature not enabled (build with --features pvcam-native-usb on Linux)".into(),
            ))
        }
    }
    impl PvcamBus for UsbBus {
        fn write_read(&mut self, _f: &[u8], _r: &mut [u8]) -> MmResult<usize> {
            Err(MmError::NotSupported)
        }
        fn arm_acquisition(&mut self, _fb: u32, _tb: u64, _c: bool) -> MmResult<()> {
            Err(MmError::NotSupported)
        }
        fn poll_notification(&mut self) -> MmResult<Option<Notification>> {
            Ok(None)
        }
        fn read_frame(&mut self, _d: &mut [u8]) -> MmResult<()> {
            Err(MmError::NotSupported)
        }
        fn stop_acquisition(&mut self) -> MmResult<()> {
            Err(MmError::NotSupported)
        }
    }
}

pub use imp::UsbBus;
