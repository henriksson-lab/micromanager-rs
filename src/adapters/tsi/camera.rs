use std::ffi::{CStr, CString};

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Camera, Device};
use crate::types::{DeviceType, ImageRoi, PropertyType, PropertyValue};

use super::ffi;

// SAFETY: TSICamera holds a raw pointer to TsiCtx.  The TSI SDK is not
// internally thread-safe per camera handle; we enforce single-thread access
// via `&mut self` on all mutating methods.
unsafe impl Send for TSICamera {}

const BUF: usize = 256;
const TSI_TRIGGER_SOFTWARE: i32 = 1;
const TSI_TRIGGER_HARDWARE_STANDARD: i32 = 2;
const TSI_TRIGGER_HARDWARE_BULB: i32 = 3;
const TSI_POLARITY_POSITIVE: i32 = 1;
const TSI_POLARITY_NEGATIVE: i32 = 0;

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

fn exposure_ms_to_us(exp_ms: f64) -> i64 {
    (exp_ms * 1_000.0 + 0.5) as i64
}

fn validate_exposure_ms(exp_ms: f64) -> MmResult<f64> {
    if !exp_ms.is_finite() || exp_ms <= 0.0 {
        return Err(MmError::InvalidPropertyValue);
    }
    Ok(exp_ms)
}

fn validate_i32_property_value(val: &PropertyValue) -> MmResult<i32> {
    match val {
        PropertyValue::Integer(v) => i32::try_from(*v).map_err(|_| MmError::InvalidPropertyValue),
        PropertyValue::Float(v) if v.is_finite() && v.fract() == 0.0 => {
            if *v < i32::MIN as f64 || *v > i32::MAX as f64 {
                Err(MmError::InvalidPropertyValue)
            } else {
                Ok(*v as i32)
            }
        }
        PropertyValue::String(s) => s.parse::<i32>().map_err(|_| MmError::InvalidPropertyValue),
        _ => Err(MmError::InvalidPropertyValue),
    }
}

fn read_str<F: FnOnce(*mut i8, i32) -> i32>(f: F) -> Option<String> {
    let mut buf = [0i8; BUF];
    if f(buf.as_mut_ptr(), BUF as i32) != 0 {
        return None;
    }
    let s = unsafe { CStr::from_ptr(buf.as_ptr()) };
    Some(s.to_string_lossy().into_owned())
}

fn sensor_type_name(t: i32) -> &'static str {
    match t {
        1 => "Bayer",
        2 => "Polarized",
        _ => "Monochrome",
    }
}

fn disables_binning(sensor_type: i32) -> bool {
    sensor_type == 1
}

fn pixel_type_bytes_and_depth(pixel_type: &str, default_bit_depth: u32) -> Option<(u32, u32)> {
    match pixel_type {
        "Mono16" => Some((2, default_bit_depth.max(1))),
        "RGBA32" => Some((4, 8)),
        "RGBA64" => Some((8, default_bit_depth.max(1))),
        _ => None,
    }
}

fn trigger_mode_code(mode: &str) -> Option<i32> {
    match mode {
        "Software" => Some(TSI_TRIGGER_SOFTWARE),
        "HardwareEdge" | "HardwareStandard" => Some(TSI_TRIGGER_HARDWARE_STANDARD),
        "HardwareDuration" | "HardwareBulb" => Some(TSI_TRIGGER_HARDWARE_BULB),
        _ => None,
    }
}

fn trigger_mode_name(code: i32) -> Option<&'static str> {
    match code {
        TSI_TRIGGER_SOFTWARE => Some("Software"),
        TSI_TRIGGER_HARDWARE_STANDARD => Some("HardwareEdge"),
        TSI_TRIGGER_HARDWARE_BULB => Some("HardwareDuration"),
        _ => None,
    }
}

fn canonical_trigger_mode(mode: &str) -> Option<&'static str> {
    trigger_mode_code(mode).and_then(trigger_mode_name)
}

fn trigger_polarity_code(polarity: &str) -> Option<i32> {
    match polarity {
        "Positive" => Some(TSI_POLARITY_POSITIVE),
        "Negative" => Some(TSI_POLARITY_NEGATIVE),
        _ => None,
    }
}

fn trigger_polarity_name(code: i32) -> &'static str {
    if code == TSI_POLARITY_NEGATIVE {
        "Negative"
    } else {
        "Positive"
    }
}

fn on_off(enabled: bool) -> PropertyValue {
    PropertyValue::String(if enabled { "On" } else { "Off" }.into())
}

fn gain_db_string(gain_db: f64) -> String {
    gain_db.to_string()
}

// ── Camera struct ─────────────────────────────────────────────────────────────

pub struct TSICamera {
    props: PropertyMap,
    ctx: *mut ffi::TsiCtx,
    img_buf: Vec<u8>,

    // Pre-init / cached
    camera_id: String, // TSI camera ID string; "0" = first found, matching upstream
    camera_name: String,
    exposure_ms: f64,
    binning: i32,
    trigger_mode: String,
    trigger_polarity: String,
    hardware_trigger_supported: bool,
    pixel_type: String,

    // Post-init read-only
    img_width: u32,
    img_height: u32,
    bit_depth: u32,
    bytes_per_pixel: u32,
    sensor_type: i32,

    capturing: bool,
}

