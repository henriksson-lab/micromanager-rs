use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Camera, Device};
use crate::types::{DeviceType, ImageRoi, PropertyValue};

use super::ffi;

// SAFETY: SpotCamera holds a raw pointer to SpotCtx.  SpotCam is a global
// (non-thread-safe) API; we enforce single-thread access via `&mut self`.
unsafe impl Send for SpotCamera {}

const BUF: usize = 256;

fn read_str_idx<F: FnOnce(i32, *mut i8, i32) -> i32>(idx: i32, f: F) -> Option<String> {
    let mut buf = [0i8; BUF];
    if f(idx, buf.as_mut_ptr(), BUF as i32) != 0 {
        return None;
    }
    let s = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) };
    Some(s.to_string_lossy().into_owned())
}

// ── Camera struct ──────────────────────────────────────────────────────────────

pub struct SpotCamera {
    props: PropertyMap,
    ctx: *mut ffi::SpotCtx,

    // Pre-init
    device_index: i32, // 0-based; -1 = first found
    exposure_ms: f64,
    gain_index: i32,
    binning: i32,
    auto_exposure: bool,
    auto_exp_image_type: String,
    trigger_mode: String,
    chip_defect_correction: bool,
    clearing_mode: String,
    noise_filter_threshold: i32,
    temperature_setpoint_c: f64,
    multishot_exposures_ms: [f64; 3],

    // Post-init read-only
    img_width: u32,
    img_height: u32,
    bit_depth: u32,
    image_buf: Vec<u8>,

    capturing: bool,
}

