use std::ffi::{CStr, CString};

use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Camera, Device};
use mm_device::types::{DeviceType, ImageRoi, PropertyValue};

use crate::ffi;

// SAFETY: Andor3Camera holds a raw pointer to Andor3Ctx.  The SDK is not
// internally thread-safe per handle; `&mut self` enforces single-thread access.
unsafe impl Send for Andor3Camera {}

const BUF: usize = 256;
const VALS_BUF: usize = 2048;

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

fn read_str<F: FnOnce(*mut i8, i32) -> i32>(f: F) -> Option<String> {
    let mut buf = [0i8; BUF];
    if f(buf.as_mut_ptr(), BUF as i32) != 0 { return None; }
    Some(unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned())
}

fn read_enum(ctx: *mut ffi::Andor3Ctx, feature: &str) -> Option<String> {
    let feat = cstr(feature);
    read_str(|b, l| unsafe { ffi::andor3_get_enum(ctx, feat.as_ptr(), b, l) })
}

fn enum_values(ctx: *mut ffi::Andor3Ctx, feature: &str) -> Vec<String> {
    let feat = cstr(feature);
    let mut buf = vec![0i8; VALS_BUF];
    let rc = unsafe { ffi::andor3_enum_values(ctx, feat.as_ptr(), buf.as_mut_ptr(), VALS_BUF as i32) };
    if rc <= 0 { return vec![]; }
    let s = unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned();
    s.split('\n').map(|v| v.to_string()).filter(|v| !v.is_empty()).collect()
}

// ── Camera struct ──────────────────────────────────────────────────────────────

pub struct Andor3Camera {
    props:        PropertyMap,
    ctx:          *mut ffi::Andor3Ctx,

    // Pre-init
    camera_index: i32,
    exposure_ms:  f64,
    pixel_enc:    String,  // "Mono16" default
    binning:      String,  // "1x1" default
    trigger_mode: String,  // "Internal" default

    // Post-init (refreshed after snap / ROI changes)
    img_width:       u32,
    img_height:      u32,
    bytes_per_pixel: u32,
    bit_depth:       u32,

    capturing: bool,
}

impl Andor3Camera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("CameraIndex",  PropertyValue::Integer(0),                 false).unwrap();
        props.define_property("Exposure",     PropertyValue::Float(10.0),                false).unwrap();
        props.define_property("PixelEncoding",PropertyValue::String("Mono16".into()),    false).unwrap();
        props.define_property("Binning",      PropertyValue::String("1x1".into()),       false).unwrap();
        props.define_property("TriggerMode",  PropertyValue::String("Internal".into()),  false).unwrap();
        props.define_property("Width",        PropertyValue::Integer(0),                 true).unwrap();
        props.define_property("Height",       PropertyValue::Integer(0),                 true).unwrap();
        props.define_property("BitDepth",     PropertyValue::Integer(16),                true).unwrap();
        props.define_property("SensorWidth",  PropertyValue::Integer(0),                 true).unwrap();
        props.define_property("SensorHeight", PropertyValue::Integer(0),                 true).unwrap();
        props.define_property("Temperature",  PropertyValue::Float(0.0),                 true).unwrap();
        props.define_property("SerialNumber", PropertyValue::String("".into()),          true).unwrap();
        props.define_property("CameraModel",  PropertyValue::String("".into()),          true).unwrap();
        props.define_property("FirmwareVersion", PropertyValue::String("".into()),       true).unwrap();

        Self {
            props,
            ctx: std::ptr::null_mut(),
            camera_index:    0,
            exposure_ms:     10.0,
            pixel_enc:       "Mono16".into(),
            binning:         "1x1".into(),
            trigger_mode:    "Internal".into(),
            img_width:       0,
            img_height:      0,
            bytes_per_pixel: 2,
            bit_depth:       16,
            capturing:       false,
        }
    }

    fn check_open(&self) -> MmResult<()> {
        if self.ctx.is_null() { Err(MmError::NotConnected) } else { Ok(()) }
    }

    fn sync_dims(&mut self) {
        if self.ctx.is_null() { return; }
        self.img_width       = unsafe { ffi::andor3_get_image_width(self.ctx)       } as u32;
        self.img_height      = unsafe { ffi::andor3_get_image_height(self.ctx)      } as u32;
        self.bytes_per_pixel = unsafe { ffi::andor3_get_bytes_per_pixel(self.ctx)   } as u32;
        self.bit_depth       = unsafe { ffi::andor3_get_bit_depth(self.ctx)         } as u32;
        self.props.entry_mut("Width")   .map(|e| e.value = PropertyValue::Integer(self.img_width    as i64));
        self.props.entry_mut("Height")  .map(|e| e.value = PropertyValue::Integer(self.img_height   as i64));
        self.props.entry_mut("BitDepth").map(|e| e.value = PropertyValue::Integer(self.bit_depth    as i64));
    }

    fn snap_timeout_ms(&self) -> i32 {
        (self.exposure_ms as i32 + 5_000).max(5_000)
    }
}

