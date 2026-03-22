use std::ffi::{CStr, CString};

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Camera, Device};
use micromanager::types::{DeviceType, ImageRoi, PropertyValue};

use crate::ffi;

// SAFETY: TSICamera holds a raw pointer to TsiCtx.  The TSI SDK is not
// internally thread-safe per camera handle; we enforce single-thread access
// via `&mut self` on all mutating methods.
unsafe impl Send for TSICamera {}

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

fn sensor_type_name(t: i32) -> &'static str {
    match t {
        1 => "Bayer",
        2 => "Polarized",
        _ => "Monochrome",
    }
}

// ── Camera struct ─────────────────────────────────────────────────────────────

pub struct TSICamera {
    props:    PropertyMap,
    ctx:      *mut ffi::TsiCtx,

    // Pre-init / cached
    camera_id:   String,   // TSI camera ID string; empty = first found
    exposure_ms: f64,
    binning:     i32,

    // Post-init read-only
    img_width:      u32,
    img_height:     u32,
    bit_depth:      u32,
    bytes_per_pixel: u32,
    sensor_type:    i32,

    capturing: bool,
}

impl TSICamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("CameraID",    PropertyValue::String("".into()),          false).unwrap();
        props.define_property("Exposure",    PropertyValue::Float(10.0),               false).unwrap();
        props.define_property("Binning",     PropertyValue::Integer(1),                false).unwrap();
        props.define_property("Width",       PropertyValue::Integer(0),                true).unwrap();
        props.define_property("Height",      PropertyValue::Integer(0),                true).unwrap();
        props.define_property("BitDepth",    PropertyValue::Integer(16),               true).unwrap();
        props.define_property("SensorType",  PropertyValue::String("Monochrome".into()), true).unwrap();
        props.define_property("SerialNumber",PropertyValue::String("".into()),         true).unwrap();
        props.define_property("FirmwareVer", PropertyValue::String("".into()),         true).unwrap();

        Self {
            props,
            ctx: std::ptr::null_mut(),
            camera_id:      String::new(),
            exposure_ms:    10.0,
            binning:        1,
            img_width:      0,
            img_height:     0,
            bit_depth:      16,
            bytes_per_pixel: 2,
            sensor_type:    0,
            capturing:      false,
        }
    }

    fn check_open(&self) -> MmResult<()> {
        if self.ctx.is_null() { Err(MmError::NotConnected) } else { Ok(()) }
    }

    fn sync_dims(&mut self) {
        if self.ctx.is_null() { return; }
        self.img_width  = unsafe { ffi::tsi_get_image_width(self.ctx)  } as u32;
        self.img_height = unsafe { ffi::tsi_get_image_height(self.ctx) } as u32;
        self.props.entry_mut("Width") .map(|e| e.value = PropertyValue::Integer(self.img_width  as i64));
        self.props.entry_mut("Height").map(|e| e.value = PropertyValue::Integer(self.img_height as i64));
    }

    fn apply_binning(&mut self) {
        if self.ctx.is_null() { return; }
        let b = self.binning;
        unsafe {
            ffi::tsi_set_binx(self.ctx, b);
            ffi::tsi_set_biny(self.ctx, b);
        }
        self.sync_dims();
    }

    /// Snap timeout: exposure + generous readout overhead, minimum 5 s.
    fn snap_timeout_ms(&self) -> i32 {
        (self.exposure_ms as i32 + 5_000).max(5_000)
    }
}

impl Default for TSICamera {
    fn default() -> Self { Self::new() }
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
    fn name(&self) -> &str { "TSICamera" }
    fn description(&self) -> &str { "Thorlabs Scientific Imaging camera (TSI SDK3)" }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.ctx.is_null() { return Ok(()); }

        if unsafe { ffi::tsi_sdk_open() } != 0 {
            return Err(MmError::LocallyDefined("tsi_sdk_open failed".into()));
        }

        // Discover cameras.
        let mut disc_buf = [0i8; 4096];
        let count = unsafe { ffi::tsi_discover_cameras(disc_buf.as_mut_ptr(), 4096) };
        if count < 0 {
            return Err(MmError::LocallyDefined("TSI: camera discovery failed".into()));
        }
        if count == 0 {
            return Err(MmError::LocallyDefined("TSI: no cameras found".into()));
        }