impl SpotCamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("CameraIndex", PropertyValue::Integer(-1), false)
            .unwrap();
        props
            .define_property("Exposure", PropertyValue::Float(10.0), false)
            .unwrap();
        props
            .define_property("Gain", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .define_property("Binning", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .define_property("PixelSize", PropertyValue::String("0x0 nm".into()), true)
            .unwrap();
        props
            .define_property("ActualGain", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("AutoExposure", PropertyValue::String("OFF".into()), false)
            .unwrap();
        props
            .set_allowed_values("AutoExposure", &["OFF", "ON"])
            .unwrap();
        props
            .define_property(
                "AutoExpImageType",
                PropertyValue::String("BRIGHTFIELD".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("AutoExpImageType", &["BRIGHTFIELD", "DARKFIELD"])
            .unwrap();
        props
            .define_property("TriggerMode", PropertyValue::String("NONE".into()), false)
            .unwrap();
        props
            .set_allowed_values("TriggerMode", &["NONE", "EDGE", "BULB"])
            .unwrap();
        props
            .define_property(
                "ChipDefectCorrection",
                PropertyValue::String("ON".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("ChipDefectCorrection", &["ON", "OFF"])
            .unwrap();
        props
            .define_property(
                "ClearingMode",
                PropertyValue::String("CONTINUOUS".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("ClearingMode", &["CONTINUOUS", "PREEMPTABLE", "NEVER"])
            .unwrap();
        props
            .define_property("NoiseFilterThreshold%", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .set_property_limits("NoiseFilterThreshold%", 0.0, 100.0)
            .unwrap();
        for color in ["R", "G", "B"] {
            props
                .define_property(
                    format!("ExposureTime for {color}"),
                    PropertyValue::Float(0.0),
                    false,
                )
                .unwrap();
        }
        props
            .define_property("Width", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("Height", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("BitDepth", PropertyValue::Integer(16), false)
            .unwrap();
        props
            .set_allowed_values("BitDepth", &["8", "16", "24", "48"])
            .unwrap();
        props
            .define_property("Temperature", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("TemperatureSetpoint", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("SerialNumber", PropertyValue::String("".into()), true)
            .unwrap();
        props
            .define_property("ModelName", PropertyValue::String("".into()), true)
            .unwrap();

        Self {
            props,
            ctx: std::ptr::null_mut(),
            device_index: -1,
            exposure_ms: 10.0,
            gain_index: 1,
            binning: 1,
            auto_exposure: false,
            auto_exp_image_type: "BRIGHTFIELD".into(),
            trigger_mode: "NONE".into(),
            chip_defect_correction: true,
            clearing_mode: "CONTINUOUS".into(),
            noise_filter_threshold: 0,
            temperature_setpoint_c: 0.0,
            multishot_exposures_ms: [0.0; 3],
            img_width: 0,
            img_height: 0,
            bit_depth: 16,
            image_buf: Vec::new(),
            capturing: false,
        }
    }

    fn check_open(&self) -> MmResult<()> {
        if self.ctx.is_null() {
            Err(MmError::NotConnected)
        } else {
            Ok(())
        }
    }

    fn sync_dims(&mut self) {
        if self.ctx.is_null() {
            return;
        }
        self.img_width = unsafe { ffi::spot_get_image_width(self.ctx) } as u32;
        self.img_height = unsafe { ffi::spot_get_image_height(self.ctx) } as u32;
        self.props
            .entry_mut("Width")
            .map(|e| e.value = PropertyValue::Integer(self.img_width as i64));
        self.props
            .entry_mut("Height")
            .map(|e| e.value = PropertyValue::Integer(self.img_height as i64));
    }

    fn snap_timeout_ms(&self) -> i32 {
        (self.exposure_ms as i32 + 10_000).max(10_000)
    }

    fn copy_frame_from_ctx(&mut self) -> MmResult<()> {
        let ptr = unsafe { ffi::spot_get_frame_ptr(self.ctx) };
        if ptr.is_null() {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        let bytes = unsafe { ffi::spot_get_frame_bytes(self.ctx) } as usize;
        if bytes == 0 {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        self.image_buf.resize(bytes, 0);
        unsafe { std::ptr::copy_nonoverlapping(ptr, self.image_buf.as_mut_ptr(), bytes) };
        Ok(())
    }
}

impl Default for SpotCamera {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SpotCamera {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            unsafe { ffi::spot_close(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
    }
}

// ── Device trait ───────────────────────────────────────────────────────────────

impl Device for SpotCamera {
    fn name(&self) -> &str {
        "SpotCamera"
    }
    fn description(&self) -> &str {
        "Diagnostic Instruments Spot camera (SpotCam SDK)"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.ctx.is_null() {
            return Ok(());
        }

        // Discover cameras.
        let count = unsafe { ffi::spot_find_devices() };
        if count < 0 {
            return Err(MmError::LocallyDefined(
                "SpotCam: device discovery failed".into(),
            ));
        }
        if count == 0 {
            return Err(MmError::LocallyDefined("SpotCam: no cameras found".into()));
        }

        // Select camera by index (default: 0).
        let idx = if self.device_index < 0 {
            0
        } else {
            self.device_index
        };
        if idx >= count {
            return Err(MmError::LocallyDefined(format!(
                "SpotCam: device index {} out of range (found {})",
                idx, count
            )));
        }

        let ctx = unsafe { ffi::spot_open(idx) };
        if ctx.is_null() {
            return Err(MmError::LocallyDefined(format!(
                "SpotCam: failed to open device {}",
                idx
            )));
        }
        self.ctx = ctx;
        self.device_index = idx;
        self.props
            .entry_mut("CameraIndex")
            .map(|e| e.value = PropertyValue::Integer(idx as i64));

        // Read static properties.
        self.bit_depth = unsafe { ffi::spot_get_bit_depth(ctx) }.max(1) as u32;
        self.props
            .entry_mut("BitDepth")
            .map(|e| e.value = PropertyValue::Integer(self.bit_depth as i64));

        if let Some(sn) = read_str_idx(idx, |i, b, l| unsafe {
            ffi::spot_get_serial_number(i, b, l)
        }) {
            self.props
                .entry_mut("SerialNumber")
                .map(|e| e.value = PropertyValue::String(sn));
        }
        if let Some(mn) = read_str_idx(idx, |i, b, l| unsafe { ffi::spot_get_device_name(i, b, l) })
        {
            self.props
                .entry_mut("ModelName")
                .map(|e| e.value = PropertyValue::String(mn));
        }

        // Allowed gain values.
        let gain_max = unsafe { ffi::spot_get_gain_max(ctx) }.max(1);
        let allowed: Vec<String> = (1..=gain_max).map(|g| g.to_string()).collect();
        let refs: Vec<&str> = allowed.iter().map(|s| s.as_str()).collect();
        self.props.set_allowed_values("Gain", &refs).ok();

        // Apply pre-init settings.
        unsafe { ffi::spot_set_exposure_ms(ctx, self.exposure_ms) };
        unsafe { ffi::spot_set_gain(ctx, self.gain_index) };
        unsafe { ffi::spot_set_binning(ctx, self.binning) };
        self.sync_dims();

        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if !self.ctx.is_null() {
            unsafe { ffi::spot_close(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "CameraIndex" => Ok(PropertyValue::Integer(self.device_index as i64)),
            "Exposure" => Ok(PropertyValue::Float(self.exposure_ms)),
            "Gain" => Ok(PropertyValue::Integer(self.gain_index as i64)),
            "Binning" => Ok(PropertyValue::Integer(self.binning as i64)),
            "AutoExposure" => Ok(PropertyValue::String(
                if self.auto_exposure { "ON" } else { "OFF" }.into(),
            )),
            "AutoExpImageType" => Ok(PropertyValue::String(self.auto_exp_image_type.clone())),
            "TriggerMode" => Ok(PropertyValue::String(self.trigger_mode.clone())),
            "ChipDefectCorrection" => {
                let mode = if self.chip_defect_correction {
                    "ON"
                } else {
                    "OFF"
                };
                Ok(PropertyValue::String(mode.into()))
            }
            "ClearingMode" => Ok(PropertyValue::String(self.clearing_mode.clone())),
            "NoiseFilterThreshold%" => {
                Ok(PropertyValue::Integer(self.noise_filter_threshold as i64))
            }
            "BitDepth" => Ok(PropertyValue::Integer(self.bit_depth as i64)),
            "Temperature" => {
                let t = if self.ctx.is_null() {
                    0.0f64
                } else {
                    (unsafe { ffi::spot_get_temperature_c(self.ctx) }) as f64
                };
                Ok(PropertyValue::Float(t))
            }
            "TemperatureSetpoint" => Ok(PropertyValue::Float(self.temperature_setpoint_c)),
            "ExposureTime for R" => Ok(PropertyValue::Float(self.multishot_exposures_ms[0])),
            "ExposureTime for G" => Ok(PropertyValue::Float(self.multishot_exposures_ms[1])),
            "ExposureTime for B" => Ok(PropertyValue::Float(self.multishot_exposures_ms[2])),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "CameraIndex" => {
                if !self.ctx.is_null() {
                    return Err(MmError::LocallyDefined(
                        "CameraIndex cannot be changed after initialize()".into(),
                    ));
                }
                self.device_index = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props.set(name, val)
            }
            "Exposure" => {
                self.exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.exposure_ms))?;
                if !self.ctx.is_null() {
                    unsafe { ffi::spot_set_exposure_ms(self.ctx, self.exposure_ms) };
                }
                Ok(())
            }
            "Gain" => {
                self.gain_index = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props
                    .set(name, PropertyValue::Integer(self.gain_index as i64))?;
                if !self.ctx.is_null() {
                    unsafe { ffi::spot_set_gain(self.ctx, self.gain_index) };
                }
                Ok(())
            }
            "Binning" => {
                self.binning = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props
                    .set(name, PropertyValue::Integer(self.binning as i64))?;
                if !self.ctx.is_null() {
                    unsafe { ffi::spot_set_binning(self.ctx, self.binning) };
                    self.sync_dims();
                }
                Ok(())
            }
            "AutoExposure" => {
                let mode = val.as_str();
                self.props
                    .set(name, PropertyValue::String(mode.to_string()))?;
                self.auto_exposure = mode == "ON";
                Ok(())
            }
            "AutoExpImageType" => {
                let image_type = val.as_str().to_string();
                self.props
                    .set(name, PropertyValue::String(image_type.clone()))?;
                self.auto_exp_image_type = image_type;
                Ok(())
            }
            "TriggerMode" => {
                let mode = val.as_str().to_string();
                self.props.set(name, PropertyValue::String(mode.clone()))?;
                self.trigger_mode = mode;
                Ok(())
            }
            "ChipDefectCorrection" => {
                let mode = val.as_str().to_string();
                self.props.set(name, PropertyValue::String(mode.clone()))?;
                self.chip_defect_correction = mode == "ON";
                Ok(())
            }
            "ClearingMode" => {
                let mode = val.as_str().to_string();
                self.props.set(name, PropertyValue::String(mode.clone()))?;
                self.clearing_mode = mode;
                Ok(())
            }
            "NoiseFilterThreshold%" => {
                self.noise_filter_threshold =
                    val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props.set(
                    name,
                    PropertyValue::Integer(self.noise_filter_threshold as i64),
                )
            }
            "BitDepth" => {
                let depth = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32;
                self.props.set(name, PropertyValue::Integer(depth as i64))?;
                self.bit_depth = depth;
                Ok(())
            }
            "ExposureTime for R" | "ExposureTime for G" | "ExposureTime for B" => {
                let exposure = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                let idx = match name {
                    "ExposureTime for R" => 0,
                    "ExposureTime for G" => 1,
                    "ExposureTime for B" => 2,
                    _ => unreachable!(),
                };
                self.multishot_exposures_ms[idx] = exposure;
                self.props.set(name, PropertyValue::Float(exposure))
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> {
        self.props.property_names().to_vec()
    }
    fn has_property(&self, name: &str) -> bool {
        self.props.has_property(name)
    }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Camera
    }
    fn busy(&self) -> bool {
        false
    }
}

// ── Camera trait ───────────────────────────────────────────────────────────────

impl Camera for SpotCamera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_open()?;
        if self.capturing {
            // SpotCam has no native continuous mode — re-snap each frame.
            return self.snap_image_single();
        }
        self.snap_image_single()
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if self.image_buf.is_empty() {
            Err(MmError::LocallyDefined("No image captured yet".into()))
        } else {
            Ok(&self.image_buf)
        }
    }

    fn get_image_width(&self) -> u32 {
        self.img_width
    }
    fn get_image_height(&self) -> u32 {
        self.img_height
    }
    fn get_image_bytes_per_pixel(&self) -> u32 {
        if self.bit_depth > 8 {
            2
        } else {
            1
        }
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

    fn set_exposure(&mut self, exp_ms: f64) {
        self.exposure_ms = exp_ms;
        self.props
            .set("Exposure", PropertyValue::Float(exp_ms))
            .ok();
        if !self.ctx.is_null() {
            unsafe { ffi::spot_set_exposure_ms(self.ctx, exp_ms) };
        }
    }

    fn get_binning(&self) -> i32 {
        self.binning
    }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        self.binning = bin;
        self.props
            .set("Binning", PropertyValue::Integer(bin as i64))?;
        if !self.ctx.is_null() {
            unsafe { ffi::spot_set_binning(self.ctx, bin) };
            self.sync_dims();
        }
        Ok(())
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        Ok(ImageRoi::new(0, 0, self.img_width, self.img_height))
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        self.check_open()?;
        let rc = unsafe {
            ffi::spot_set_roi(
                self.ctx,
                roi.x as i32,
                roi.y as i32,
                roi.width as i32,
                roi.height as i32,
            )
        };
        if rc != 0 {
            return Err(MmError::Err);
        }
        self.img_width = roi.width;
        self.img_height = roi.height;
        self.props
            .entry_mut("Width")
            .map(|e| e.value = PropertyValue::Integer(roi.width as i64));
        self.props
            .entry_mut("Height")
            .map(|e| e.value = PropertyValue::Integer(roi.height as i64));
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        self.check_open()?;
        unsafe { ffi::spot_clear_roi(self.ctx) };
        self.sync_dims();
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, _count: i64, _interval_ms: f64) -> MmResult<()> {
        self.check_open()?;
        // SpotCam has no hardware continuous mode; flag capturing so the
        // caller can repeatedly call snap_image().
        self.capturing = true;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        self.capturing = false;
        Ok(())
    }

    fn is_capturing(&self) -> bool {
        self.capturing
    }
}

impl SpotCamera {
    fn snap_image_single(&mut self) -> MmResult<()> {
        let timeout = self.snap_timeout_ms();
        let rc = unsafe { ffi::spot_snap(self.ctx, timeout) };
        if rc != 0 {
            return Err(MmError::SnapImageFailed);
        }
        self.sync_dims();
        self.copy_frame_from_ctx()?;
        Ok(())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_properties() {
        let d = SpotCamera::new();
        assert_eq!(d.device_type(), DeviceType::Camera);
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(d.get_binning(), 1);
        assert!(!d.is_capturing());
        assert_eq!(d.get_number_of_components(), 1);
    }

    #[test]
    fn set_camera_index_pre_init() {
        let mut d = SpotCamera::new();
        d.set_property("CameraIndex", PropertyValue::Integer(1))
            .unwrap();
        assert_eq!(d.device_index, 1);
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = SpotCamera::new();
        d.set_property("Exposure", PropertyValue::Float(25.0))
            .unwrap();
        assert_eq!(d.exposure_ms, 25.0);
        assert_eq!(d.get_exposure(), 25.0);
    }

    #[test]
    fn set_gain_pre_init() {
        let mut d = SpotCamera::new();
        d.set_property("Gain", PropertyValue::Integer(3)).unwrap();
        assert_eq!(d.gain_index, 3);
    }

    #[test]
    fn set_binning_pre_init() {
        let mut d = SpotCamera::new();
        d.set_property("Binning", PropertyValue::Integer(2))
            .unwrap();
        assert_eq!(d.binning, 2);
        assert_eq!(d.get_binning(), 2);
    }

    #[test]
    fn snap_without_init_errors() {
        let mut d = SpotCamera::new();
        assert!(d.snap_image().is_err());
    }

    #[test]
    fn no_image_before_snap() {
        let d = SpotCamera::new();
        assert!(d.get_image_buffer().is_err());
    }

    #[test]
    fn initialize_no_camera_fails() {
        let mut d = SpotCamera::new();
        // No SpotCam cameras present — expect an error.
        assert!(d.initialize().is_err());
    }

    #[test]
    fn readonly_properties() {
        let d = SpotCamera::new();
        assert!(d.is_property_read_only("Width"));
        assert!(d.is_property_read_only("Height"));
        assert!(!d.is_property_read_only("BitDepth"));
        assert!(d.is_property_read_only("PixelSize"));
        assert!(d.is_property_read_only("ActualGain"));
        assert!(d.is_property_read_only("Temperature"));
        assert!(d.is_property_read_only("TemperatureSetpoint"));
        assert!(d.is_property_read_only("SerialNumber"));
        assert!(d.is_property_read_only("ModelName"));
        assert!(!d.is_property_read_only("Exposure"));
        assert!(!d.is_property_read_only("Gain"));
        assert!(!d.is_property_read_only("Binning"));
        assert!(!d.is_property_read_only("AutoExposure"));
        assert!(!d.is_property_read_only("AutoExpImageType"));
        assert!(!d.is_property_read_only("TriggerMode"));
        assert!(!d.is_property_read_only("ChipDefectCorrection"));
        assert!(!d.is_property_read_only("ClearingMode"));
        assert!(!d.is_property_read_only("NoiseFilterThreshold%"));
    }

    #[test]
    fn exposure_ms_roundtrip() {
        // Verify ns ↔ ms conversion math.
        let ms = 50.0_f64;
        let inc_ns: u64 = 1; // 1 ns increment
        let ticks = (ms * 1e6 / inc_ns as f64 + 0.5) as u64;
        let back_ms = ticks as f64 * inc_ns as f64 / 1e6;
        assert!((back_ms - ms).abs() < 0.001);
    }

    #[test]
    fn bytes_per_pixel_from_bit_depth() {
        let mut d = SpotCamera::new();
        d.bit_depth = 16;
        assert_eq!(d.get_image_bytes_per_pixel(), 2);
        d.bit_depth = 8;
        assert_eq!(d.get_image_bytes_per_pixel(), 1);
    }

    #[test]
    fn upstream_auto_exposure_surface() {
        let mut d = SpotCamera::new();
        assert_eq!(
            d.get_property("AutoExposure").unwrap(),
            PropertyValue::String("OFF".into())
        );
        assert_eq!(
            d.get_property("AutoExpImageType").unwrap(),
            PropertyValue::String("BRIGHTFIELD".into())
        );

        d.set_property("AutoExposure", PropertyValue::String("ON".into()))
            .unwrap();
        assert_eq!(
            d.get_property("AutoExposure").unwrap(),
            PropertyValue::String("ON".into())
        );
        assert!(d
            .set_property("AutoExposure", PropertyValue::String("AUTO".into()))
            .is_err());

        d.set_property(
            "AutoExpImageType",
            PropertyValue::String("DARKFIELD".into()),
        )
        .unwrap();
        assert_eq!(
            d.get_property("AutoExpImageType").unwrap(),
            PropertyValue::String("DARKFIELD".into())
        );
        assert!(d
            .set_property("AutoExpImageType", PropertyValue::String("FLAT".into()))
            .is_err());
    }

    #[test]
    fn upstream_readonly_info_surface() {
        let d = SpotCamera::new();
        assert_eq!(
            d.get_property("PixelSize").unwrap(),
            PropertyValue::String("0x0 nm".into())
        );
        assert_eq!(
            d.get_property("ActualGain").unwrap(),
            PropertyValue::Float(0.0)
        );
    }

    #[test]
    fn upstream_spot_control_surface() {
        let mut d = SpotCamera::new();
        for name in [
            "TriggerMode",
            "ChipDefectCorrection",
            "ClearingMode",
            "NoiseFilterThreshold%",
            "TemperatureSetpoint",
            "ExposureTime for R",
            "ExposureTime for G",
            "ExposureTime for B",
        ] {
            assert!(d.has_property(name), "missing {name}");
        }

        d.set_property("TriggerMode", PropertyValue::String("EDGE".into()))
            .unwrap();
        assert_eq!(
            d.get_property("TriggerMode").unwrap(),
            PropertyValue::String("EDGE".into())
        );
        assert!(d
            .set_property("TriggerMode", PropertyValue::String("LEVEL".into()))
            .is_err());

        d.set_property("ChipDefectCorrection", PropertyValue::String("OFF".into()))
            .unwrap();
        assert_eq!(
            d.get_property("ChipDefectCorrection").unwrap(),
            PropertyValue::String("OFF".into())
        );

        d.set_property("ClearingMode", PropertyValue::String("NEVER".into()))
            .unwrap();
        assert_eq!(
            d.get_property("ClearingMode").unwrap(),
            PropertyValue::String("NEVER".into())
        );

        d.set_property("NoiseFilterThreshold%", PropertyValue::Integer(100))
            .unwrap();
        assert!(d
            .set_property("NoiseFilterThreshold%", PropertyValue::Integer(101))
            .is_err());

        d.set_property("BitDepth", PropertyValue::Integer(24))
            .unwrap();
        assert_eq!(d.get_bit_depth(), 24);
        assert!(d
            .set_property("BitDepth", PropertyValue::Integer(32))
            .is_err());

        d.set_property("ExposureTime for G", PropertyValue::Float(12.5))
            .unwrap();
        assert_eq!(
            d.get_property("ExposureTime for G").unwrap(),
            PropertyValue::Float(12.5)
        );
    }
}
