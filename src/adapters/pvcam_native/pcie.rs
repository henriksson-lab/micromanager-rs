//! PCIe carrier — talks to the GPLv2 `pvcam_pcie.ko` kernel module (RE notes §3).
//!
//! The DDI layer drives PCIe cameras with plain `write()`/`read()`/`ioctl()`
//! syscalls on the char device (`pvcam_driver_kernel_os_{write,read,ioctl}`): the
//! kernel module copies the command blob into the FPGA command buffer, rings the
//! doorbell, and returns the response — all invisible to userspace. Acquisition is
//! armed with `ioctl(ACQ_SET_ACTIVE)` (handing the driver a host DMA buffer) and
//! frames are signalled via `ioctl(GET_NOTIFICATIONS)`.
//!
//! Real implementation is gated behind `feature = "pvcam-native-pcie"` (Linux);
//! without it, [`PciBus::open`] returns an error so default builds need no `libc`.

use super::transport::{Notification, PvcamBus};
use crate::error::{MmError, MmResult};

/// ioctl request numbers (`drivercom_linux.h`, magic `'='`, API v3).
#[allow(dead_code)]
pub mod ioc {
    pub const MAGIC: u32 = b'=' as u32;
    pub const GET_DRIVER_API_VERSION: u32 = 0;
    pub const CAM_POST_OPEN: u32 = 1;
    pub const CAM_PRE_CLOSE: u32 = 2;
    pub const ACQ_SET_ACTIVE: u32 = 10;
    pub const ACQ_GET_STATUS: u32 = 11;
    pub const ACQ_ABORT: u32 = 12;
    pub const STOP_DATA_TRANSFER: u32 = 13;
    pub const CLEAR_IRQS: u32 = 20;
    pub const GET_IRQS: u32 = 21;
    pub const FLUSH_NOTIFICATIONS: u32 = 30;
    pub const GET_NOTIFICATIONS: u32 = 31;
    pub const CONFIRM_EOF_NOTIFICATION: u32 = 32;
    pub const RESET_INTERFACE: u32 = 40;
    pub const GET_DRIVER_VERSION: u32 = 60;
    pub const GET_PCI_FW_REVISION: u32 = 62;
}

/// `pvcam_notif.type` bits (`drivercom.h`).
pub const NOTIF_EOF: u32 = 0x0000_0008;
pub const NOTIF_BOF: u32 = 0x0000_0004;

#[cfg(all(target_os = "linux", feature = "pvcam-native-pcie"))]
mod imp {
    use super::*;
    use crate::adapters::pvcam_native::protocol::decode_response;
    use std::os::fd::{AsRawFd, OwnedFd};

    // ── Linux ioctl encoding (asm-generic) ──────────────────────────────────
    const IOC_NONE: u32 = 0;
    const IOC_WRITE: u32 = 1;
    const IOC_READ: u32 = 2;
    const fn ioc(dir: u32, ty: u32, nr: u32, size: u32) -> libc::c_ulong {
        ((dir << 30) | (ty << 8) | nr | (size << 16)) as libc::c_ulong
    }

    #[repr(C)]
    struct AcqActive {
        total_bytes: u64,
        frame_bytes: u32,
        is_circular: u32,
        data_ptr: u64,
        timeout_ms: u32,
        notif_mask: u32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Notif {
        size: u16,
        version: u16,
        ntype: u32,
        time_stamp: u64,
        time_stamp_bof: u64,
        bof_count: u32,
        eof_count: u32,
    }
    #[repr(C)]
    struct NotifsHdr {
        capacity: u16,
        count: u16,
        _reserved: [u8; 12],
    }

    fn io_get_notifications() -> libc::c_ulong {
        ioc(IOC_READ | IOC_WRITE, ioc::MAGIC, ioc::GET_NOTIFICATIONS,
            std::mem::size_of::<NotifsHdr>() as u32)
    }
    fn io_acq_set_active() -> libc::c_ulong {
        ioc(IOC_WRITE, ioc::MAGIC, ioc::ACQ_SET_ACTIVE,
            std::mem::size_of::<AcqActive>() as u32)
    }
    fn io_acq_abort() -> libc::c_ulong {
        ioc(IOC_NONE, ioc::MAGIC, ioc::ACQ_ABORT, 0)
    }
    fn io_cam_post_open() -> libc::c_ulong {
        ioc(IOC_NONE, ioc::MAGIC, ioc::CAM_POST_OPEN, 0)
    }

    fn errno_err(ctx: &str) -> MmError {
        MmError::LocallyDefined(format!(
            "pvcam_pcie {ctx}: {}",
            std::io::Error::last_os_error()
        ))
    }

    pub struct PciBus {
        fd: OwnedFd,
        /// Host DMA buffer handed to the driver via ACQ_SET_ACTIVE.
        dma: Vec<u8>,
        frame_bytes: u32,
        eof_seen: u32,
    }