        // Parse the space-separated ID list.
        let ids_str = unsafe { CStr::from_ptr(disc_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let ids: Vec<&str> = ids_str.split_whitespace().collect();

        // Select camera: match by pre-configured ID or take the first one.
        let target_id: &str = if self.camera_id.is_empty() {
            ids[0]
        } else {
            ids.iter()
                .find(|&&id| id == self.camera_id.as_str())
                .ok_or_else(|| {
                    MmError::LocallyDefined(format!(
                        "TSI: camera '{}' not found (available: {})",
                        self.camera_id, ids_str
                    ))
                })?
        };

        let id_cstr = cstr(target_id);
        let ctx = unsafe { ffi::tsi_open_camera(id_cstr.as_ptr()) };
        if ctx.is_null() {
            return Err(MmError::LocallyDefined(
                format!("TSI: failed to open camera '{}'", target_id),
            ));
        }
        self.ctx = ctx;
        self.camera_id = target_id.to_string();
        self.props.entry_mut("CameraID")
            .map(|e| e.value = PropertyValue::String(self.camera_id.clone()));

        // Read sensor properties.
        self.bit_depth     = unsafe { ffi::tsi_get_bit_depth(ctx) }.max(1) as u32;
        self.bytes_per_pixel = unsafe { ffi::tsi_get_bytes_per_pixel(ctx) }.max(1) as u32;
        self.sensor_type   = unsafe { ffi::tsi_get_sensor_type(ctx) };

        self.props.entry_mut("BitDepth")
            .map(|e| e.value = PropertyValue::Integer(self.bit_depth as i64));
        self.props.entry_mut("SensorType")
            .map(|e| e.value = PropertyValue::String(sensor_type_name(self.sensor_type).into()));

        if let Some(sn) = read_str(|b, l| unsafe { ffi::tsi_get_serial_number(ctx, b, l) }) {
            self.props.entry_mut("SerialNumber")
                .map(|e| e.value = PropertyValue::String(sn));
        }
        if let Some(fw) = read_str(|b, l| unsafe { ffi::tsi_get_firmware_version(ctx, b, l) }) {
            self.props.entry_mut("FirmwareVer")
                .map(|e| e.value = PropertyValue::String(fw));
        }

        // Apply pre-init settings.
        let exp_us = (self.exposure_ms * 1_000.0) as i64;
        unsafe { ffi::tsi_set_exposure_us(ctx, exp_us) };
        self.apply_binning();
        self.sync_dims();

        // Populate binning allowed values from camera range.
        let mut bin_min = 1i32;
        let mut bin_max = 1i32;
        unsafe { ffi::tsi_get_binx_range(ctx, &mut bin_min, &mut bin_max) };
        let allowed: Vec<String> = (bin_min..=bin_max)
            .filter(|&b| (bin_max as f64).log2() >= 0.0 && b.count_ones() == 1)   // powers of 2
            .map(|b| b.to_string())
            .collect();
        if !allowed.is_empty() {
            let refs: Vec<&str> = allowed.iter().map(|s| s.as_str()).collect();
            self.props.set_allowed_values("Binning", &refs).ok();
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
            "CameraID"   => Ok(PropertyValue::String(self.camera_id.clone())),
            "Exposure"   => Ok(PropertyValue::Float(self.exposure_ms)),
            "Binning"    => Ok(PropertyValue::Integer(self.binning as i64)),
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
                self.exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.exposure_ms))?;
                if !self.ctx.is_null() {
                    let us = (self.exposure_ms * 1_000.0) as i64;
                    unsafe { ffi::tsi_set_exposure_us(self.ctx, us) };
                }
                Ok(())
            }
            "Binning" => {
                self.binning = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props.set(name, PropertyValue::Integer(self.binning as i64))?;
                self.apply_binning();
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

impl Camera for TSICamera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_open()?;

        if self.capturing {
            // Sequence mode: wait for the next frame from the continuous stream.
            let timeout = self.snap_timeout_ms();
            let rc = unsafe { ffi::tsi_get_next_frame(self.ctx, timeout) };
            if rc != 0 { return Err(MmError::SnapImageFailed); }
            return Ok(());
        }

        // Single-frame snap.
        let timeout = self.snap_timeout_ms();
        let rc = unsafe { ffi::tsi_snap(self.ctx, timeout) };
        if rc != 0 { return Err(MmError::SnapImageFailed); }

        self.img_width  = unsafe { ffi::tsi_get_image_width(self.ctx)  } as u32;
        self.img_height = unsafe { ffi::tsi_get_image_height(self.ctx) } as u32;
        self.props.entry_mut("Width") .map(|e| e.value = PropertyValue::Integer(self.img_width  as i64));
        self.props.entry_mut("Height").map(|e| e.value = PropertyValue::Integer(self.img_height as i64));
        Ok(())
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if self.ctx.is_null() { return Err(MmError::NotConnected); }
        let ptr = unsafe { ffi::tsi_get_frame_ptr(self.ctx) };
        if ptr.is_null() {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        let bytes = unsafe { ffi::tsi_get_frame_bytes(self.ctx) } as usize;
        if bytes == 0 {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        // SAFETY: ptr points into the shim's internal buffer which lives for
        // the duration of ctx; we borrow it with the same lifetime as &self.
        Ok(unsafe { std::slice::from_raw_parts(ptr as *const u8, bytes) })
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
        if !self.ctx.is_null() {
            unsafe { ffi::tsi_set_exposure_us(self.ctx, (exp_ms * 1_000.0) as i64) };
        }
    }

    fn get_binning(&self) -> i32 { self.binning }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        self.binning = bin;
        self.props.set("Binning", PropertyValue::Integer(bin as i64))?;
        self.apply_binning();
        Ok(())
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        if self.ctx.is_null() {
            return Ok(ImageRoi::new(0, 0, self.img_width, self.img_height));
        }
        let (mut x, mut y, mut w, mut h) = (0i32, 0i32, 0i32, 0i32);
        unsafe { ffi::tsi_get_roi(self.ctx, &mut x, &mut y, &mut w, &mut h) };
        Ok(ImageRoi::new(x as u32, y as u32, w as u32, h as u32))
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        self.check_open()?;
        let rc = unsafe {
            ffi::tsi_set_roi(
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
        unsafe { ffi::tsi_clear_roi(self.ctx) };
        self.sync_dims();
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, _count: i64, _interval_ms: f64) -> MmResult<()> {
        self.check_open()?;
        if self.capturing { return Ok(()); }

        let rc = unsafe { ffi::tsi_start_cont(self.ctx) };
        if rc != 0 {
            return Err(MmError::LocallyDefined("TSI: failed to start continuous acquisition".into()));
        }
        self.capturing = true;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        if !self.capturing { return Ok(()); }
        if !self.ctx.is_null() {
            unsafe { ffi::tsi_stop_cont(self.ctx) };
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
        let d = TSICamera::new();
        assert_eq!(d.device_type(), DeviceType::Camera);
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(d.get_binning(), 1);
        assert!(!d.is_capturing());
        assert_eq!(d.get_number_of_components(), 1);
    }

    #[test]
    fn set_camera_id_pre_init() {
        let mut d = TSICamera::new();
        d.set_property("CameraID", PropertyValue::String("CS2100M-USB".into())).unwrap();
        assert_eq!(d.camera_id, "CS2100M-USB");
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = TSICamera::new();
        d.set_property("Exposure", PropertyValue::Float(50.0)).unwrap();
        assert_eq!(d.exposure_ms, 50.0);
        assert_eq!(d.get_exposure(), 50.0);
    }

    #[test]
    fn set_binning_pre_init() {
        let mut d = TSICamera::new();
        d.set_property("Binning", PropertyValue::Integer(2)).unwrap();
        assert_eq!(d.binning, 2);
        assert_eq!(d.get_binning(), 2);
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
    fn initialize_no_camera_fails() {
        let mut d = TSICamera::new();
        // No TSI cameras present — expect an error.
        assert!(d.initialize().is_err());
    }

    #[test]
    fn readonly_properties() {
        let d = TSICamera::new();
        assert!(d.is_property_read_only("Width"));
        assert!(d.is_property_read_only("Height"));
        assert!(d.is_property_read_only("BitDepth"));
        assert!(d.is_property_read_only("SensorType"));
        assert!(d.is_property_read_only("SerialNumber"));
        assert!(d.is_property_read_only("FirmwareVer"));
        assert!(!d.is_property_read_only("Exposure"));
        assert!(!d.is_property_read_only("Binning"));
    }

    #[test]
    fn exposure_ms_to_us_conversion() {
        // Verify the conversion factor (tested without SDK).
        let exp_ms = 15.5_f64;
        let exp_us = (exp_ms * 1_000.0) as i64;
        assert_eq!(exp_us, 15_500);
    }
}
