use std::ffi::{CStr, CString};
use std::sync::Arc;

use crate::circular_buffer::ImageFrame;
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Camera, Device, SequenceImageSink};
use crate::types::{DeviceType, ImageRoi, PropertyValue};

use super::ffi;

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
    props: PropertyMap,
    ctx: *mut ffi::PvcamCtx,

    // Pre-init / cached state
    camera_name: String, // PVCAM camera name, e.g. "pvcam0"
    exposure_ms: f64,
    gain_index: i32, // 1-based
    binning_x: i32,
    binning_y: i32,
    temp_setpoint: f64,
    roi_x: u32,
    roi_y: u32,
    roi_width: u32,
    roi_height: u32,
    sequence_remaining: Option<i64>,

    // Post-init read-only info
    sensor_width: u32,
    sensor_height: u32,
    img_width: u32,
    img_height: u32,
    bit_depth: u32,
    bytes_per_pixel: u32,
    image_buf: Vec<u8>,

    capturing: bool,
    sequence_image_sink: Option<Arc<dyn SequenceImageSink>>,
}

impl PICAMCamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("CameraName", PropertyValue::String("".into()), false)
            .unwrap();
        props
            .define_property("Name", PropertyValue::String("".into()), false)
            .unwrap();
        props
            .define_property("Exposure", PropertyValue::Float(10.0), false)
            .unwrap();
        props
            .define_property("GainIndex", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .define_property("Gain", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .define_property("Binning", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .set_allowed_values("Binning", &["1", "2", "4", "8"])
            .unwrap();
        props
            .define_property("BinningX", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .set_allowed_values("BinningX", &["1", "2", "4", "8"])
            .unwrap();
        props
            .define_property("BinningY", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .set_allowed_values("BinningY", &["1", "2", "4", "8"])
            .unwrap();
        props
            .define_property("TempSetpoint", PropertyValue::Float(-20.0), false)
            .unwrap();
        props
            .define_property("CCDTemperatureSetPoint", PropertyValue::Float(-20.0), false)
            .unwrap();
        props
            .define_property("Width", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("X-dimension", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("Height", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("Y-dimension", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("BitDepth", PropertyValue::Integer(16), true)
            .unwrap();
        props
            .define_property("PixelType", PropertyValue::String("16bit".into()), true)
            .unwrap();
        props
            .define_property("Temperature", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("CCDTemperature", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("SerialNumber", PropertyValue::String("".into()), true)
            .unwrap();
        props
            .define_property("ChipName", PropertyValue::String("".into()), true)
            .unwrap();

        Self {
            props,
            ctx: std::ptr::null_mut(),
            camera_name: String::new(),
            exposure_ms: 10.0,
            gain_index: 1,
            binning_x: 1,
            binning_y: 1,
            temp_setpoint: -20.0,
            roi_x: 0,
            roi_y: 0,
            roi_width: 0,
            roi_height: 0,
            sequence_remaining: None,
            sensor_width: 0,
            sensor_height: 0,
            img_width: 0,
            img_height: 0,
            bit_depth: 16,
            bytes_per_pixel: 2,
            image_buf: Vec::new(),
            capturing: false,
            sequence_image_sink: None,
        }
    }

    fn check_open(&self) -> MmResult<()> {
        if self.ctx.is_null() {
            Err(MmError::NotConnected)
        } else {
            Ok(())
        }
    }

    fn pvcam_err() -> MmError {
        let msg = read_str(|b, l| unsafe { ffi::pvcam_get_error_message(b, l) })
            .unwrap_or_else(|| "PVCAM error".into());
        MmError::LocallyDefined(msg)
    }

    fn sync_image_dims(&mut self) {
        if self.ctx.is_null() {
            return;
        }
        self.img_width = unsafe { ffi::pvcam_get_image_width(self.ctx) } as u32;
        self.img_height = unsafe { ffi::pvcam_get_image_height(self.ctx) } as u32;
        self.props
            .entry_mut("Width")
            .map(|e| e.value = PropertyValue::Integer(self.img_width as i64));
        self.props
            .entry_mut("X-dimension")
            .map(|e| e.value = PropertyValue::Integer(self.img_width as i64));
        self.props
            .entry_mut("Height")
            .map(|e| e.value = PropertyValue::Integer(self.img_height as i64));
        self.props
            .entry_mut("Y-dimension")
            .map(|e| e.value = PropertyValue::Integer(self.img_height as i64));
    }

    fn apply_roi(&mut self) {
        if self.ctx.is_null() {
            return;
        }
        let xbin = self.binning_x as u16;
        let ybin = self.binning_y as u16;
        let mm_width = if self.roi_width == 0 {
            self.sensor_width.saturating_sub(self.roi_x)
        } else {
            self.roi_width
        };
        let mm_height = if self.roi_height == 0 {
            self.sensor_height.saturating_sub(self.roi_y)
        } else {
            self.roi_height
        };
        let full_sensor_roi = self.roi_x == 0
            && self.roi_y == 0
            && mm_width == self.sensor_width
            && mm_height == self.sensor_height;
        let (sensor_x, sensor_y, sensor_width, sensor_height) = if full_sensor_roi {
            (0, 0, self.sensor_width, self.sensor_height)
        } else {
            (
                self.roi_x.saturating_mul(self.binning_x as u32),
                self.roi_y.saturating_mul(self.binning_y as u32),
                mm_width.saturating_mul(self.binning_x as u32),
                mm_height.saturating_mul(self.binning_y as u32),
            )
        };
        unsafe {
            ffi::pvcam_set_roi(
                self.ctx,
                sensor_x as u16,
                sensor_y as u16,
                sensor_width as u16,
                sensor_height as u16,
                xbin,
                ybin,
            );
        }
        self.sync_image_dims();
    }

    fn validate_binning(bin: i32) -> MmResult<i32> {
        match bin {
            1 | 2 | 4 | 8 => Ok(bin),
            _ => Err(MmError::InvalidPropertyValue),
        }
    }

    fn set_binning_cached(&mut self, xbin: i32, ybin: i32) -> MmResult<()> {
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        self.binning_x = Self::validate_binning(xbin)?;
        self.binning_y = Self::validate_binning(ybin)?;
        let symmetric = self.binning_x == self.binning_y;
        self.props.set(
            "Binning",
            PropertyValue::Integer(if symmetric { self.binning_x } else { 1 } as i64),
        )?;
        self.props
            .set("BinningX", PropertyValue::Integer(self.binning_x as i64))?;
        self.props
            .set("BinningY", PropertyValue::Integer(self.binning_y as i64))?;
        self.apply_roi();
        Ok(())
    }
}

impl Default for PICAMCamera {
    fn default() -> Self {
        Self::new()
    }
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
    fn name(&self) -> &str {
        "PICAMCamera"
    }
    fn description(&self) -> &str {
        "PICAM API device adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.ctx.is_null() {
            return Ok(());
        }

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
        self.props
            .entry_mut("CameraName")
            .map(|e| e.value = PropertyValue::String(cam_name.clone()));
        self.props
            .entry_mut("Name")
            .map(|e| e.value = PropertyValue::String(cam_name));

        // Cache sensor info.
        self.sensor_width = unsafe { ffi::pvcam_get_sensor_width(ctx) } as u32;
        self.sensor_height = unsafe { ffi::pvcam_get_sensor_height(ctx) } as u32;
        self.bit_depth = unsafe { ffi::pvcam_get_bit_depth(ctx) }.max(8) as u32;
        self.bytes_per_pixel = (self.bit_depth + 7) / 8;

        self.props
            .entry_mut("BitDepth")
            .map(|e| e.value = PropertyValue::Integer(self.bit_depth as i64));
        let pixel_type = if self.bit_depth <= 8 { "8bit" } else { "16bit" };
        self.props
            .entry_mut("PixelType")
            .map(|e| e.value = PropertyValue::String(pixel_type.into()));

        // Read serial number and chip name.
        if let Some(sn) = read_str(|b, l| unsafe { ffi::pvcam_get_serial_number(ctx, b, l) }) {
            self.props
                .entry_mut("SerialNumber")
                .map(|e| e.value = PropertyValue::String(sn));
        }
        if let Some(chip) = read_str(|b, l| unsafe { ffi::pvcam_get_chip_name(ctx, b, l) }) {
            self.props
                .entry_mut("ChipName")
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
        self.props.set_allowed_values("Gain", &refs).ok();

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
            "CameraName" | "Name" => Ok(PropertyValue::String(self.camera_name.clone())),
            "Exposure" => Ok(PropertyValue::Float(self.exposure_ms)),
            "GainIndex" | "Gain" => {
                if !self.ctx.is_null() {
                    let g = unsafe { ffi::pvcam_get_gain_index(self.ctx) };
                    if g >= 0 {
                        return Ok(PropertyValue::Integer(g as i64));
                    }
                }
                Ok(PropertyValue::Integer(self.gain_index as i64))
            }
            "Binning" => Ok(PropertyValue::Integer(
                if self.binning_x == self.binning_y {
                    self.binning_x
                } else {
                    1
                } as i64,
            )),
            "BinningX" => Ok(PropertyValue::Integer(self.binning_x as i64)),
            "BinningY" => Ok(PropertyValue::Integer(self.binning_y as i64)),
            "Temperature" => {
                if !self.ctx.is_null() {
                    let t = unsafe { ffi::pvcam_get_temperature(self.ctx) };
                    return Ok(PropertyValue::Float(t));
                }
                self.props.get("Temperature").cloned()
            }
            "CCDTemperature" => {
                if !self.ctx.is_null() {
                    let t = unsafe { ffi::pvcam_get_temperature(self.ctx) };
                    return Ok(PropertyValue::Float(t));
                }
                self.props.get("CCDTemperature").cloned()
            }
            "TempSetpoint" | "CCDTemperatureSetPoint" => {
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
            "CameraName" | "Name" => {
                if !self.ctx.is_null() {
                    return Err(MmError::LocallyDefined(
                        "CameraName cannot be changed after initialize()".into(),
                    ));
                }
                self.camera_name = val.as_str().to_string();
                self.props.set(
                    "CameraName",
                    PropertyValue::String(self.camera_name.clone()),
                )?;
                self.props
                    .set("Name", PropertyValue::String(self.camera_name.clone()))?;
                Ok(())
            }
            "Exposure" => {
                self.exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.exposure_ms))
            }
            "GainIndex" | "Gain" => {
                self.gain_index = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props
                    .set("GainIndex", PropertyValue::Integer(self.gain_index as i64))?;
                self.props
                    .set("Gain", PropertyValue::Integer(self.gain_index as i64))?;
                if !self.ctx.is_null() {
                    unsafe { ffi::pvcam_set_gain_index(self.ctx, self.gain_index) };
                }
                Ok(())
            }
            "Binning" => {
                let bin = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.set_binning_cached(bin, bin)
            }
            "BinningX" => {
                let bin = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.set_binning_cached(bin, self.binning_y)
            }
            "BinningY" => {
                let bin = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.set_binning_cached(self.binning_x, bin)
            }
            "TempSetpoint" | "CCDTemperatureSetPoint" => {
                self.temp_setpoint = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set("TempSetpoint", PropertyValue::Float(self.temp_setpoint))?;
                self.props.set(
                    "CCDTemperatureSetPoint",
                    PropertyValue::Float(self.temp_setpoint),
                )?;
                if !self.ctx.is_null() {
                    unsafe { ffi::pvcam_set_temp_setpoint(self.ctx, self.temp_setpoint) };
                }
                Ok(())
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
            let size = unsafe { ffi::pvcam_get_frame_size(self.ctx) } as usize;
            if size == 0 {
                unsafe { ffi::pvcam_release_frame_cont(self.ctx) };
                return Err(MmError::SnapImageFailed);
            }
            self.image_buf.clear();
            self.image_buf
                .extend_from_slice(unsafe { std::slice::from_raw_parts(frame_ptr, size) });
            unsafe { ffi::pvcam_release_frame_cont(self.ctx) };
            if let Some(sink) = &self.sequence_image_sink {
                if sink.insert_sequence_image(ImageFrame::new(
                    self.image_buf.clone(),
                    self.img_width,
                    self.img_height,
                    self.bytes_per_pixel,
                )) {
                    self.stop_sequence_acquisition()?;
                    return Err(MmError::BufferOverflow);
                }
            }
            if let Some(remaining) = self.sequence_remaining.as_mut() {
                *remaining -= 1;
                if *remaining <= 0 {
                    self.stop_sequence_acquisition()?;
                }
            }
            return Ok(());
        }

        // Single-frame snap (blocking, up to 10 s timeout).
        let timeout_ms = (self.exposure_ms as u32 + 1).max(10_000);
        let rc = unsafe { ffi::pvcam_snap(self.ctx, self.exposure_ms as u32, timeout_ms) };
        if rc != 0 {
            return Err(Self::pvcam_err());
        }

        // Update image dimensions (they might have changed if ROI/binning changed).
        self.sync_image_dims();

        let ptr = unsafe { ffi::pvcam_get_snap_frame(self.ctx) };
        let size = unsafe { ffi::pvcam_get_frame_size(self.ctx) } as usize;
        if ptr.is_null() || size == 0 {
            return Err(MmError::SnapImageFailed);
        }
        self.image_buf.clear();
        self.image_buf
            .extend_from_slice(unsafe { std::slice::from_raw_parts(ptr, size) });

        Ok(())
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if self.ctx.is_null() {
            return Err(MmError::NotConnected);
        }
        if !self.image_buf.is_empty() {
            return Ok(&self.image_buf);
        }
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
        self.props.set("Exposure", PropertyValue::Float(exp_ms))?;
        Ok(())
    }

    fn get_binning(&self) -> i32 {
        if self.binning_x == self.binning_y {
            self.binning_x
        } else {
            1
        }
    }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        self.set_binning_cached(bin, bin)
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        Ok(ImageRoi::new(
            self.roi_x,
            self.roi_y,
            if self.roi_width == 0 {
                self.sensor_width.saturating_sub(self.roi_x)
            } else {
                self.roi_width
            },
            if self.roi_height == 0 {
                self.sensor_height.saturating_sub(self.roi_y)
            } else {
                self.roi_height
            },
        ))
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        self.check_open()?;
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        if roi.width.saturating_mul(roi.height) < 4 {
            return Err(MmError::InvalidPropertyValue);
        }
        self.roi_x = roi.x;
        self.roi_y = roi.y;
        self.roi_width = roi.width;
        self.roi_height = roi.height;
        self.apply_roi();
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        self.check_open()?;
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        self.roi_x = 0;
        self.roi_y = 0;
        self.roi_width = 0;
        self.roi_height = 0;
        self.apply_roi();
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, count: i64, _interval_ms: f64) -> MmResult<()> {
        self.check_open()?;
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        if count < 0 {
            return Err(MmError::InvalidPropertyValue);
        }

        // Use 8 circular frames.
        let rc = unsafe { ffi::pvcam_start_cont(self.ctx, self.exposure_ms as u32, 8) };
        if rc != 0 {
            return Err(Self::pvcam_err());
        }
        self.capturing = true;
        self.sequence_remaining = if count > 0 { Some(count) } else { None };
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        if !self.capturing {
            return Ok(());
        }
        if !self.ctx.is_null() {
            unsafe { ffi::pvcam_stop_cont(self.ctx) };
        }
        self.capturing = false;
        self.sequence_remaining = None;
        Ok(())
    }

    fn is_capturing(&self) -> bool {
        self.capturing
    }

    fn set_sequence_image_sink(
        &mut self,
        sink: Option<Arc<dyn SequenceImageSink>>,
    ) -> MmResult<()> {
        self.sequence_image_sink = sink;
        Ok(())
    }

    fn sequence_images_delivered_to_sink(&self) -> bool {
        self.sequence_image_sink.is_some()
    }
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
        d.set_property("CameraName", PropertyValue::String("pvcam1".into()))
            .unwrap();
        assert_eq!(d.camera_name, "pvcam1");
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = PICAMCamera::new();
        d.set_property("Exposure", PropertyValue::Float(100.0))
            .unwrap();
        assert_eq!(d.exposure_ms, 100.0);
        assert_eq!(d.get_exposure(), 100.0);
    }

    #[test]
    fn set_gain_pre_init() {
        let mut d = PICAMCamera::new();
        d.set_property("GainIndex", PropertyValue::Integer(3))
            .unwrap();
        assert_eq!(d.gain_index, 3);
    }

    #[test]
    fn set_temp_setpoint_pre_init() {
        let mut d = PICAMCamera::new();
        d.set_property("TempSetpoint", PropertyValue::Float(-30.0))
            .unwrap();
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

    #[test]
    fn stub_open_snap_and_asymmetric_roi_binning() {
        let mut d = PICAMCamera::new();
        d.set_property("CameraName", PropertyValue::String("pvcam0".into()))
            .unwrap();
        d.initialize().unwrap();

        d.set_property("BinningX", PropertyValue::Integer(2))
            .unwrap();
        d.set_property("BinningY", PropertyValue::Integer(4))
            .unwrap();
        d.set_roi(ImageRoi::new(2, 3, 20, 8)).unwrap();
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(2, 3, 20, 8));

        d.snap_image().unwrap();
        assert_eq!(d.get_image_width(), 20);
        assert_eq!(d.get_image_height(), 8);
        assert_eq!(d.get_image_buffer().unwrap().len(), 20 * 8 * 2);
        assert_eq!(
            d.get_property("X-dimension").unwrap(),
            PropertyValue::Integer(20)
        );
        assert_eq!(
            d.get_property("Y-dimension").unwrap(),
            PropertyValue::Integer(8)
        );
    }

    #[test]
    fn stub_rejects_unsafe_roi_and_geometry_changes_while_capturing() {
        let mut d = PICAMCamera::new();
        d.set_property("CameraName", PropertyValue::String("pvcam0".into()))
            .unwrap();
        d.initialize().unwrap();

        assert_eq!(
            d.set_roi(ImageRoi::new(0, 0, 1, 1)).unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            d.start_sequence_acquisition(-1, 0.0).unwrap_err(),
            MmError::InvalidPropertyValue
        );

        d.start_sequence_acquisition(3, 0.0).unwrap();
        assert_eq!(
            d.set_property("BinningX", PropertyValue::Integer(2))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(
            d.set_roi(ImageRoi::new(0, 0, 16, 16)).unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(d.clear_roi().unwrap_err(), MmError::CameraBusyAcquiring);
        d.stop_sequence_acquisition().unwrap();
    }

    #[test]
    fn stub_clear_roi_preserves_binning() {
        let mut d = PICAMCamera::new();
        d.set_property("CameraName", PropertyValue::String("pvcam0".into()))
            .unwrap();
        d.initialize().unwrap();

        d.set_property("BinningX", PropertyValue::Integer(2))
            .unwrap();
        d.set_property("BinningY", PropertyValue::Integer(4))
            .unwrap();
        d.clear_roi().unwrap();
        assert_eq!(
            d.get_property("BinningX").unwrap(),
            PropertyValue::Integer(2)
        );
        assert_eq!(
            d.get_property("BinningY").unwrap(),
            PropertyValue::Integer(4)
        );
        assert_eq!(d.get_image_width(), 32);
        assert_eq!(d.get_image_height(), 12);
    }
}
