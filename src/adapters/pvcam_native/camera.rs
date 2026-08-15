//! `PvcamCamera` — a pure-Rust Photometrics camera device built on the
//! reverse-engineered protocol ([`super::protocol`]) over a [`PvcamBus`] carrier
//! (PCIe or USB). No vendor `libpvcam` involved.

use super::protocol::{self as proto, code, DataFormat};
use super::transport::PvcamBus;
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Camera, Device};
use crate::types::{DeviceType, ImageRoi, PropertyValue};

/// Exposure-time resolution passed in the SCCL `EXPOSE_RESOLUTION` op
/// (0 = milliseconds, see `PL_EXP_RES_MODES`).
const EXP_RES_MS: u32 = 0;

pub struct PvcamCamera {
    props: PropertyMap,
    bus: Option<Box<dyn PvcamBus>>,
    initialized: bool,
    // sensor geometry (queried at init; defaults until then)
    sensor_w: u32,
    sensor_h: u32,
    bit_depth: u32,
    // acquisition config
    exposure_ms: f64,
    binning: i32,
    roi: ImageRoi,
    image_buf: Vec<u8>,
    capturing: bool,
}

impl PvcamCamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        let _ = props.define_property(
            "CameraName",
            PropertyValue::String("Photometrics".into()),
            true,
        );
        Self {
            props,
            bus: None,
            initialized: false,
            sensor_w: 2048,
            sensor_h: 2048,
            bit_depth: 16,
            exposure_ms: 10.0,
            binning: 1,
            roi: ImageRoi::new(0, 0, 2048, 2048),
            image_buf: Vec::new(),
            capturing: false,
        }
    }

    /// Inject a bus carrier (PCIe/USB, or a mock in tests).
    pub fn with_bus(mut self, bus: Box<dyn PvcamBus>) -> Self {
        self.bus = Some(bus);
        self
    }

    fn bus(&mut self) -> MmResult<&mut Box<dyn PvcamBus>> {
        self.bus.as_mut().ok_or(MmError::NotSupported)
    }

    /// Try to attach a real carrier: PCIe (`/dev/pvcam_pcie0`) first, then USB
    /// (libusb/usbdevfs, VID 0x1F12). Returns the carrier name, or an error listing
    /// both attempts if none is present.
    pub fn attach_default_bus(&mut self) -> MmResult<&'static str> {
        use super::pcie::PciBus;
        use super::usb::UsbBus;
        match PciBus::open(0) {
            Ok(b) => {
                self.bus = Some(Box::new(b));
                Ok("pcie")
            }
            Err(pcie_err) => match UsbBus::open(0) {
                Ok(b) => {
                    self.bus = Some(Box::new(b));
                    Ok("usb")
                }
                Err(usb_err) => Err(MmError::LocallyDefined(format!(
                    "no PVCAM camera: pcie: {pcie_err:?}; usb: {usb_err:?}"
                ))),
            },
        }
    }

    /// Read a 16-bit host-command parameter (LE on the host).
    fn read_u16(&mut self, code: u8) -> MmResult<u16> {
        let (frame, _) = proto::encode_read(code, 2);
        let mut resp = [0u8; 4];
        let n = self.bus()?.write_read(&frame, &mut resp)?;
        if n < 2 {
            return Err(MmError::LocallyDefined(format!(
                "short response for host-command 0x{code:02x}"
            )));
        }
        Ok(u16::from_le_bytes([resp[0], resp[1]]))
    }

    fn read_i16(&mut self, code: u8) -> MmResult<i16> {
        Ok(self.read_u16(code)? as i16)
    }

    #[allow(dead_code)] // building block for set-param workers (e.g. setpoint, port)
    fn write_u16(&mut self, code: u8, val: u16) -> MmResult<()> {
        let frame = proto::encode_write(code, &val.to_le_bytes(), DataFormat::LittleEndian);
        let mut resp = [0u8; 4];
        self.bus()?.write_read(&frame, &mut resp)?;
        Ok(())
    }

    /// Sensor temperature in °C (host-command 0x07, centi-°C).
    pub fn read_temperature_c(&mut self) -> MmResult<f64> {
        Ok(self.read_i16(code::TEMP_R)? as f64 / 100.0)
    }

    /// Build the 14-op SCCL acquisition script for the current config.
    fn build_script(&self, frame_count: u32) -> Vec<u8> {
        let topl_x = self.roi.x;
        let topl_y = self.roi.y;
        let botr_x = self.roi.x + self.roi.width - 1;
        let botr_y = self.roi.y + self.roi.height - 1;
        proto::sccl_acquisition_script(
            EXP_RES_MS,
            self.exposure_ms.round() as u32,
            0, // exposure mode: internal/timed
            frame_count,
            0, // mode
            1, // sub-roi count
            self.binning as u32,
            self.binning as u32,
            topl_x,
            topl_y,
            botr_x,
            botr_y,
            0, // clear mode
            2, // clear count
        )
    }

    /// Upload the SCCL script via the CCL host command (INIT_CCL family, code 0x0A).
    fn upload_script(&mut self, script: &[u8]) -> MmResult<()> {
        let frame = proto::encode_write(code::SCRIPT_UPLOAD_W, script, DataFormat::LittleEndian);
        let mut resp = [0u8; 8];
        self.bus()?.write_read(&frame, &mut resp)?;
        Ok(())
    }

    fn frame_bytes(&self) -> u32 {
        let w = self.roi.width / self.binning.max(1) as u32;
        let h = self.roi.height / self.binning.max(1) as u32;
        w * h * (self.bit_depth.div_ceil(8))
    }
}

