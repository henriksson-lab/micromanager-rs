use std::ffi::{CStr, CString};

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Camera, Device};
use micromanager::types::{DeviceType, ImageRoi, PropertyValue};

use crate::ffi;

// SAFETY: PICAMCamera holds a raw pointer to an opaque PvcamCtx.
// PVCAM is not thread-safe across handles, but each camera is independent
// and we guarantee single-threaded access per camera via `&mut self`.
unsafe impl Send for PICAMCamera {}

const BUF: usize = 256;

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

fn read_str<F: FnOnce(*mut i8, i32) -> i32>(f: F) -> Option<String> {
    let mut buf = [0i8; BUF];
    if f(buf.as_mut_ptr(), BUF as i32) != 0 {
        return None;
    }
    let s = unsafe { CStr::from_ptr(buf.as_ptr()) };
    Some(s.to_string_lossy().into_owned())
}

// ── Camera struct ─────────────────────────────────────────────────────────────

pub struct PICAMCamera {
    props:    PropertyMap,
    ctx:      *mut ffi::PvcamCtx,

    // Pre-init / cached state
    camera_name:  String,    // PVCAM camera name, e.g. "pvcam0"
    exposure_ms:  f64,
    gain_index:   i32,       // 1-based
    binning:      i32,       // symmetric
    temp_setpoint: f64,

    // Post-init read-only info
    sensor_width:  u32,
    sensor_height: u32,
    img_width:     u32,
    img_height:    u32,
    bit_depth:     u32,
    bytes_per_pixel: u32,

    capturing: bool,
}

impl PICAMCamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("CameraName",   PropertyValue::String("".into()),    false).unwrap();
        props.define_property("Exposure",     PropertyValue::Float(10.0),          false).unwrap();
        props.define_property("GainIndex",    PropertyValue::Integer(1),           false).unwrap();
        props.define_property("Binning",      PropertyValue::Integer(1),           false).unwrap();
        props.define_property("TempSetpoint", PropertyValue::Float(-20.0),         false).unwrap();
        props.define_property("Width",        PropertyValue::Integer(0),           true).unwrap();
        props.define_property("Height",       PropertyValue::Integer(0),           true).unwrap();
        props.define_property("BitDepth",     PropertyValue::Integer(16),          true).unwrap();
        props.define_property("Temperature",  PropertyValue::Float(0.0),           true).unwrap();
        props.define_property("SerialNumber", PropertyValue::String("".into()),    true).unwrap();
        props.define_property("ChipName",     PropertyValue::String("".into()),    true).unwrap();

        Self {
            props,
            ctx: std::ptr::null_mut(),
            camera_name:  String::new(),
            exposure_ms:  10.0,
            gain_index:   1,
            binning:      1,
            temp_setpoint: -20.0,
            sensor_width:  0,
            sensor_height: 0,
            img_width:     0,
            img_height:    0,
            bit_depth:     16,
            bytes_per_pixel: 2,
            capturing:    false,
        }
    }

    fn check_open(&self) -> MmResult<()> {
        if self.ctx.is_null() { Err(MmError::NotConnected) } else { Ok(()) }
    }

    fn pvcam_err() -> MmError {
        let msg = read_str(|b, l| unsafe { ffi::pvcam_get_error_message(b, l) })
            .unwrap_or_else(|| "PVCAM error".into());
        MmError::LocallyDefined(msg)
    }

    fn sync_image_dims(&mut self) {
        if self.ctx.is_null() { return; }
        self.img_width  = unsafe { ffi::pvcam_get_image_width(self.ctx)  } as u32;
        self.img_height = unsafe { ffi::pvcam_get_image_height(self.ctx) } as u32;
        self.props.entry_mut("Width") .map(|e| e.value = PropertyValue::Integer(self.img_width  as i64));
        self.props.entry_mut("Height").map(|e| e.value = PropertyValue::Integer(self.img_height as i64));
    }

    fn apply_roi(&mut self) {
        if self.ctx.is_null() { return; }
        let bin = self.binning as u16;
        let sw  = self.sensor_width  as u16;
        let sh  = self.sensor_height as u16;
        unsafe {
            ffi::pvcam_set_roi(self.ctx, 0, 0, sw, sh, bin, bin);
        }
        self.sync_image_dims();
    }
}