    impl PciBus {
        pub fn open(index: usize) -> MmResult<Self> {
            use std::ffi::CString;
            let path = CString::new(format!("/dev/pvcam_pcie{index}")).unwrap();
            // SAFETY: valid NUL-terminated path; O_RDWR|O_CLOEXEC.
            let raw = unsafe { libc::open(path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
            if raw < 0 {
                return Err(errno_err("open"));
            }
            // SAFETY: raw is a fresh, owned, valid fd.
            let fd = unsafe { OwnedFd::from_raw_fd(raw) };
            // Session handshake.
            // SAFETY: no-arg ioctl on our fd.
            if unsafe { libc::ioctl(fd.as_raw_fd(), io_cam_post_open()) } < 0 {
                return Err(errno_err("CAM_POST_OPEN"));
            }
            Ok(Self { fd, dma: Vec::new(), frame_bytes: 0, eof_seen: 0 })
        }
    }

    impl PvcamBus for PciBus {
        fn write_read(&mut self, frame: &[u8], resp: &mut [u8]) -> MmResult<usize> {
            // SAFETY: write() the command frame to the char device.
            let w = unsafe {
                libc::write(self.fd.as_raw_fd(), frame.as_ptr().cast(), frame.len())
            };
            if w < 0 {
                return Err(errno_err("write"));
            }
            let mut raw = [0u8; 4160]; // >= framed response (4KB response buffer)
            // SAFETY: read() the response frame back.
            let r = unsafe {
                libc::read(self.fd.as_raw_fd(), raw.as_mut_ptr().cast(), raw.len())
            };
            if r < 0 {
                return Err(errno_err("read"));
            }
            let payload = decode_response(&raw[..r as usize]);
            let n = payload.len().min(resp.len());
            resp[..n].copy_from_slice(&payload[..n]);
            Ok(n)
        }

        fn arm_acquisition(&mut self, frame_bytes: u32, total_bytes: u64, circular: bool) -> MmResult<()> {
            self.dma = vec![0u8; total_bytes.max(frame_bytes as u64) as usize];
            self.frame_bytes = frame_bytes;
            self.eof_seen = 0;
            let active = AcqActive {
                total_bytes,
                frame_bytes,
                is_circular: circular as u32,
                data_ptr: self.dma.as_mut_ptr() as u64,
                timeout_ms: 5000,
                notif_mask: NOTIF_EOF | NOTIF_BOF,
            };
            // SAFETY: ACQ_SET_ACTIVE takes a pointer to AcqActive; dma outlives the call.
            let rc = unsafe {
                libc::ioctl(self.fd.as_raw_fd(), io_acq_set_active(), &active as *const _)
            };
            if rc < 0 {
                return Err(errno_err("ACQ_SET_ACTIVE"));
            }
            Ok(())
        }

        fn poll_notification(&mut self) -> MmResult<Option<Notification>> {
            // Request up to 8 queued notifications.
            const CAP: usize = 8;
            #[repr(C)]
            struct Buf {
                hdr: NotifsHdr,
                notifs: [Notif; CAP],
            }
            let mut buf = Buf {
                hdr: NotifsHdr { capacity: CAP as u16, count: 0, _reserved: [0; 12] },
                notifs: [Notif { size: 0, version: 0, ntype: 0, time_stamp: 0, time_stamp_bof: 0, bof_count: 0, eof_count: 0 }; CAP],
            };
            // SAFETY: GET_NOTIFICATIONS reads/writes the notifs buffer.
            let rc = unsafe {
                libc::ioctl(self.fd.as_raw_fd(), io_get_notifications(), &mut buf as *mut _)
            };
            if rc < 0 {
                return Err(errno_err("GET_NOTIFICATIONS"));
            }
            for n in buf.notifs.iter().take(buf.hdr.count as usize) {
                if n.ntype & NOTIF_EOF != 0 {
                    self.eof_seen = n.eof_count;
                    return Ok(Some(Notification {
                        bof_count: n.bof_count,
                        eof_count: n.eof_count,
                        timestamp: n.time_stamp,
                    }));
                }
            }
            Ok(None)
        }

        fn read_frame(&mut self, dst: &mut [u8]) -> MmResult<()> {
            // Newest frame lives at slot (eof_seen-1) in the DMA buffer.
            let fb = self.frame_bytes as usize;
            if fb == 0 || self.dma.len() < fb {
                return Err(MmError::LocallyDefined("pvcam_pcie: acquisition not armed".into()));
            }
            let slot = self.eof_seen.saturating_sub(1) as usize;
            let off = (slot * fb) % self.dma.len().max(fb);
            let end = (off + fb).min(self.dma.len());
            let n = (end - off).min(dst.len());
            dst[..n].copy_from_slice(&self.dma[off..off + n]);
            Ok(())
        }

        fn stop_acquisition(&mut self) -> MmResult<()> {
            // SAFETY: no-arg abort ioctl.
            let rc = unsafe { libc::ioctl(self.fd.as_raw_fd(), io_acq_abort()) };
            if rc < 0 {
                return Err(errno_err("ACQ_ABORT"));
            }
            Ok(())
        }
    }

    use std::os::fd::FromRawFd;
}

#[cfg(not(all(target_os = "linux", feature = "pvcam-native-pcie")))]
mod imp {
    use super::*;

    /// Stub used when the `pvcam-native-pcie` feature (Linux) is off.
    pub struct PciBus {
        _priv: (),
    }
    impl PciBus {
        pub fn open(index: usize) -> MmResult<Self> {
            let _ = index;
            Err(MmError::LocallyDefined(
                "pvcam-native-pcie feature not enabled (build with --features pvcam-native-pcie on Linux)".into(),
            ))
        }
    }
    impl PvcamBus for PciBus {
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

pub use imp::PciBus;