impl Default for Andor3Camera {
    fn default() -> Self { Self::new() }
}

impl Drop for Andor3Camera {
    fn drop(&mut self) {
        let _ = self.stop_sequence_acquisition();
        if !self.ctx.is_null() {
            unsafe { ffi::andor3_close(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
        unsafe { ffi::andor3_sdk_close() };
    }
}

// ── Device trait ───────────────────────────────────────────────────────────────

impl Device for Andor3Camera {
    fn name(&self) -> &str { "Andor3Camera" }
    fn description(&self) -> &str { "Andor camera via SDK3 (atcore)" }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.ctx.is_null() { return Ok(()); }

        if unsafe { ffi::andor3_sdk_open() } != 0 {
            return Err(MmError::LocallyDefined("Andor SDK3: library initialisation failed".into()));
        }

        let count = unsafe { ffi::andor3_get_device_count() };
        if count <= 0 {
            return Err(MmError::LocallyDefined("Andor SDK3: no cameras found".into()));
        }
        if self.camera_index >= count {
            return Err(MmError::LocallyDefined(format!(
                "Andor SDK3: camera index {} out of range (found {})",
                self.camera_index, count
            )));
        }

        let ctx = unsafe { ffi::andor3_open(self.camera_index) };
        if ctx.is_null() {
            return Err(MmError::LocallyDefined(
                format!("Andor SDK3: failed to open camera {}", self.camera_index),
            ));
        }
        self.ctx = ctx;

        // Read static properties.
        let sw = unsafe { ffi::andor3_get_sensor_width(ctx)  } as i64;
        let sh = unsafe { ffi::andor3_get_sensor_height(ctx) } as i64;
        self.props.entry_mut("SensorWidth") .map(|e| e.value = PropertyValue::Integer(sw));
        self.props.entry_mut("SensorHeight").map(|e| e.value = PropertyValue::Integer(sh));

        for (feat, prop) in &[
            ("SerialNumber",     "SerialNumber"),
            ("CameraModel",      "CameraModel"),
            ("FirmwareVersion",  "FirmwareVersion"),
        ] {
            if let Some(s) = read_str(|b, l| {
                let f = cstr(feat);
                unsafe { ffi::andor3_get_string(ctx, f.as_ptr(), b, l) }
            }) {
                self.props.entry_mut(prop).map(|e| e.value = PropertyValue::String(s));
            }
        }

        // Populate allowed values for enum properties.
        for feat in &["PixelEncoding", "Binning", "TriggerMode"] {
            let vals = enum_values(ctx, feat);
            if !vals.is_empty() {
                let refs: Vec<&str> = vals.iter().map(|s| s.as_str()).collect();
                self.props.set_allowed_values(feat, &refs).ok();
            }
        }

        // Apply pre-init settings.
        unsafe { ffi::andor3_set_exposure_s(ctx, self.exposure_ms / 1_000.0) };
        let pe   = cstr("PixelEncoding");
        let pev  = cstr(&self.pixel_enc);
        unsafe { ffi::andor3_set_enum(ctx, pe.as_ptr(), pev.as_ptr()) };

        let bf  = cstr("AOIBinning");
        let bv  = cstr(&self.binning);
        unsafe { ffi::andor3_set_enum(ctx, bf.as_ptr(), bv.as_ptr()) };

        let tf  = cstr("TriggerMode");
        let tv  = cstr(&self.trigger_mode);
        unsafe { ffi::andor3_set_enum(ctx, tf.as_ptr(), tv.as_ptr()) };

        self.sync_dims();
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        let _ = self.stop_sequence_acquisition();
        if !self.ctx.is_null() {
            unsafe { ffi::andor3_close(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
        unsafe { ffi::andor3_sdk_close() };
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "CameraIndex"  => Ok(PropertyValue::Integer(self.camera_index as i64)),
            "Exposure"     => Ok(PropertyValue::Float(self.exposure_ms)),
            "PixelEncoding"=> Ok(PropertyValue::String(self.pixel_enc.clone())),
            "Binning"      => Ok(PropertyValue::String(self.binning.clone())),
            "TriggerMode"  => Ok(PropertyValue::String(self.trigger_mode.clone())),
            "Temperature"  => {
                let t = if self.ctx.is_null() { 0.0 }
                        else { unsafe { ffi::andor3_get_temperature(self.ctx) } };
                Ok(PropertyValue::Float(t))
            }
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
                self.camera_index = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props.set(name, val)
            }
            "Exposure" => {
                self.exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.exposure_ms))?;
                if !self.ctx.is_null() {
                    unsafe { ffi::andor3_set_exposure_s(self.ctx, self.exposure_ms / 1_000.0) };
                }
                Ok(())
            }
            "PixelEncoding" => {
                self.pixel_enc = val.as_str().to_string();
                self.props.set(name, val.clone())?;
                if !self.ctx.is_null() {
                    let f = cstr("PixelEncoding");
                    let v = cstr(&self.pixel_enc);
                    unsafe { ffi::andor3_set_enum(self.ctx, f.as_ptr(), v.as_ptr()) };
                    self.sync_dims();
                }
                Ok(())
            }
            "Binning" => {
                self.binning = val.as_str().to_string();
                self.props.set(name, val.clone())?;
                if !self.ctx.is_null() {
                    let f = cstr("AOIBinning");
                    let v = cstr(&self.binning);
                    unsafe { ffi::andor3_set_enum(self.ctx, f.as_ptr(), v.as_ptr()) };
                    self.sync_dims();
                }
                Ok(())
            }
            "TriggerMode" => {
                self.trigger_mode = val.as_str().to_string();
                self.props.set(name, val.clone())?;
                if !self.ctx.is_null() {
                    let f = cstr("TriggerMode");
                    let v = cstr(&self.trigger_mode);
                    unsafe { ffi::andor3_set_enum(self.ctx, f.as_ptr(), v.as_ptr()) };
                }
                Ok(())
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Camera }
    fn busy(&self) -> bool { false }
}

// ── Camera trait ───────────────────────────────────────────────────────────────

impl Camera for Andor3Camera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_open()?;
        if self.capturing {
            let timeout = self.snap_timeout_ms();
            let rc = unsafe { ffi::andor3_get_next_frame(self.ctx, timeout) };
            if rc != 0 { return Err(MmError::SnapImageFailed); }
            return Ok(());
        }
        let timeout = self.snap_timeout_ms();
        let rc = unsafe { ffi::andor3_snap(self.ctx, timeout) };
        if rc != 0 { return Err(MmError::SnapImageFailed); }
        self.sync_dims();
        Ok(())
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if self.ctx.is_null() { return Err(MmError::NotConnected); }
        let ptr = unsafe { ffi::andor3_get_frame_ptr(self.ctx) };
        if ptr.is_null() {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        let bytes = unsafe { ffi::andor3_get_frame_bytes(self.ctx) } as usize;
        if bytes == 0 {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        Ok(unsafe { std::slice::from_raw_parts(ptr, bytes) })
    }

    fn get_image_width(&self) -> u32  { self.img_width }
    fn get_image_height(&self) -> u32 { self.img_height }
    fn get_image_bytes_per_pixel(&self) -> u32 { self.bytes_per_pixel.max(1) }
    fn get_bit_depth(&self) -> u32 { self.bit_depth }
    fn get_number_of_components(&self) -> u32 { 1 }
    fn get_number_of_channels(&self) -> u32 { 1 }
    fn get_exposure(&self) -> f64 { self.exposure_ms }

    fn set_exposure(&mut self, exp_ms: f64) {
        self.exposure_ms = exp_ms;
        self.props.set("Exposure", PropertyValue::Float(exp_ms)).ok();
        if !self.ctx.is_null() {
            unsafe { ffi::andor3_set_exposure_s(self.ctx, exp_ms / 1_000.0) };
        }
    }

    fn get_binning(&self) -> i32 {
        // Parse "NxN" → N
        self.binning.split('x').next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1)
    }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        let s = format!("{}x{}", bin, bin);
        self.set_property("Binning", PropertyValue::String(s))
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        if self.ctx.is_null() {
            return Ok(ImageRoi::new(0, 0, self.img_width, self.img_height));
        }
        let (mut l, mut t, mut w, mut h) = (0i32, 0i32, 0i32, 0i32);
        unsafe { ffi::andor3_get_aoi(self.ctx, &mut l, &mut t, &mut w, &mut h) };
        Ok(ImageRoi::new(l as u32, t as u32, w as u32, h as u32))
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        self.check_open()?;
        let rc = unsafe {
            ffi::andor3_set_aoi(
                self.ctx,
                roi.x as i32, roi.y as i32,
                roi.width as i32, roi.height as i32,
            )
        };
        if rc != 0 { return Err(MmError::Err); }
        self.sync_dims();
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        self.check_open()?;
        unsafe { ffi::andor3_clear_aoi(self.ctx) };
        self.sync_dims();
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, _count: i64, _interval_ms: f64) -> MmResult<()> {
        self.check_open()?;
        if self.capturing { return Ok(()); }
        let rc = unsafe { ffi::andor3_start_cont(self.ctx) };
        if rc != 0 {
            return Err(MmError::LocallyDefined(
                "Andor SDK3: failed to start continuous acquisition".into(),
            ));
        }
        self.capturing = true;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        if !self.capturing { return Ok(()); }
        if !self.ctx.is_null() {
            unsafe { ffi::andor3_stop_cont(self.ctx) };
        }
        self.capturing = false;
        Ok(())
    }

    fn is_capturing(&self) -> bool { self.capturing }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_properties() {
        let d = Andor3Camera::new();
        assert_eq!(d.device_type(), DeviceType::Camera);
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(d.get_binning(), 1);
        assert!(!d.is_capturing());
        assert_eq!(d.get_number_of_components(), 1);
    }

    #[test]
    fn set_camera_index_pre_init() {
        let mut d = Andor3Camera::new();
        d.set_property("CameraIndex", PropertyValue::Integer(1)).unwrap();
        assert_eq!(d.camera_index, 1);
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = Andor3Camera::new();
        d.set_property("Exposure", PropertyValue::Float(50.0)).unwrap();
        assert_eq!(d.exposure_ms, 50.0);
        assert_eq!(d.get_exposure(), 50.0);
    }

    #[test]
    fn set_pixel_encoding_pre_init() {
        let mut d = Andor3Camera::new();
        d.set_property("PixelEncoding", PropertyValue::String("Mono12".into())).unwrap();
        assert_eq!(d.pixel_enc, "Mono12");
    }

    #[test]
    fn set_trigger_mode_pre_init() {
        let mut d = Andor3Camera::new();
        d.set_property("TriggerMode", PropertyValue::String("Software".into())).unwrap();
        assert_eq!(d.trigger_mode, "Software");
    }

    #[test]
    fn binning_parse() {
        let mut d = Andor3Camera::new();
        d.binning = "2x2".into();
        assert_eq!(d.get_binning(), 2);
        d.binning = "4x4".into();
        assert_eq!(d.get_binning(), 4);
    }

    #[test]
    fn snap_without_init_errors() {
        let mut d = Andor3Camera::new();
        assert!(d.snap_image().is_err());
    }

    #[test]
    fn no_image_before_snap() {
        let d = Andor3Camera::new();
        assert!(d.get_image_buffer().is_err());
    }

    #[test]
    fn initialize_no_camera_fails() {
        let mut d = Andor3Camera::new();
        assert!(d.initialize().is_err());
    }

    #[test]
    fn readonly_properties() {
        let d = Andor3Camera::new();
        assert!(d.is_property_read_only("Width"));
        assert!(d.is_property_read_only("Height"));
        assert!(d.is_property_read_only("BitDepth"));
        assert!(d.is_property_read_only("SensorWidth"));
        assert!(d.is_property_read_only("SensorHeight"));
        assert!(d.is_property_read_only("Temperature"));
        assert!(d.is_property_read_only("SerialNumber"));
        assert!(d.is_property_read_only("CameraModel"));
        assert!(!d.is_property_read_only("Exposure"));
        assert!(!d.is_property_read_only("Binning"));
        assert!(!d.is_property_read_only("TriggerMode"));
        assert!(!d.is_property_read_only("PixelEncoding"));
    }

    #[test]
    fn exposure_ms_to_s_conversion() {
        let ms = 33.3_f64;
        let s  = ms / 1_000.0;
        assert!((s - 0.0333).abs() < 1e-6);
    }

    #[test]
    fn snap_timeout_at_least_5s() {
        let d = Andor3Camera::new();
        assert!(d.snap_timeout_ms() >= 5_000);
    }
}