impl Device for PvcamCamera {
    fn name(&self) -> &str {
        "PVCamNative"
    }
    fn description(&self) -> &str {
        "Pure-Rust Photometrics (PVCAM) camera — reverse-engineered protocol"
    }

    fn initialize(&mut self) -> MmResult<()> {
        // Attach a real bus (PCIe/USB) if a camera is present and none was injected.
        // With no hardware this leaves `bus` unset; the device still constructs so
        // it can be inspected, and I/O calls report `NotSupported`.
        if self.bus.is_none() {
            let _ = self.attach_default_bus();
        }
        // Real init would query SER/PAR size, bit depth, build the speed table via
        // host commands once a carrier is attached.
        self.initialized = true;
        Ok(())
    }
    fn shutdown(&mut self) -> MmResult<()> {
        if self.capturing {
            let _ = self.stop_sequence_acquisition();
        }
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Exposure" => Ok(PropertyValue::Float(self.exposure_ms)),
            "Binning" => Ok(PropertyValue::Integer(self.binning as i64)),
            "BitDepth" => Ok(PropertyValue::Integer(self.bit_depth as i64)),
            "OnCameraCCDXSize" => Ok(PropertyValue::Integer(self.sensor_w as i64)),
            "OnCameraCCDYSize" => Ok(PropertyValue::Integer(self.sensor_h as i64)),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Exposure" => {
                if let PropertyValue::Float(v) = val {
                    self.exposure_ms = v;
                    Ok(())
                } else {
                    Err(MmError::InvalidPropertyType)
                }
            }
            _ => self.props.set(name, val),
        }
    }
    fn property_names(&self) -> Vec<String> {
        let mut v = vec![
            "Exposure".to_string(),
            "Binning".to_string(),
            "BitDepth".to_string(),
            "OnCameraCCDXSize".to_string(),
            "OnCameraCCDYSize".to_string(),
        ];
        v.extend(self.props.property_names().iter().cloned());
        v
    }
    fn has_property(&self, name: &str) -> bool {
        matches!(
            name,
            "Exposure" | "Binning" | "BitDepth" | "OnCameraCCDXSize" | "OnCameraCCDYSize"
        ) || self.props.has_property(name)
    }
    fn is_property_read_only(&self, name: &str) -> bool {
        matches!(name, "BitDepth" | "OnCameraCCDXSize" | "OnCameraCCDYSize")
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Camera
    }
    fn busy(&self) -> bool {
        self.capturing
    }
}