impl TSICamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("CameraID", PropertyValue::String("0".into()), false)
            .unwrap();
        props
            .define_property("CameraName", PropertyValue::String("".into()), true)
            .unwrap();
        props
            .define_property("Exposure", PropertyValue::Float(2.0), false)
            .unwrap();
        props
            .define_property("Binning", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .define_property(
                "TriggerMode",
                PropertyValue::String("Software".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values(
                "TriggerMode",
                &["Software", "HardwareEdge", "HardwareDuration"],
            )
            .unwrap();
        props
            .define_property(
                "TriggerPolarity",
                PropertyValue::String("Positive".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("TriggerPolarity", &["Positive", "Negative"])
            .unwrap();
        props
            .define_property("PixelType", PropertyValue::String("Mono16".into()), false)
            .unwrap();
        props
            .define_property("Width", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("Height", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("BitDepth", PropertyValue::Integer(16), true)
            .unwrap();
        props
            .define_property(
                "SensorType",
                PropertyValue::String("Monochrome".into()),
                true,
            )
            .unwrap();
        props
            .define_property("SerialNumber", PropertyValue::String("".into()), true)
            .unwrap();
        props
            .define_property("FirmwareVer", PropertyValue::String("".into()), true)
            .unwrap();
        props
            .define_property("FirmwareVersion", PropertyValue::String("".into()), true)
            .unwrap();

        Self {
            props,
            ctx: std::ptr::null_mut(),
            img_buf: Vec::new(),
            camera_id: "0".into(),
            camera_name: String::new(),
            exposure_ms: 2.0,
            binning: 1,
            trigger_mode: "Software".into(),
            trigger_polarity: "Positive".into(),
            hardware_trigger_supported: true,
            pixel_type: "Mono16".into(),
            img_width: 0,
            img_height: 0,
            bit_depth: 16,
            bytes_per_pixel: 2,
            sensor_type: 0,
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

    fn cleanup_failed_initialize(&mut self) {
        if !self.ctx.is_null() {
            unsafe { ffi::tsi_close_camera(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
        self.capturing = false;
        unsafe { ffi::tsi_sdk_close() };
    }

    fn initialize_error<T>(&mut self, err: MmError) -> MmResult<T> {
        self.cleanup_failed_initialize();
        Err(err)
    }

    fn sync_dims(&mut self) {
        if self.ctx.is_null() {
            return;
        }
        self.img_width = unsafe { ffi::tsi_get_image_width(self.ctx) } as u32;
        self.img_height = unsafe { ffi::tsi_get_image_height(self.ctx) } as u32;
        self.props
            .entry_mut("Width")
            .map(|e| e.value = PropertyValue::Integer(self.img_width as i64));
        self.props
            .entry_mut("Height")
            .map(|e| e.value = PropertyValue::Integer(self.img_height as i64));
    }

    fn apply_binning(&mut self) -> MmResult<()> {
        if self.ctx.is_null() {
            return Ok(());
        }
        let b = self.binning;
        let previous_x = unsafe { ffi::tsi_get_binx(self.ctx) };
        if unsafe { ffi::tsi_set_binx(self.ctx, b) } != 0 {
            return Err(MmError::LocallyDefined(
                "TSI: failed to set horizontal binning".into(),
            ));
        }
        if unsafe { ffi::tsi_set_biny(self.ctx, b) } != 0 {
            if previous_x > 0 {
                unsafe { ffi::tsi_set_binx(self.ctx, previous_x) };
            }
            self.sync_dims();
            return Err(MmError::LocallyDefined(
                "TSI: failed to set vertical binning".into(),
            ));
        }
        self.sync_dims();
        Ok(())
    }

    fn apply_trigger_mode(&mut self, mode: &str) -> MmResult<()> {
        let code = trigger_mode_code(mode).ok_or(MmError::InvalidPropertyValue)?;
        if self.ctx.is_null() {
            return Ok(());
        }
        if unsafe { ffi::tsi_set_operation_mode(self.ctx, code) } != 0 {
            return Err(MmError::LocallyDefined(
                "TSI: failed to set trigger mode".into(),
            ));
        }
        Ok(())
    }

    fn refresh_trigger_mode(&mut self) {
        if self.ctx.is_null() {
            return;
        }
        let current_mode = unsafe { ffi::tsi_get_operation_mode(self.ctx) };
        if let Some(mode) = trigger_mode_name(current_mode) {
            self.trigger_mode = mode.into();
            self.props
                .entry_mut("TriggerMode")
                .map(|e| e.value = PropertyValue::String(self.trigger_mode.clone()));
        }
    }

    fn refresh_trigger_polarity(&mut self) {
        if self.ctx.is_null() {
            return;
        }
        let current_polarity = unsafe { ffi::tsi_get_trigger_polarity(self.ctx) };
        if current_polarity == TSI_POLARITY_POSITIVE || current_polarity == TSI_POLARITY_NEGATIVE {
            self.trigger_polarity = trigger_polarity_name(current_polarity).into();
            self.props
                .entry_mut("TriggerPolarity")
                .map(|e| e.value = PropertyValue::String(self.trigger_polarity.clone()));
        }
    }

    fn trigger_polarity_property_exposed(&self) -> bool {
        self.ctx.is_null() || self.hardware_trigger_supported
    }

    fn apply_trigger_polarity(&mut self, polarity: &str) -> MmResult<()> {
        let code = trigger_polarity_code(polarity).ok_or(MmError::InvalidPropertyValue)?;
        if self.ctx.is_null() {
            return Ok(());
        }
        if unsafe { ffi::tsi_set_trigger_polarity(self.ctx, code) } != 0 {
            return Err(MmError::LocallyDefined(
                "TSI: failed to set trigger polarity".into(),
            ));
        }
        Ok(())
    }

    fn refresh_gain_property(&mut self) -> MmResult<()> {
        if self.ctx.is_null() || !self.props.has_property("Gain") {
            return Ok(());
        }

        let mut gain_db = 0.0f64;
        if unsafe { ffi::tsi_get_gain_db(self.ctx, &mut gain_db) } != 0 {
            return Err(MmError::LocallyDefined("TSI: failed to read gain".into()));
        }
        let property_type = self
            .props
            .entry("Gain")
            .map(|e| e.property_type)
            .unwrap_or(PropertyType::Float);
        let value = if property_type == PropertyType::String {
            PropertyValue::String(gain_db_string(gain_db))
        } else {
            PropertyValue::Float(gain_db)
        };
        self.props.entry_mut("Gain").map(|e| e.value = value);
        Ok(())
    }

    /// Snap timeout: exposure + generous readout overhead, minimum 5 s.
    fn snap_timeout_ms(&self) -> i32 {
        (self.exposure_ms as i32 + 5_000).max(5_000)
    }

    fn copy_frame_from_shim(&mut self) -> MmResult<()> {
        let ptr = unsafe { ffi::tsi_get_frame_ptr(self.ctx) };
        if ptr.is_null() {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        let bytes = unsafe { ffi::tsi_get_frame_bytes(self.ctx) } as usize;
        if bytes == 0 {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        let src = unsafe { std::slice::from_raw_parts(ptr as *const u8, bytes) };
        self.img_buf.clear();
        self.img_buf.extend_from_slice(src);
        Ok(())
    }
}

impl Default for TSICamera {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TSICamera {
    fn drop(&mut self) {
        let _ = self.stop_sequence_acquisition();
        if !self.ctx.is_null() {
            unsafe { ffi::tsi_close_camera(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
        unsafe { ffi::tsi_sdk_close() };
    }
}

// ── Device trait ──────────────────────────────────────────────────────────────

impl Device for TSICamera {
    fn name(&self) -> &str {
        "TSICamera"
    }
    fn description(&self) -> &str {
        "Thorlabs Scientific Imaging camera"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.ctx.is_null() {
            return Ok(());
        }

        if unsafe { ffi::tsi_sdk_open() } != 0 {
            return Err(MmError::LocallyDefined("tsi_sdk_open failed".into()));
        }

        // Discover cameras.
        let mut disc_buf = [0i8; 4096];
        let count = unsafe { ffi::tsi_discover_cameras(disc_buf.as_mut_ptr(), 4096) };
        if count < 0 {
            return self.initialize_error(MmError::LocallyDefined(
                "TSI: camera discovery failed".into(),
            ));
        }
        if count == 0 {
            return self.initialize_error(MmError::LocallyDefined("TSI: no cameras found".into()));
        }

        // Parse the space-separated ID list.
        let ids_str = unsafe { CStr::from_ptr(disc_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let ids: Vec<&str> = ids_str.split_whitespace().collect();

        // Select camera: match by pre-configured ID or take the first one.
        let target_id: &str = if self.camera_id == "0" || self.camera_id.is_empty() {
            ids[0]
        } else {
            match ids
                .iter()
                .find(|&&id| id == self.camera_id.as_str())
                .copied()
            {
                Some(id) => id,
                None => {
                    return self.initialize_error(MmError::LocallyDefined(format!(
                        "TSI: camera '{}' not found (available: {})",
                        self.camera_id, ids_str
                    )));
                }
            }
        };

        let id_cstr = cstr(target_id);
        let ctx = unsafe { ffi::tsi_open_camera(id_cstr.as_ptr()) };
        if ctx.is_null() {
            return self.initialize_error(MmError::LocallyDefined(format!(
                "TSI: failed to open camera '{}'",
                target_id
            )));
        }
        self.ctx = ctx;
        self.camera_name = target_id.to_string();
        self.props
            .entry_mut("CameraID")
            .map(|e| e.value = PropertyValue::String(self.camera_id.clone()));
        self.props
            .entry_mut("CameraName")
            .map(|e| e.value = PropertyValue::String(self.camera_name.clone()));

        // Read sensor properties.
        self.bit_depth = unsafe { ffi::tsi_get_bit_depth(ctx) }.max(1) as u32;
        self.bytes_per_pixel = unsafe { ffi::tsi_get_bytes_per_pixel(ctx) }.max(1) as u32;
        self.sensor_type = unsafe { ffi::tsi_get_sensor_type(ctx) };

        self.props
            .entry_mut("BitDepth")
            .map(|e| e.value = PropertyValue::Integer(self.bit_depth as i64));
        self.props
            .entry_mut("SensorType")
            .map(|e| e.value = PropertyValue::String(sensor_type_name(self.sensor_type).into()));
        self.pixel_type = if self.sensor_type == 1 {
            self.bytes_per_pixel = 4;
            self.bit_depth = 8;
            if disables_binning(self.sensor_type) {
                self.binning = 1;
                self.props
                    .entry_mut("Binning")
                    .map(|e| e.value = PropertyValue::Integer(1));
            }
            "RGBA32".into()
        } else {
            "Mono16".into()
        };
        self.props
            .entry_mut("BitDepth")
            .map(|e| e.value = PropertyValue::Integer(self.bit_depth as i64));
        self.props
            .entry_mut("PixelType")
            .map(|e| e.value = PropertyValue::String(self.pixel_type.clone()));
        if self.sensor_type == 1 {
            self.props.set_allowed_values("PixelType", &["RGBA32"]).ok();
        } else {
            self.props.set_allowed_values("PixelType", &["Mono16"]).ok();
        }

        if let Some(sn) = read_str(|b, l| unsafe { ffi::tsi_get_serial_number(ctx, b, l) }) {
            self.props
                .entry_mut("SerialNumber")
                .map(|e| e.value = PropertyValue::String(sn));
        }
        if let Some(fw) = read_str(|b, l| unsafe { ffi::tsi_get_firmware_version(ctx, b, l) }) {
            self.props
                .entry_mut("FirmwareVer")
                .map(|e| e.value = PropertyValue::String(fw.clone()));
            self.props
                .entry_mut("FirmwareVersion")
                .map(|e| e.value = PropertyValue::String(fw));
        }

        // Apply pre-init settings.
        let exp_us = exposure_ms_to_us(self.exposure_ms);
        if unsafe { ffi::tsi_set_exposure_us(ctx, exp_us) } != 0 {
            return self.initialize_error(MmError::LocallyDefined(
                "TSI: failed to set exposure".into(),
            ));
        }
        if let Err(err) = self.apply_binning() {
            return self.initialize_error(err);
        }
        self.sync_dims();

        let hardware_trigger_supported = unsafe {
            ffi::tsi_is_operation_mode_supported(ctx, TSI_TRIGGER_HARDWARE_STANDARD) == 1
        };
        self.hardware_trigger_supported = hardware_trigger_supported;
        if hardware_trigger_supported {
            self.props
                .set_allowed_values(
                    "TriggerMode",
                    &["Software", "HardwareEdge", "HardwareDuration"],
                )
                .ok();
        } else {
            self.props
                .set_allowed_values("TriggerMode", &["Software"])
                .ok();
            self.trigger_mode = "Software".into();
            self.props
                .entry_mut("TriggerMode")
                .map(|e| e.value = PropertyValue::String(self.trigger_mode.clone()));
        }
        let requested_trigger_mode = self.trigger_mode.clone();
        if let Err(err) = self.apply_trigger_mode(&requested_trigger_mode) {
            return self.initialize_error(err);
        }
        if self.hardware_trigger_supported {
            let requested_trigger_polarity = self.trigger_polarity.clone();
            if let Err(err) = self.apply_trigger_polarity(&requested_trigger_polarity) {
                return self.initialize_error(err);
            }
            self.refresh_trigger_polarity();
        }
        self.refresh_trigger_mode();

        // Populate binning allowed values from camera range.
        let mut hbin_min = 1i32;
        let mut hbin_max = 1i32;
        let mut vbin_min = 1i32;
        let mut vbin_max = 1i32;
        unsafe { ffi::tsi_get_binx_range(ctx, &mut hbin_min, &mut hbin_max) };
        unsafe { ffi::tsi_get_biny_range(ctx, &mut vbin_min, &mut vbin_max) };
        let bin_min = hbin_min.max(vbin_min).max(1);
        let bin_max = if disables_binning(self.sensor_type) {
            1
        } else {
            hbin_max.min(vbin_max).max(bin_min)
        };
        let allowed: Vec<String> = (bin_min..=bin_max).map(|b| b.to_string()).collect();
        if !allowed.is_empty() {
            let refs: Vec<&str> = allowed.iter().map(|s| s.as_str()).collect();
            self.props.set_allowed_values("Binning", &refs).ok();
        }

        let mut exp_min = 0i64;
        let mut exp_max = 0i64;
        if unsafe { ffi::tsi_get_exposure_range_us(ctx, &mut exp_min, &mut exp_max) } == 0 {
            self.props
                .set_property_limits(
                    "Exposure",
                    exp_min as f64 / 1_000.0,
                    exp_max as f64 / 1_000.0,
                )
                .ok();
        }

        if unsafe { ffi::tsi_is_eep_supported(ctx) } == 1 {
            if !self.props.has_property("EEP") {
                self.props
                    .define_property("EEP", PropertyValue::String("Off".into()), false)
                    .ok();
                self.props.set_allowed_values("EEP", &["Off", "On"]).ok();
            }
            let enabled = unsafe { ffi::tsi_get_eep_enabled(ctx) } == 1;
            self.props
                .entry_mut("EEP")
                .map(|e| e.value = on_off(enabled));
        }

        let mut hot_min = 0i32;
        let mut hot_max = 0i32;
        if unsafe { ffi::tsi_get_hot_pixel_threshold_range(ctx, &mut hot_min, &mut hot_max) } == 0
            && hot_max != 0
        {
            if !self.props.has_property("HotPixelThreshold") {
                self.props
                    .define_property("HotPixelThreshold", PropertyValue::Integer(0), false)
                    .ok();
                self.props
                    .define_property("HotPixel", PropertyValue::String("Off".into()), false)
                    .ok();
                self.props
                    .set_allowed_values("HotPixel", &["Off", "On"])
                    .ok();
            }
            self.props
                .set_property_limits("HotPixelThreshold", hot_min as f64, hot_max as f64)
                .ok();
            let threshold = unsafe { ffi::tsi_get_hot_pixel_threshold(ctx) };
            if threshold >= 0 {
                self.props.entry_mut("HotPixelThreshold").map(|e| {
                    e.value = PropertyValue::Integer(threshold as i64);
                });
            }
            let enabled = unsafe { ffi::tsi_get_hot_pixel_enabled(ctx) } == 1;
            self.props
                .entry_mut("HotPixel")
                .map(|e| e.value = on_off(enabled));
        }

        let mut gain_min = 0i32;
        let mut gain_max = 0i32;
        if unsafe { ffi::tsi_get_gain_range(ctx, &mut gain_min, &mut gain_max) } == 0
            && gain_max > 0
        {
            if !self.props.has_property("Gain") {
                let value = if gain_max == 3 {
                    PropertyValue::String("0".into())
                } else {
                    PropertyValue::Float(0.0)
                };
                self.props.define_property("Gain", value, false).ok();
            }
            let mut gain_db = 0.0f64;
            if unsafe { ffi::tsi_get_gain_db(ctx, &mut gain_db) } == 0 {
                self.props.entry_mut("Gain").map(|e| {
                    e.value = if gain_max == 3 {
                        PropertyValue::String(gain_db_string(gain_db))
                    } else {
                        PropertyValue::Float(gain_db)
                    };
                });
            }
            let mut min_db = 0.0f64;
            let mut max_db = 0.0f64;
            if unsafe { ffi::tsi_convert_gain_to_db(ctx, gain_min, &mut min_db) } == 0
                && unsafe { ffi::tsi_convert_gain_to_db(ctx, gain_max, &mut max_db) } == 0
            {
                if gain_max == 3 {
                    let mut allowed = Vec::new();
                    for gain in gain_min..=gain_max {
                        let mut db = 0.0f64;
                        if unsafe { ffi::tsi_convert_gain_to_db(ctx, gain, &mut db) } != 0 {
                            return self.initialize_error(MmError::LocallyDefined(
                                "TSI: failed to enumerate gain values".into(),
                            ));
                        }
                        allowed.push(gain_db_string(db));
                    }
                    let refs: Vec<&str> = allowed.iter().map(|s| s.as_str()).collect();
                    self.props.set_allowed_values("Gain", &refs).ok();
                } else {
                    self.props.set_property_limits("Gain", min_db, max_db).ok();
                }
            }
        }

        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        let _ = self.stop_sequence_acquisition();
        if !self.ctx.is_null() {
            unsafe { ffi::tsi_close_camera(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
        unsafe { ffi::tsi_sdk_close() };
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "CameraID" => Ok(PropertyValue::String(self.camera_id.clone())),
            "CameraName" => Ok(PropertyValue::String(self.camera_name.clone())),
            "Exposure" => Ok(PropertyValue::Float(self.exposure_ms)),
            "Binning" => Ok(PropertyValue::Integer(self.binning as i64)),
            "TriggerMode" => Ok(PropertyValue::String(self.trigger_mode.clone())),
            "TriggerPolarity" if self.trigger_polarity_property_exposed() => {
                Ok(PropertyValue::String(self.trigger_polarity.clone()))
            }
            "TriggerPolarity" => Err(MmError::UnknownLabel(name.to_string())),
            "PixelType" => Ok(PropertyValue::String(self.pixel_type.clone())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "CameraID" => {
                if !self.ctx.is_null() {
                    return Err(MmError::LocallyDefined(
                        "CameraID cannot be changed after initialize()".into(),
                    ));
                }
                self.camera_id = val.as_str().to_string();
                self.props.set(name, val)
            }
            "Exposure" => {
                if self.capturing {
                    return Err(MmError::CameraBusyAcquiring);
                }
                let exposure_ms =
                    validate_exposure_ms(val.as_f64().ok_or(MmError::InvalidPropertyValue)?)?;
                let previous = self.exposure_ms;
                self.props.set(name, PropertyValue::Float(exposure_ms))?;
                if !self.ctx.is_null() {
                    let us = exposure_ms_to_us(exposure_ms);
                    if unsafe { ffi::tsi_set_exposure_us(self.ctx, us) } != 0 {
                        self.props.set(name, PropertyValue::Float(previous)).ok();
                        return Err(MmError::LocallyDefined(
                            "TSI: failed to set exposure".into(),
                        ));
                    }
                }
                self.exposure_ms = exposure_ms;
                Ok(())
            }
            "Binning" => {
                if self.capturing {
                    return Err(MmError::CameraBusyAcquiring);
                }
                let binning = validate_i32_property_value(&val)?;
                if binning < 1 {
                    return Err(MmError::InvalidPropertyValue);
                }
                let previous = self.binning;
                self.props
                    .set(name, PropertyValue::Integer(binning as i64))?;
                self.binning = binning;
                if let Err(err) = self.apply_binning() {
                    self.binning = previous;
                    self.props
                        .set(name, PropertyValue::Integer(previous as i64))
                        .ok();
                    return Err(err);
                }
                Ok(())
            }
            "TriggerMode" => {
                if self.capturing {
                    return Err(MmError::CameraBusyAcquiring);
                }
                let trigger_mode =
                    canonical_trigger_mode(val.as_str()).ok_or(MmError::InvalidPropertyValue)?;
                let previous = self.trigger_mode.clone();
                self.props
                    .set(name, PropertyValue::String(trigger_mode.into()))?;
                if let Err(err) = self.apply_trigger_mode(&trigger_mode) {
                    self.props
                        .set(name, PropertyValue::String(previous.clone()))
                        .ok();
                    self.trigger_mode = previous;
                    return Err(err);
                }
                if self.ctx.is_null() {
                    self.trigger_mode = trigger_mode.into();
                } else {
                    self.refresh_trigger_mode();
                }
                Ok(())
            }
            "TriggerPolarity" => {
                if !self.trigger_polarity_property_exposed() {
                    return Err(MmError::UnknownLabel(name.to_string()));
                }
                if self.capturing {
                    return Err(MmError::CameraBusyAcquiring);
                }
                let trigger_polarity = val.as_str().to_string();
                let previous = self.trigger_polarity.clone();
                self.props.set(name, val)?;
                if let Err(err) = self.apply_trigger_polarity(&trigger_polarity) {
                    self.props
                        .set(name, PropertyValue::String(previous.clone()))
                        .ok();
                    return Err(err);
                }
                if self.ctx.is_null() {
                    self.trigger_polarity = trigger_polarity;
                } else {
                    self.refresh_trigger_polarity();
                }
                Ok(())
            }
            "PixelType" => {
                if self.capturing {
                    return Err(MmError::CameraBusyAcquiring);
                }
                let pixel_type = val.as_str().to_string();
                let (bytes_per_pixel, bit_depth) =
                    pixel_type_bytes_and_depth(&pixel_type, self.bit_depth)
                        .ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, val)?;
                self.pixel_type = pixel_type;
                self.bytes_per_pixel = bytes_per_pixel;
                self.bit_depth = bit_depth;
                Ok(())
            }
            "EEP" => {
                if self.capturing {
                    return Err(MmError::CameraBusyAcquiring);
                }
                let enabled = match val.as_str() {
                    "On" => true,
                    "Off" => false,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                let previous = self.props.get(name).cloned().ok();
                self.props.set(name, val)?;
                if !self.ctx.is_null()
                    && unsafe { ffi::tsi_set_eep_enabled(self.ctx, enabled as i32) } != 0
                {
                    if let Some(previous) = previous {
                        self.props.set(name, previous).ok();
                    }
                    return Err(MmError::LocallyDefined("TSI: failed to set EEP".into()));
                }
                Ok(())
            }
            "HotPixel" => {
                if self.capturing {
                    return Err(MmError::CameraBusyAcquiring);
                }
                let enabled = match val.as_str() {
                    "On" => true,
                    "Off" => false,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                let previous = self.props.get(name).cloned().ok();
                self.props.set(name, val)?;
                if !self.ctx.is_null()
                    && unsafe { ffi::tsi_set_hot_pixel_enabled(self.ctx, enabled as i32) } != 0
                {
                    if let Some(previous) = previous {
                        self.props.set(name, previous).ok();
                    }
                    return Err(MmError::LocallyDefined(
                        "TSI: failed to set hot-pixel correction".into(),
                    ));
                }
                Ok(())
            }
            "HotPixelThreshold" => {
                if self.capturing {
                    return Err(MmError::CameraBusyAcquiring);
                }
                let threshold = validate_i32_property_value(&val)?;
                let previous = self.props.get(name).cloned().ok();
                self.props
                    .set(name, PropertyValue::Integer(threshold as i64))?;
                if !self.ctx.is_null()
                    && unsafe { ffi::tsi_set_hot_pixel_threshold(self.ctx, threshold) } != 0
                {
                    if let Some(previous) = previous {
                        self.props.set(name, previous).ok();
                    }
                    return Err(MmError::LocallyDefined(
                        "TSI: failed to set hot-pixel threshold".into(),
                    ));
                }
                Ok(())
            }
            "Gain" => {
                if self.capturing {
                    return Err(MmError::CameraBusyAcquiring);
                }
                let gain_db = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                let previous = self.props.get(name).cloned().ok();
                let property_type = self
                    .props
                    .entry(name)
                    .map(|e| e.property_type)
                    .unwrap_or(PropertyType::Float);
                let stored_value = if property_type == PropertyType::String {
                    PropertyValue::String(gain_db_string(gain_db))
                } else {
                    PropertyValue::Float(gain_db)
                };
                self.props.set(name, stored_value)?;
                if !self.ctx.is_null() && unsafe { ffi::tsi_set_gain_db(self.ctx, gain_db) } != 0 {
                    if let Some(previous) = previous {
                        self.props.set(name, previous).ok();
                    }
                    return Err(MmError::LocallyDefined("TSI: failed to set gain".into()));
                }
                if let Err(err) = self.refresh_gain_property() {
                    if let Some(previous) = previous {
                        self.props.set(name, previous).ok();
                    }
                    return Err(err);
                }
                Ok(())
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> {
        self.props
            .property_names()
            .iter()
            .filter(|name| {
                name.as_str() != "TriggerPolarity" || self.trigger_polarity_property_exposed()
            })
            .cloned()
            .collect()
    }
    fn has_property(&self, name: &str) -> bool {
        if name == "TriggerPolarity" && !self.trigger_polarity_property_exposed() {
            return false;
        }
        self.props.has_property(name)
    }
    fn is_property_read_only(&self, name: &str) -> bool {
        if name == "TriggerPolarity" && !self.trigger_polarity_property_exposed() {
            return false;
        }
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Camera
    }
    fn busy(&self) -> bool {
        self.capturing
    }
}

// ── Camera trait ──────────────────────────────────────────────────────────────

impl Camera for TSICamera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_open()?;

        if self.capturing {
            // Sequence mode: wait for the next frame from the continuous stream.
            let timeout = self.snap_timeout_ms();
            let rc = unsafe { ffi::tsi_get_next_frame(self.ctx, timeout) };
            if rc != 0 {
                return Err(MmError::SnapImageFailed);
            }
            self.copy_frame_from_shim()?;
            return Ok(());
        }

        // Single-frame snap.
        let timeout = self.snap_timeout_ms();
        let rc = unsafe { ffi::tsi_snap(self.ctx, timeout) };
        if rc != 0 {
            return Err(MmError::SnapImageFailed);
        }

        self.img_width = unsafe { ffi::tsi_get_image_width(self.ctx) } as u32;
        self.img_height = unsafe { ffi::tsi_get_image_height(self.ctx) } as u32;
        self.props
            .entry_mut("Width")
            .map(|e| e.value = PropertyValue::Integer(self.img_width as i64));
        self.props
            .entry_mut("Height")
            .map(|e| e.value = PropertyValue::Integer(self.img_height as i64));
        self.copy_frame_from_shim()?;
        Ok(())
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if self.img_buf.is_empty() {
            Err(MmError::LocallyDefined("No image captured yet".into()))
        } else {
            Ok(&self.img_buf)
        }
    }

    fn get_image_width(&self) -> u32 {
        self.img_width
    }
    fn get_image_height(&self) -> u32 {
        self.img_height
    }
    fn get_image_bytes_per_pixel(&self) -> u32 {
        self.bytes_per_pixel
    }
    fn get_bit_depth(&self) -> u32 {
        self.bit_depth
    }
    fn get_number_of_components(&self) -> u32 {
        match self.pixel_type.as_str() {
            "RGBA32" | "RGBA64" => 4,
            _ => 1,
        }
    }
    fn get_number_of_channels(&self) -> u32 {
        1
    }
    fn get_exposure(&self) -> f64 {
        self.exposure_ms
    }

    fn set_exposure(&mut self, exp_ms: f64) -> MmResult<()> {
        <Self as Device>::set_property(self, "Exposure", PropertyValue::Float(exp_ms))
    }

    fn get_binning(&self) -> i32 {
        self.binning
    }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        if bin < 1 {
            return Err(MmError::InvalidPropertyValue);
        }
        self.props
            .set("Binning", PropertyValue::Integer(bin as i64))?;
        let previous = self.binning;
        self.binning = bin;
        if let Err(err) = self.apply_binning() {
            self.binning = previous;
            self.props
                .set("Binning", PropertyValue::Integer(previous as i64))
                .ok();
            return Err(err);
        }
        Ok(())
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        if self.ctx.is_null() {
            return Ok(ImageRoi::new(0, 0, self.img_width, self.img_height));
        }
        let (mut x, mut y, mut w, mut h) = (0i32, 0i32, 0i32, 0i32);
        if unsafe { ffi::tsi_get_roi(self.ctx, &mut x, &mut y, &mut w, &mut h) } != 0 {
            return Err(MmError::Err);
        }
        let bin = self.binning.max(1);
        Ok(ImageRoi::new(
            (x / bin) as u32,
            (y / bin) as u32,
            (w / bin) as u32,
            (h / bin) as u32,
        ))
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        self.check_open()?;
        let rc = unsafe {
            let bin = self.binning.max(1);
            ffi::tsi_set_roi(
                self.ctx,
                roi.x as i32 * bin,
                roi.y as i32 * bin,
                roi.width as i32 * bin,
                roi.height as i32 * bin,
            )
        };
        if rc != 0 {
            return Err(MmError::Err);
        }
        self.sync_dims();
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        self.check_open()?;
        if unsafe { ffi::tsi_clear_roi(self.ctx) } != 0 {
            return Err(MmError::Err);
        }
        self.sync_dims();
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, count: i64, _interval_ms: f64) -> MmResult<()> {
        self.check_open()?;
        if count < 0 {
            return Err(MmError::InvalidPropertyValue);
        }
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }

        let frame_count = count.clamp(0, i32::MAX as i64) as i32;
        let rc = unsafe { ffi::tsi_start_cont(self.ctx, frame_count) };
        if rc != 0 {
            return Err(MmError::LocallyDefined(
                "TSI: failed to start continuous acquisition".into(),
            ));
        }
        self.capturing = true;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        if !self.capturing {
            return Ok(());
        }
        if !self.ctx.is_null() {
            unsafe { ffi::tsi_stop_cont(self.ctx) };
        }
        self.capturing = false;
        Ok(())
    }

    fn is_capturing(&self) -> bool {
        self.capturing
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static TSI_STUB_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn tsi_stub_enabled() -> bool {
        std::env::var_os("TSI_STUB").is_some()
    }

    fn tsi_stub_test_lock() -> MutexGuard<'static, ()> {
        TSI_STUB_TEST_LOCK.lock().unwrap()
    }

    struct EnvVarGuard(&'static str);

    impl EnvVarGuard {
        fn set(name: &'static str, value: &str) -> Self {
            std::env::set_var(name, value);
            Self(name)
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            std::env::remove_var(self.0);
        }
    }

    #[test]
    fn default_properties() {
        let d = TSICamera::new();
        assert_eq!(d.device_type(), DeviceType::Camera);
        assert_eq!(
            d.get_property("CameraID").unwrap(),
            PropertyValue::String("0".into())
        );
        assert_eq!(d.get_exposure(), 2.0);
        assert_eq!(d.get_binning(), 1);
        assert!(!d.is_capturing());
        assert_eq!(d.get_number_of_components(), 1);
    }

    #[test]
    fn set_camera_id_pre_init() {
        let mut d = TSICamera::new();
        d.set_property("CameraID", PropertyValue::String("CS2100M-USB".into()))
            .unwrap();
        assert_eq!(d.camera_id, "CS2100M-USB");
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = TSICamera::new();
        d.set_property("Exposure", PropertyValue::Float(50.0))
            .unwrap();
        assert_eq!(d.exposure_ms, 50.0);
        assert_eq!(d.get_exposure(), 50.0);
    }

    #[test]
    fn set_binning_pre_init() {
        let mut d = TSICamera::new();
        d.set_property("Binning", PropertyValue::Integer(2))
            .unwrap();
        assert_eq!(d.binning, 2);
        assert_eq!(d.get_binning(), 2);
    }

    #[test]
    fn upstream_trigger_and_pixel_properties_are_present() {
        let d = TSICamera::new();
        assert_eq!(
            d.get_property("TriggerMode").unwrap(),
            PropertyValue::String("Software".into())
        );
        assert_eq!(
            d.get_property("TriggerPolarity").unwrap(),
            PropertyValue::String("Positive".into())
        );
        assert_eq!(
            d.get_property("PixelType").unwrap(),
            PropertyValue::String("Mono16".into())
        );
    }

    #[test]
    fn trigger_mode_uses_upstream_names_with_compatibility_aliases() {
        let mut d = TSICamera::new();
        d.set_property(
            "TriggerMode",
            PropertyValue::String("HardwareStandard".into()),
        )
        .unwrap();
        assert_eq!(
            d.get_property("TriggerMode").unwrap(),
            PropertyValue::String("HardwareEdge".into())
        );

        d.set_property("TriggerMode", PropertyValue::String("HardwareBulb".into()))
            .unwrap();
        assert_eq!(
            d.get_property("TriggerMode").unwrap(),
            PropertyValue::String("HardwareDuration".into())
        );
    }

    #[test]
    fn upstream_color_pixel_type_uses_rgba_names_and_components() {
        let mut d = TSICamera::new();
        d.props
            .set_allowed_values("PixelType", &["RGBA32"])
            .unwrap();

        d.set_property("PixelType", PropertyValue::String("RGBA32".into()))
            .unwrap();

        assert_eq!(
            d.get_property("PixelType").unwrap(),
            PropertyValue::String("RGBA32".into())
        );
        assert_eq!(d.get_image_bytes_per_pixel(), 4);
        assert_eq!(d.get_bit_depth(), 8);
        assert_eq!(d.get_number_of_components(), 4);
    }

    #[test]
    fn invalid_allowed_values_do_not_mutate_cached_state() {
        let mut d = TSICamera::new();
        assert!(d
            .set_property("Binning", PropertyValue::Integer(0))
            .is_err());
        assert_eq!(d.get_binning(), 1);
        assert!(d
            .set_property("TriggerMode", PropertyValue::String("External".into()))
            .is_err());
        assert_eq!(
            d.get_property("TriggerMode").unwrap(),
            PropertyValue::String("Software".into())
        );
        assert!(d
            .set_property("TriggerPolarity", PropertyValue::String("Either".into()))
            .is_err());
        assert_eq!(
            d.get_property("TriggerPolarity").unwrap(),
            PropertyValue::String("Positive".into())
        );
        assert!(d
            .set_property("PixelType", PropertyValue::String("Mono8".into()))
            .is_err());
        assert_eq!(
            d.get_property("PixelType").unwrap(),
            PropertyValue::String("Mono16".into())
        );
    }

    #[test]
    fn snap_without_init_errors() {
        let mut d = TSICamera::new();
        assert!(d.snap_image().is_err());
    }

    #[test]
    fn no_image_before_snap() {
        let d = TSICamera::new();
        assert!(d.get_image_buffer().is_err());
    }

    #[test]
    fn initialize_missing_camera_fails() {
        let mut d = TSICamera::new();
        d.set_property("CameraID", PropertyValue::String("missing".into()))
            .unwrap();
        assert!(d.initialize().is_err());
    }

    #[test]
    fn stub_missing_camera_initialize_closes_sdk() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut missing = TSICamera::new();
        missing
            .set_property("CameraID", PropertyValue::String("missing".into()))
            .unwrap();
        assert!(missing.initialize().is_err());

        let mut found = TSICamera::new();
        found.initialize().unwrap();
        found.shutdown().unwrap();
    }

    #[test]
    fn stub_dll_initialize_failure_prevents_sdk_open_and_recovers() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let env = EnvVarGuard::set("TSI_STUB_FAIL_DLL_INITIALIZE", "1");
        let mut failed = TSICamera::new();
        assert_eq!(
            failed.initialize().unwrap_err(),
            MmError::LocallyDefined("tsi_sdk_open failed".into())
        );
        assert!(failed.ctx.is_null());

        drop(env);
        let mut recovered = TSICamera::new();
        recovered.initialize().unwrap();
        recovered.shutdown().unwrap();
    }

    #[test]
    fn stub_failed_initialize_closes_partial_camera() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut d = TSICamera::new();
        d.set_property("Binning", PropertyValue::Integer(5))
            .unwrap();

        assert!(d.initialize().is_err());
        assert!(d.ctx.is_null());
        assert_eq!(d.snap_image().unwrap_err(), MmError::NotConnected);

        d.set_property("Binning", PropertyValue::Integer(1))
            .unwrap();
        d.initialize().unwrap();
        assert!(!d.ctx.is_null());

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_initializes_and_exercises_sdk_backed_properties() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut d = TSICamera::new();
        d.initialize().unwrap();

        assert_eq!(
            d.get_property("CameraName").unwrap(),
            PropertyValue::String("0".into())
        );
        assert_eq!(
            d.get_property("SerialNumber").unwrap(),
            PropertyValue::String("TSI-STUB".into())
        );
        assert_eq!(
            d.get_property("FirmwareVersion").unwrap(),
            PropertyValue::String("0.0-stub".into())
        );
        assert!(d.has_property("EEP"));
        assert!(d.has_property("HotPixel"));
        assert!(d.has_property("HotPixelThreshold"));
        assert!(d.has_property("Gain"));

        d.set_property("TriggerMode", PropertyValue::String("HardwareEdge".into()))
            .unwrap();
        assert_eq!(
            d.get_property("TriggerMode").unwrap(),
            PropertyValue::String("HardwareEdge".into())
        );
        d.set_property("TriggerMode", PropertyValue::String("Software".into()))
            .unwrap();
        d.set_property("EEP", PropertyValue::String("On".into()))
            .unwrap();
        d.set_property("HotPixel", PropertyValue::String("On".into()))
            .unwrap();
        d.set_property("HotPixelThreshold", PropertyValue::Integer(12))
            .unwrap();
        d.set_property("Gain", PropertyValue::Float(3.0)).unwrap();

        d.set_binning(2).unwrap();
        d.set_roi(ImageRoi::new(2, 3, 10, 5)).unwrap();
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(2, 3, 10, 5));
        assert_eq!(d.get_image_width(), 10);
        assert_eq!(d.get_image_height(), 5);

        d.snap_image().unwrap();
        assert_eq!(d.get_image_buffer().unwrap().len(), 10 * 5 * 2);
    }

    #[test]
    fn stub_allows_multiple_camera_instances_with_shared_sdk() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut first = TSICamera::new();
        first.initialize().unwrap();

        let mut second = TSICamera::new();
        second.initialize().unwrap();

        second.shutdown().unwrap();
        first.shutdown().unwrap();
    }

    #[test]
    fn stub_cs135_gain_is_discrete_string_property() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();
        let _env = EnvVarGuard::set("TSI_STUB_CS135", "1");

        let mut d = TSICamera::new();
        d.initialize().unwrap();

        let gain = d.props.entry("Gain").unwrap();
        assert_eq!(gain.property_type, PropertyType::String);
        assert_eq!(gain.allowed_values, vec!["0", "1", "2", "3"]);

        d.set_property("Gain", PropertyValue::String("2".into()))
            .unwrap();
        assert_eq!(
            d.get_property("Gain").unwrap(),
            PropertyValue::String("2".into())
        );
        assert_eq!(
            d.set_property("Gain", PropertyValue::String("2.5".into()))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_gain_set_refreshes_sdk_reported_value() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut d = TSICamera::new();
        d.initialize().unwrap();

        d.set_property("Gain", PropertyValue::Float(2.5)).unwrap();
        assert_eq!(d.get_property("Gain").unwrap(), PropertyValue::Float(2.0));

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_bayer_sensor_disables_binning_like_upstream() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();
        let _env = EnvVarGuard::set("TSI_STUB_SENSOR_TYPE", "BAYER");

        let mut d = TSICamera::new();
        d.set_property("Binning", PropertyValue::Integer(2))
            .unwrap();
        d.initialize().unwrap();

        assert_eq!(d.get_binning(), 1);
        assert_eq!(d.get_image_bytes_per_pixel(), 4);
        assert_eq!(d.get_number_of_components(), 4);
        assert_eq!(d.get_bit_depth(), 8);
        assert_eq!(
            d.get_property("BitDepth").unwrap(),
            PropertyValue::Integer(8)
        );
        assert_eq!(
            d.get_property("PixelType").unwrap(),
            PropertyValue::String("RGBA32".into())
        );
        assert_eq!(d.props.entry("Binning").unwrap().allowed_values, vec!["1"]);
        assert_eq!(
            d.set_property("Binning", PropertyValue::Integer(2))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_bayer_snap_returns_rgba_sized_buffer() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();
        let _env = EnvVarGuard::set("TSI_STUB_SENSOR_TYPE", "BAYER");

        let mut d = TSICamera::new();
        d.initialize().unwrap();

        d.snap_image().unwrap();
        assert_eq!(d.get_image_buffer().unwrap().len(), 64 * 48 * 4);
        assert_eq!(d.get_image_bytes_per_pixel(), 4);
        assert_eq!(d.get_number_of_components(), 4);

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_polarized_sensor_uses_mono_metadata_and_keeps_upstream_binning() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();
        let _env = EnvVarGuard::set("TSI_STUB_SENSOR_TYPE", "POLARIZED");

        let mut d = TSICamera::new();
        d.set_property("Binning", PropertyValue::Integer(2))
            .unwrap();
        d.initialize().unwrap();

        assert_eq!(
            d.get_property("SensorType").unwrap(),
            PropertyValue::String("Polarized".into())
        );
        assert_eq!(d.get_binning(), 2);
        assert_eq!(d.get_image_bytes_per_pixel(), 2);
        assert_eq!(d.get_bit_depth(), 16);
        assert_eq!(
            d.get_property("BitDepth").unwrap(),
            PropertyValue::Integer(16)
        );
        assert_eq!(d.get_number_of_components(), 1);
        assert_eq!(
            d.get_property("PixelType").unwrap(),
            PropertyValue::String("Mono16".into())
        );
        assert_eq!(
            d.props.entry("Binning").unwrap().allowed_values,
            vec!["1", "2", "3", "4"]
        );
        assert_eq!(d.get_image_width(), 32);
        assert_eq!(d.get_image_height(), 24);
        d.set_property("Binning", PropertyValue::Integer(4))
            .unwrap();
        assert_eq!(d.get_binning(), 4);
        assert_eq!(d.get_image_width(), 16);
        assert_eq!(d.get_image_height(), 12);
        d.snap_image().unwrap();
        assert_eq!(d.get_image_buffer().unwrap().len(), 16 * 12 * 2);

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_software_only_camera_ignores_preinit_hardware_trigger() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();
        let _env = EnvVarGuard::set("TSI_STUB_NO_HARDWARE_TRIGGER", "1");

        let mut d = TSICamera::new();
        d.set_property("TriggerMode", PropertyValue::String("HardwareEdge".into()))
            .unwrap();

        d.initialize().unwrap();

        assert_eq!(
            d.get_property("TriggerMode").unwrap(),
            PropertyValue::String("Software".into())
        );
        assert_eq!(
            d.props.entry("TriggerMode").unwrap().allowed_values,
            vec!["Software"]
        );
        assert!(!d.has_property("TriggerPolarity"));
        assert!(!d
            .property_names()
            .iter()
            .any(|name| name == "TriggerPolarity"));
        assert_eq!(
            d.get_property("TriggerPolarity").unwrap_err(),
            MmError::UnknownLabel("TriggerPolarity".into())
        );
        assert_eq!(
            d.set_property("TriggerPolarity", PropertyValue::String("Negative".into()))
                .unwrap_err(),
            MmError::UnknownLabel("TriggerPolarity".into())
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_trigger_mode_set_refreshes_sdk_reported_value() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();
        let _env = EnvVarGuard::set("TSI_STUB_COERCE_HARDWARE_TRIGGER", "1");

        let mut d = TSICamera::new();
        d.initialize().unwrap();

        d.set_property("TriggerMode", PropertyValue::String("HardwareEdge".into()))
            .unwrap();
        assert_eq!(
            d.get_property("TriggerMode").unwrap(),
            PropertyValue::String("Software".into())
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_trigger_polarity_set_refreshes_sdk_reported_value() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();
        let _env = EnvVarGuard::set("TSI_STUB_COERCE_TRIGGER_POLARITY", "1");

        let mut d = TSICamera::new();
        d.initialize().unwrap();

        d.set_property("TriggerPolarity", PropertyValue::String("Negative".into()))
            .unwrap();
        assert_eq!(
            d.get_property("TriggerPolarity").unwrap(),
            PropertyValue::String("Positive".into())
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_rejects_roi_outside_sensor_without_resizing() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut d = TSICamera::new();
        d.initialize().unwrap();
        let original = ImageRoi::new(0, 0, 64, 48);
        assert_eq!(d.get_roi().unwrap(), original);

        assert!(d.set_roi(ImageRoi::new(60, 40, 16, 16)).is_err());

        assert_eq!(d.get_roi().unwrap(), original);
        assert_eq!(d.get_image_width(), 64);
        assert_eq!(d.get_image_height(), 48);
    }

    #[test]
    fn stub_failed_vertical_binning_rolls_back_horizontal_binning() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut d = TSICamera::new();
        d.initialize().unwrap();
        let env = EnvVarGuard::set("TSI_STUB_FAIL_BINY", "1");

        assert_eq!(d.get_image_width(), 64);
        assert_eq!(d.get_image_height(), 48);
        assert_eq!(
            d.set_binning(2).unwrap_err(),
            MmError::LocallyDefined("TSI: failed to set vertical binning".into())
        );
        assert_eq!(d.get_binning(), 1);
        assert_eq!(
            d.get_property("Binning").unwrap(),
            PropertyValue::Integer(1)
        );
        assert_eq!(d.get_image_width(), 64);
        assert_eq!(d.get_image_height(), 48);

        drop(env);
        d.set_binning(2).unwrap();
        assert_eq!(d.get_image_width(), 32);
        assert_eq!(d.get_image_height(), 24);

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_binning_rejects_wrapping_integer_without_mutation() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut d = TSICamera::new();
        d.initialize().unwrap();

        assert_eq!(
            d.set_property("Binning", PropertyValue::Integer(4_294_967_297))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(d.get_binning(), 1);
        assert_eq!(
            d.get_property("Binning").unwrap(),
            PropertyValue::Integer(1)
        );
        assert_eq!(d.get_image_width(), 64);
        assert_eq!(d.get_image_height(), 48);

        d.shutdown().unwrap();
    }

    #[test]
    fn readonly_properties() {
        let d = TSICamera::new();
        assert!(d.is_property_read_only("Width"));
        assert!(d.is_property_read_only("Height"));
        assert!(d.is_property_read_only("BitDepth"));
        assert!(d.is_property_read_only("SensorType"));
        assert!(d.is_property_read_only("CameraName"));
        assert!(d.is_property_read_only("SerialNumber"));
        assert!(d.is_property_read_only("FirmwareVer"));
        assert!(!d.is_property_read_only("Exposure"));
        assert!(!d.is_property_read_only("Binning"));
    }

    #[test]
    fn exposure_ms_to_us_conversion() {
        // Verify the conversion factor (tested without SDK).
        let exp_ms = 15.5_f64;
        let exp_us = exposure_ms_to_us(exp_ms);
        assert_eq!(exp_us, 15_500);
    }

    #[test]
    fn invalid_exposure_values_do_not_mutate_cached_state() {
        let mut d = TSICamera::new();

        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                d.set_property("Exposure", PropertyValue::Float(bad))
                    .unwrap_err(),
                MmError::InvalidPropertyValue
            );
            assert_eq!(d.get_exposure(), 2.0);
            assert_eq!(
                d.get_property("Exposure").unwrap(),
                PropertyValue::Float(2.0)
            );
        }

        d.set_exposure(-5.0);
        assert_eq!(d.get_exposure(), 2.0);
        d.set_exposure(f64::NAN);
        assert_eq!(d.get_exposure(), 2.0);
    }

    #[test]
    fn stub_sdk_rejected_exposure_does_not_mutate_cached_state() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut d = TSICamera::new();
        d.initialize().unwrap();

        assert_eq!(
            d.set_property("Exposure", PropertyValue::Float(1_000.001))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(d.get_exposure(), 2.0);

        d.set_exposure(1_000.001);
        assert_eq!(d.get_exposure(), 2.0);
        assert_eq!(
            d.get_property("Exposure").unwrap(),
            PropertyValue::Float(2.0)
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_trait_set_exposure_enforces_sdk_reported_limits() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut d = TSICamera::new();
        d.initialize().unwrap();

        d.set_exposure(0.0005);
        assert_eq!(d.get_exposure(), 2.0);
        assert_eq!(
            d.get_property("Exposure").unwrap(),
            PropertyValue::Float(2.0)
        );

        d.set_exposure(0.001);
        assert_eq!(d.get_exposure(), 0.001);
        assert_eq!(
            d.get_property("Exposure").unwrap(),
            PropertyValue::Float(0.001)
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn acquisition_rejects_setting_changes_before_state_mutation() {
        let mut d = TSICamera::new();
        d.capturing = true;

        assert!(d.busy());
        assert!(d.is_capturing());

        assert_eq!(
            d.set_property("Exposure", PropertyValue::Float(50.0))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(d.get_exposure(), 2.0);
        assert_eq!(
            d.get_property("Exposure").unwrap(),
            PropertyValue::Float(2.0)
        );

        assert_eq!(d.set_binning(2).unwrap_err(), MmError::CameraBusyAcquiring);
        assert_eq!(d.get_binning(), 1);
        assert_eq!(
            d.set_property("Binning", PropertyValue::Integer(4))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(d.get_binning(), 1);
        assert_eq!(
            d.set_property("TriggerMode", PropertyValue::String("HardwareEdge".into()))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(
            d.get_property("TriggerMode").unwrap(),
            PropertyValue::String("Software".into())
        );
    }

    #[test]
    fn acquisition_rejects_roi_changes() {
        let mut d = TSICamera::new();
        d.capturing = true;

        assert_eq!(
            d.set_roi(ImageRoi::new(0, 0, 64, 64)).unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(d.clear_roi().unwrap_err(), MmError::CameraBusyAcquiring);
    }

    #[test]
    fn stub_sequence_duplicate_start_is_busy() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut d = TSICamera::new();
        d.initialize().unwrap();
        d.start_sequence_acquisition(3, 0.0).unwrap();

        assert_eq!(
            d.start_sequence_acquisition(1, 0.0).unwrap_err(),
            MmError::CameraBusyAcquiring
        );

        d.stop_sequence_acquisition().unwrap();
    }

    #[test]
    fn stub_sequence_rejects_correction_and_gain_changes() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut d = TSICamera::new();
        d.initialize().unwrap();
        d.start_sequence_acquisition(3, 0.0).unwrap();

        assert_eq!(
            d.set_property("EEP", PropertyValue::String("On".into()))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(
            d.get_property("EEP").unwrap(),
            PropertyValue::String("Off".into())
        );

        assert_eq!(
            d.set_property("HotPixel", PropertyValue::String("On".into()))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(
            d.get_property("HotPixel").unwrap(),
            PropertyValue::String("Off".into())
        );

        assert_eq!(
            d.set_property("HotPixelThreshold", PropertyValue::Integer(12))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(
            d.get_property("HotPixelThreshold").unwrap(),
            PropertyValue::Integer(0)
        );

        assert_eq!(
            d.set_property("Gain", PropertyValue::Float(3.0))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(d.get_property("Gain").unwrap(), PropertyValue::Float(0.0));

        d.stop_sequence_acquisition().unwrap();
        d.shutdown().unwrap();
    }

    #[test]
    fn stub_hot_pixel_threshold_rejects_non_finite_float_without_mutation() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut d = TSICamera::new();
        d.initialize().unwrap();

        assert_eq!(
            d.set_property("HotPixelThreshold", PropertyValue::Float(f64::NAN))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            d.get_property("HotPixelThreshold").unwrap(),
            PropertyValue::Integer(0)
        );

        d.set_property("HotPixelThreshold", PropertyValue::Integer(12))
            .unwrap();
        assert_eq!(
            d.set_property("HotPixelThreshold", PropertyValue::Float(12.5))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            d.get_property("HotPixelThreshold").unwrap(),
            PropertyValue::Integer(12)
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_negative_sequence_count_is_rejected_before_starting() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut d = TSICamera::new();
        d.initialize().unwrap();

        assert_eq!(
            d.start_sequence_acquisition(-1, 0.0).unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert!(!d.is_capturing());

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_sequence_snap_reads_continuous_frame() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut d = TSICamera::new();
        d.initialize().unwrap();
        d.start_sequence_acquisition(3, 0.0).unwrap();

        d.snap_image().unwrap();
        assert_eq!(d.get_image_buffer().unwrap().len(), 64 * 48 * 2);
        assert!(d.is_capturing());

        d.stop_sequence_acquisition().unwrap();
        assert!(!d.is_capturing());
    }

    #[test]
    fn stub_sequence_uses_upstream_arm_buffer_count() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();
        let _env = EnvVarGuard::set("TSI_STUB_REQUIRE_UPSTREAM_ARM_BUFFER", "1");

        let mut d = TSICamera::new();
        d.initialize().unwrap();
        d.start_sequence_acquisition(1, 0.0).unwrap();
        d.stop_sequence_acquisition().unwrap();
        d.shutdown().unwrap();
    }

    #[test]
    fn stub_sequence_snap_can_read_multiple_finite_trigger_frames() {
        if !tsi_stub_enabled() {
            return;
        }
        let _guard = tsi_stub_test_lock();

        let mut d = TSICamera::new();
        d.initialize().unwrap();
        d.start_sequence_acquisition(3, 0.0).unwrap();

        d.snap_image().unwrap();
        assert_eq!(d.get_image_buffer().unwrap().len(), 64 * 48 * 2);
        d.snap_image().unwrap();
        assert_eq!(d.get_image_buffer().unwrap().len(), 64 * 48 * 2);

        d.stop_sequence_acquisition().unwrap();
        d.shutdown().unwrap();
    }
}