impl Default for PICAMCamera {
    fn default() -> Self { Self::new() }
}

impl Drop for PICAMCamera {
    fn drop(&mut self) {
        let _ = self.stop_sequence_acquisition();
        if !self.ctx.is_null() {
            unsafe { ffi::pvcam_close(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
        unsafe { ffi::pvcam_uninit() };
    }
}

// ── Device trait ──────────────────────────────────────────────────────────────

impl Device for PICAMCamera {
    fn name(&self) -> &str { "PICAMCamera" }
    fn description(&self) -> &str { "Princeton Instruments camera (PVCAM SDK)" }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.ctx.is_null() { return Ok(()); }

        if unsafe { ffi::pvcam_init() } != 0 {
            return Err(MmError::LocallyDefined("pvcam_init failed".into()));
        }

        // Select camera: by name if provided, else first found.
        let cam_name = if self.camera_name.is_empty() {
            let count = unsafe { ffi::pvcam_get_camera_count() };
            if count <= 0 {
                return Err(MmError::LocallyDefined("PVCAM: no cameras found".into()));
            }
            read_str(|b, l| unsafe { ffi::pvcam_get_camera_name(0, b, l) })
                .ok_or_else(|| MmError::LocallyDefined("PVCAM: cannot get camera name".into()))?
        } else {
            self.camera_name.clone()
        };

        let name_cstr = cstr(&cam_name);
        let ctx = unsafe { ffi::pvcam_open(name_cstr.as_ptr()) };
        if ctx.is_null() {
            return Err(Self::pvcam_err());
        }
        self.ctx = ctx;
        self.camera_name = cam_name.clone();
        self.props.entry_mut("CameraName")
            .map(|e| e.value = PropertyValue::String(cam_name));

        // Cache sensor info.
        self.sensor_width  = unsafe { ffi::pvcam_get_sensor_width(ctx)  } as u32;
        self.sensor_height = unsafe { ffi::pvcam_get_sensor_height(ctx) } as u32;
        self.bit_depth     = unsafe { ffi::pvcam_get_bit_depth(ctx) }.max(8) as u32;
        self.bytes_per_pixel = (self.bit_depth + 7) / 8;

        self.props.entry_mut("BitDepth")
            .map(|e| e.value = PropertyValue::Integer(self.bit_depth as i64));

        // Read serial number and chip name.
        if let Some(sn) = read_str(|b, l| unsafe { ffi::pvcam_get_serial_number(ctx, b, l) }) {
            self.props.entry_mut("SerialNumber")
                .map(|e| e.value = PropertyValue::String(sn));
        }
        if let Some(chip) = read_str(|b, l| unsafe { ffi::pvcam_get_chip_name(ctx, b, l) }) {
            self.props.entry_mut("ChipName")
                .map(|e| e.value = PropertyValue::String(chip));
        }

        // Apply pre-init settings.
        self.apply_roi();

        let gi = self.gain_index;
        unsafe { ffi::pvcam_set_gain_index(ctx, gi) };

        let ts = self.temp_setpoint;
        unsafe { ffi::pvcam_set_temp_setpoint(ctx, ts) };

        // Read back gain range and populate allowed values as strings.
        let gain_max = unsafe { ffi::pvcam_get_gain_max(ctx) }.max(1) as i64;
        let allowed: Vec<String> = (1..=gain_max).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = allowed.iter().map(|s| s.as_str()).collect();
        self.props.set_allowed_values("GainIndex", &refs).ok();

        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        let _ = self.stop_sequence_acquisition();
        if !self.ctx.is_null() {
            unsafe { ffi::pvcam_close(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
        unsafe { ffi::pvcam_uninit() };
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "CameraName"   => Ok(PropertyValue::String(self.camera_name.clone())),
            "Exposure"     => Ok(PropertyValue::Float(self.exposure_ms)),
            "GainIndex"    => {
                if !self.ctx.is_null() {
                    let g = unsafe { ffi::pvcam_get_gain_index(self.ctx) };
                    if g >= 0 { return Ok(PropertyValue::Integer(g as i64)); }
                }
                Ok(PropertyValue::Integer(self.gain_index as i64))
            }
            "Binning"      => Ok(PropertyValue::Integer(self.binning as i64)),
            "Temperature"  => {
                if !self.ctx.is_null() {
                    let t = unsafe { ffi::pvcam_get_temperature(self.ctx) };
                    return Ok(PropertyValue::Float(t));
                }
                self.props.get("Temperature").cloned()
            }
            "TempSetpoint" => {
                if !self.ctx.is_null() {
                    let t = unsafe { ffi::pvcam_get_temp_setpoint(self.ctx) };
                    return Ok(PropertyValue::Float(t));
                }
                Ok(PropertyValue::Float(self.temp_setpoint))
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "CameraName" => {
                if !self.ctx.is_null() {
                    return Err(MmError::LocallyDefined(
                        "CameraName cannot be changed after initialize()".into(),
                    ));
                }
                self.camera_name = val.as_str().to_string();
                self.props.set(name, val)
            }
            "Exposure" => {
                self.exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.exposure_ms))
            }
            "GainIndex" => {
                self.gain_index = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props.set(name, PropertyValue::Integer(self.gain_index as i64))?;
                if !self.ctx.is_null() {
                    unsafe { ffi::pvcam_set_gain_index(self.ctx, self.gain_index) };
                }
                Ok(())
            }
            "Binning" => {
                self.binning = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props.set(name, PropertyValue::Integer(self.binning as i64))?;
                self.apply_roi();
                Ok(())
            }
            "TempSetpoint" => {
                self.temp_setpoint = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.temp_setpoint))?;
                if !self.ctx.is_null() {
                    unsafe { ffi::pvcam_set_temp_setpoint(self.ctx, self.temp_setpoint) };
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

// ── Camera trait ──────────────────────────────────────────────────────────────

impl Camera for PICAMCamera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_open()?;

        if self.capturing {
            // Continuous mode: get the oldest queued frame.
            let mut frame_ptr: *const u8 = std::ptr::null();
            let rc = unsafe { ffi::pvcam_get_frame_cont(self.ctx, &mut frame_ptr) };
            if rc != 0 || frame_ptr.is_null() {
                return Err(MmError::SnapImageFailed);
            }
            // pvcam_get_frame_cont copies the pointer; the data is inside the
            // circular buffer managed by the shim.  We just release it.
            unsafe { ffi::pvcam_release_frame_cont(self.ctx) };
            return Ok(());
        }

        // Single-frame snap (blocking, up to 10 s timeout).
        let timeout_ms = (self.exposure_ms as u32 + 1).max(10_000);
        let rc = unsafe {
            ffi::pvcam_snap(self.ctx, self.exposure_ms as u32, timeout_ms)
        };
        if rc != 0 {
            return Err(Self::pvcam_err());
        }

        // Update image dimensions (they might have changed if ROI/binning changed).
        self.img_width  = unsafe { ffi::pvcam_get_image_width(self.ctx)  } as u32;
        self.img_height = unsafe { ffi::pvcam_get_image_height(self.ctx) } as u32;
        self.props.entry_mut("Width") .map(|e| e.value = PropertyValue::Integer(self.img_width  as i64));
        self.props.entry_mut("Height").map(|e| e.value = PropertyValue::Integer(self.img_height as i64));

        Ok(())
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if self.ctx.is_null() { return Err(MmError::NotConnected); }
        let ptr = unsafe { ffi::pvcam_get_snap_frame(self.ctx) };
        if ptr.is_null() {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        let size = unsafe { ffi::pvcam_get_frame_size(self.ctx) } as usize;
        if size == 0 {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        // SAFETY: the shim owns the buffer for the lifetime of ctx;
        // we borrow it here with the same lifetime as &self.
        Ok(unsafe { std::slice::from_raw_parts(ptr, size) })
    }

    fn get_image_width(&self) -> u32  { self.img_width }
    fn get_image_height(&self) -> u32 { self.img_height }
    fn get_image_bytes_per_pixel(&self) -> u32 { self.bytes_per_pixel }
    fn get_bit_depth(&self) -> u32 { self.bit_depth }
    fn get_number_of_components(&self) -> u32 { 1 }
    fn get_number_of_channels(&self) -> u32 { 1 }
    fn get_exposure(&self) -> f64 { self.exposure_ms }

    fn set_exposure(&mut self, exp_ms: f64) {
        self.exposure_ms = exp_ms;
        self.props.set("Exposure", PropertyValue::Float(exp_ms)).ok();
    }

    fn get_binning(&self) -> i32 { self.binning }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        self.binning = bin;
        self.props.set("Binning", PropertyValue::Integer(bin as i64))?;
        self.apply_roi();
        Ok(())
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        Ok(ImageRoi::new(0, 0, self.img_width, self.img_height))
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        self.check_open()?;
        let bin = self.binning as u16;
        unsafe {
            ffi::pvcam_set_roi(
                self.ctx,
                roi.x as u16, roi.y as u16,
                roi.width as u16, roi.height as u16,
                bin, bin,
            );
        }
        self.sync_image_dims();
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        self.check_open()?;
        unsafe { ffi::pvcam_clear_roi(self.ctx) };
        self.sync_image_dims();
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, _count: i64, _interval_ms: f64) -> MmResult<()> {
        self.check_open()?;
        if self.capturing { return Ok(()); }

        // Use 8 circular frames.
        let rc = unsafe {
            ffi::pvcam_start_cont(self.ctx, self.exposure_ms as u32, 8)
        };
        if rc != 0 {
            return Err(Self::pvcam_err());
        }
        self.capturing = true;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        if !self.capturing { return Ok(()); }
        if !self.ctx.is_null() {
            unsafe { ffi::pvcam_stop_cont(self.ctx) };
        }
        self.capturing = false;
        Ok(())
    }

    fn is_capturing(&self) -> bool { self.capturing }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_properties() {
        let d = PICAMCamera::new();
        assert_eq!(d.device_type(), DeviceType::Camera);
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(d.get_binning(), 1);
        assert!(!d.is_capturing());
        assert_eq!(d.get_number_of_components(), 1);
        assert_eq!(d.get_number_of_channels(), 1);
    }

    #[test]
    fn set_camera_name_pre_init() {
        let mut d = PICAMCamera::new();
        d.set_property("CameraName", PropertyValue::String("pvcam1".into())).unwrap();
        assert_eq!(d.camera_name, "pvcam1");
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = PICAMCamera::new();
        d.set_property("Exposure", PropertyValue::Float(100.0)).unwrap();
        assert_eq!(d.exposure_ms, 100.0);
        assert_eq!(d.get_exposure(), 100.0);
    }

    #[test]
    fn set_gain_pre_init() {
        let mut d = PICAMCamera::new();
        d.set_property("GainIndex", PropertyValue::Integer(3)).unwrap();
        assert_eq!(d.gain_index, 3);
    }

    #[test]
    fn set_temp_setpoint_pre_init() {
        let mut d = PICAMCamera::new();
        d.set_property("TempSetpoint", PropertyValue::Float(-30.0)).unwrap();
        assert_eq!(d.temp_setpoint, -30.0);
    }

    #[test]
    fn snap_without_init_errors() {
        let mut d = PICAMCamera::new();
        assert!(d.snap_image().is_err());
    }

    #[test]
    fn no_image_before_snap() {
        let d = PICAMCamera::new();
        assert!(d.get_image_buffer().is_err());
    }

    #[test]
    fn initialize_no_camera_fails() {
        let mut d = PICAMCamera::new();
        // No PVCAM cameras present — expect a meaningful error.
        assert!(d.initialize().is_err());
    }

    #[test]
    fn readonly_properties() {
        let d = PICAMCamera::new();
        assert!(d.is_property_read_only("Width"));
        assert!(d.is_property_read_only("Height"));
        assert!(d.is_property_read_only("BitDepth"));
        assert!(d.is_property_read_only("SerialNumber"));
        assert!(d.is_property_read_only("ChipName"));
        assert!(!d.is_property_read_only("Exposure"));
        assert!(!d.is_property_read_only("GainIndex"));
    }
}