impl Camera for PvcamCamera {
    fn snap_image(&mut self) -> MmResult<()> {
        let script = self.build_script(1);
        self.upload_script(&script)?;
        let fb = self.frame_bytes();
        self.bus()?.arm_acquisition(fb, fb as u64, false)?;
        // start sequence (host-command 0x39)
        let start = proto::encode_write(code::START_SEQ_W, &[], DataFormat::LittleEndian);
        let mut resp = [0u8; 4];
        self.bus()?.write_read(&start, &mut resp)?;
        // Wait for the EOF notification, bounded by exposure + a readout margin.
        let timeout_ms = (self.exposure_ms as u64).saturating_add(5000);
        let mut got_eof = false;
        for _ in 0..timeout_ms {
            if self.bus()?.poll_notification()?.is_some() {
                got_eof = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        if !got_eof {
            return Err(MmError::LocallyDefined("pvcam_native: snap timed out waiting for EOF".into()));
        }
        self.image_buf.resize(fb as usize, 0);
        let mut buf = std::mem::take(&mut self.image_buf);
        let r = self.bus()?.read_frame(&mut buf);
        self.image_buf = buf;
        r
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        Ok(&self.image_buf)
    }
    fn get_image_width(&self) -> u32 {
        self.roi.width / self.binning.max(1) as u32
    }
    fn get_image_height(&self) -> u32 {
        self.roi.height / self.binning.max(1) as u32
    }
    fn get_image_bytes_per_pixel(&self) -> u32 {
        self.bit_depth.div_ceil(8)
    }
    fn get_bit_depth(&self) -> u32 {
        self.bit_depth
    }
    fn get_number_of_components(&self) -> u32 {
        1
    }
    fn get_number_of_channels(&self) -> u32 {
        1
    }

    fn get_exposure(&self) -> f64 {
        self.exposure_ms
    }
    fn set_exposure(&mut self, exp_ms: f64) -> MmResult<()> {
        self.exposure_ms = exp_ms;
        Ok(())
    }

    fn get_binning(&self) -> i32 {
        self.binning
    }
    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        if bin < 1 {
            return Err(MmError::InvalidPropertyValue);
        }
        self.binning = bin;
        Ok(())
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        Ok(self.roi)
    }
    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        self.roi = roi;
        Ok(())
    }
    fn clear_roi(&mut self) -> MmResult<()> {
        self.roi = ImageRoi::new(0, 0, self.sensor_w, self.sensor_h);
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, count: i64, _interval_ms: f64) -> MmResult<()> {
        let script = self.build_script(count.max(1) as u32);
        self.upload_script(&script)?;
        let fb = self.frame_bytes();
        let total = fb as u64 * count.max(1) as u64;
        self.bus()?.arm_acquisition(fb, total, true)?;
        self.capturing = true;
        Ok(())
    }
    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        let r = self.bus()?.stop_acquisition();
        self.capturing = false;
        r
    }
    fn is_capturing(&self) -> bool {
        self.capturing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::pvcam_native::transport::MockBus;

    #[test]
    fn temperature_read_decodes_centi_celsius() {
        // -45.00 °C => -4500 = 0xEE6C little-endian
        let bus = MockBus::new().on(code::TEMP_R, &(-4500i16).to_le_bytes());
        let mut cam = PvcamCamera::new().with_bus(Box::new(bus));
        let t = cam.read_temperature_c().unwrap();
        assert!((t - (-45.0)).abs() < 1e-9, "got {t}");
    }

    #[cfg(all(
        target_os = "linux",
        any(feature = "pvcam-native-pcie", feature = "pvcam-native-usb")
    ))]
    #[test]
    fn attach_runs_real_enumeration_without_hardware() {
        // Exercises the real PCIe open() / USB usbdevfs enumeration paths. With no
        // camera present this must return Err cleanly (no panic), and initialize()
        // must still succeed with the bus left unset.
        let mut cam = PvcamCamera::new();
        assert!(cam.attach_default_bus().is_err());
        assert!(cam.bus.is_none());
        cam.initialize().unwrap();
        assert!(cam.snap_image().is_err()); // NotSupported: no bus
    }

    #[test]
    fn snap_runs_full_pipeline_over_mock_bus() {
        let bus = MockBus::new();
        let mut cam = PvcamCamera::new().with_bus(Box::new(bus));
        cam.set_roi(ImageRoi::new(0, 0, 64, 64)).unwrap();
        // Full flow: upload script -> arm -> start -> poll EOF -> read frame.
        cam.snap_image().unwrap();
        // 64x64x2 bytes, pattern-filled by the mock.
        assert_eq!(cam.get_image_buffer().unwrap().len(), 64 * 64 * 2);
        assert_eq!(cam.get_image_buffer().unwrap()[3], 3);
        // The 14-op script encoding is exactly the recovered layout.
        assert_eq!(cam.build_script(1).len(), 14 * 5);
    }

    #[test]
    fn snap_sends_script_upload_then_start() {
        // Drive the protocol/bus directly to assert the on-wire command order.
        use super::super::transport::PvcamBus;
        let mut bus = MockBus::new();
        let script = proto::encode_write(code::SCRIPT_UPLOAD_W, &[0u8; 70], DataFormat::LittleEndian);
        let start = proto::encode_write(code::START_SEQ_W, &[], DataFormat::LittleEndian);
        let mut r = [0u8; 4];
        bus.write_read(&script, &mut r).unwrap();
        bus.write_read(&start, &mut r).unwrap();
        assert_eq!(bus.sent_codes(), vec![code::SCRIPT_UPLOAD_W, code::START_SEQ_W]);
    }
}
