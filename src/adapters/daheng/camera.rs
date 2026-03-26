/// Daheng Galaxy camera adapter.
///
/// Wraps the Daheng GxIAPI C library via raw FFI bindings.
/// Exposure is in milliseconds (MicroManager convention), converted to
/// microseconds for the Daheng API.
use std::ffi::CString;
use std::ptr;

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Camera, Device};
use crate::types::{DeviceType, ImageRoi, PropertyValue};

use super::ffi;

unsafe impl Send for DahengCamera {}

// ─── Pixel format helpers ────────────────────────────────────────────────────

fn pixel_format_bpp(fmt: i64) -> u32 {
    match fmt {
        ffi::GX_PIXEL_FORMAT_MONO8 | ffi::GX_PIXEL_FORMAT_BAYER_RG8 => 1,
        ffi::GX_PIXEL_FORMAT_MONO10
        | ffi::GX_PIXEL_FORMAT_MONO12
        | ffi::GX_PIXEL_FORMAT_MONO16
        | ffi::GX_PIXEL_FORMAT_BAYER_RG10
        | ffi::GX_PIXEL_FORMAT_BAYER_RG12 => 2,
        _ => 1,
    }
}

fn pixel_format_depth(fmt: i64) -> u32 {
    match fmt {
        ffi::GX_PIXEL_FORMAT_MONO10 | ffi::GX_PIXEL_FORMAT_BAYER_RG10 => 10,
        ffi::GX_PIXEL_FORMAT_MONO12 | ffi::GX_PIXEL_FORMAT_BAYER_RG12 => 12,
        ffi::GX_PIXEL_FORMAT_MONO16 => 16,
        _ => 8,
    }
}

fn pixel_format_name(fmt: i64) -> &'static str {
    match fmt {
        ffi::GX_PIXEL_FORMAT_MONO8 => "Mono8",
        ffi::GX_PIXEL_FORMAT_MONO10 => "Mono10",
        ffi::GX_PIXEL_FORMAT_MONO12 => "Mono12",
        ffi::GX_PIXEL_FORMAT_MONO16 => "Mono16",
        ffi::GX_PIXEL_FORMAT_BAYER_RG8 => "BayerRG8",
        ffi::GX_PIXEL_FORMAT_BAYER_RG10 => "BayerRG10",
        ffi::GX_PIXEL_FORMAT_BAYER_RG12 => "BayerRG12",
        _ => "Unknown",
    }
}

fn pixel_format_from_name(name: &str) -> i64 {
    match name {
        "Mono8" => ffi::GX_PIXEL_FORMAT_MONO8,
        "Mono10" => ffi::GX_PIXEL_FORMAT_MONO10,
        "Mono12" => ffi::GX_PIXEL_FORMAT_MONO12,
        "Mono16" => ffi::GX_PIXEL_FORMAT_MONO16,
        "BayerRG8" => ffi::GX_PIXEL_FORMAT_BAYER_RG8,
        "BayerRG10" => ffi::GX_PIXEL_FORMAT_BAYER_RG10,
        "BayerRG12" => ffi::GX_PIXEL_FORMAT_BAYER_RG12,
        _ => ffi::GX_PIXEL_FORMAT_MONO8,
    }
}

// ─── Error helper ────────────────────────────────────────────────────────────

fn gx_check(status: i32, context: &str) -> MmResult<()> {
    if status == ffi::GX_STATUS_SUCCESS {
        Ok(())
    } else {
        Err(MmError::LocallyDefined(format!(
            "Daheng {}: error {}",
            context, status
        )))
    }
}

// ─── Camera struct ───────────────────────────────────────────────────────────

pub struct DahengCamera {
    props: PropertyMap,
    handle: ffi::GX_DEV_HANDLE,
    lib_initialized: bool,
    img_buf: Vec<u8>,
    width: u32,
    height: u32,
    bytes_per_pixel: u32,
    bit_depth: u32,
    pixel_format: i64,
    exposure_ms: f64,
    binning: i32,
    capturing: bool,
    serial_number: String,
}

impl DahengCamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("SerialNumber", PropertyValue::String("".into()), false)
            .unwrap();
        props
            .define_property("Exposure", PropertyValue::Float(10.0), false)
            .unwrap();
        props
            .define_property("Gain", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property(
                "PixelFormat",
                PropertyValue::String("Mono8".into()),
                false,
            )
            .unwrap();
        props
            .define_property("Binning", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .define_property("Width", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("Height", PropertyValue::Integer(0), true)
            .unwrap();

        Self {
            props,
            handle: ptr::null_mut(),
            lib_initialized: false,
            img_buf: Vec::new(),
            width: 0,
            height: 0,
            bytes_per_pixel: 1,
            bit_depth: 8,
            pixel_format: ffi::GX_PIXEL_FORMAT_MONO8,
            exposure_ms: 10.0,
            binning: 1,
            capturing: false,
            serial_number: String::new(),
        }
    }

    fn check_open(&self) -> MmResult<()> {
        if self.handle.is_null() {
            Err(MmError::NotConnected)
        } else {
            Ok(())
        }
    }

    fn write_exposure(&self, ms: f64) {
        let us = ms * 1000.0;
        unsafe {
            let _ = ffi::GXSetFloat(self.handle, ffi::GX_FLOAT_EXPOSURE_TIME, us);
        }
    }

    fn write_gain(&self, gain: f64) {
        unsafe {
            let _ = ffi::GXSetFloat(self.handle, ffi::GX_FLOAT_GAIN, gain);
        }
    }

    fn write_binning(&self, bin: i32) {
        unsafe {
            let _ = ffi::GXSetInt(self.handle, ffi::GX_INT_BINNING_HORIZONTAL, bin as i64);
            let _ = ffi::GXSetInt(self.handle, ffi::GX_INT_BINNING_VERTICAL, bin as i64);
        }
    }

    fn write_pixel_format(&self, fmt: i64) {
        unsafe {
            let _ = ffi::GXSetEnum(self.handle, ffi::GX_ENUM_PIXEL_FORMAT, fmt);
        }
    }

    fn sync_dimensions(&mut self) {
        if self.handle.is_null() {
            return;
        }
        unsafe {
            let mut val: i64 = 0;
            if ffi::GXGetInt(self.handle, ffi::GX_INT_WIDTH, &mut val) == ffi::GX_STATUS_SUCCESS {
                self.width = val as u32;
            }
            if ffi::GXGetInt(self.handle, ffi::GX_INT_HEIGHT, &mut val) == ffi::GX_STATUS_SUCCESS {
                self.height = val as u32;
            }
            let mut fmt: i64 = 0;
            if ffi::GXGetEnum(self.handle, ffi::GX_ENUM_PIXEL_FORMAT, &mut fmt)
                == ffi::GX_STATUS_SUCCESS
            {
                self.pixel_format = fmt;
                self.bytes_per_pixel = pixel_format_bpp(fmt);
                self.bit_depth = pixel_format_depth(fmt);
            }
        }
        self.props
            .entry_mut("Width")
            .map(|e| e.value = PropertyValue::Integer(self.width as i64));
        self.props
            .entry_mut("Height")
            .map(|e| e.value = PropertyValue::Integer(self.height as i64));
    }

    fn fetch_frame(&mut self) -> MmResult<()> {
        let mut frame = ffi::GxFrameData::default();
        unsafe {
            gx_check(
                ffi::GXGetImage(self.handle, &mut frame, 5000),
                "GXGetImage",
            )?;
        }
        if frame.status != ffi::GX_STATUS_SUCCESS || frame.image_buf.is_null() {
            return Err(MmError::SnapImageFailed);
        }

        let size = frame.image_size as usize;
        self.img_buf.resize(size, 0);
        unsafe {
            ptr::copy_nonoverlapping(frame.image_buf as *const u8, self.img_buf.as_mut_ptr(), size);
        }

        self.width = frame.width as u32;
        self.height = frame.height as u32;
        self.pixel_format = frame.pixel_format as i64;
        self.bytes_per_pixel = pixel_format_bpp(self.pixel_format);
        self.bit_depth = pixel_format_depth(self.pixel_format);

        Ok(())
    }
}

impl Default for DahengCamera {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for DahengCamera {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                let _ = ffi::GXStreamOff(self.handle);
                let _ = ffi::GXCloseDevice(self.handle);
            }
            self.handle = ptr::null_mut();
        }
        if self.lib_initialized {
            unsafe {
                let _ = ffi::GXCloseLib();
            }
        }
    }
}

// ─── Device trait ────────────────────────────────────────────────────────────

impl Device for DahengCamera {
    fn name(&self) -> &str {
        "DahengCamera"
    }
    fn description(&self) -> &str {
        "Daheng Galaxy camera"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.handle.is_null() {
            return Ok(());
        }

        // Initialize library
        unsafe {
            gx_check(ffi::GXInitLib(), "GXInitLib")?;
        }
        self.lib_initialized = true;

        // Enumerate devices
        let mut device_num: u32 = 0;
        unsafe {
            gx_check(
                ffi::GXUpdateDeviceList(&mut device_num, 1000),
                "GXUpdateDeviceList",
            )?;
        }
        if device_num == 0 {
            return Err(MmError::LocallyDefined(
                "No Daheng cameras found".into(),
            ));
        }

        // Open camera
        if self.serial_number.is_empty() {
            // Open first camera by index
            let index_str = CString::new("1").unwrap();
            let param = ffi::GxOpenParam {
                content: index_str.as_ptr(),
                open_mode: ffi::GX_OPEN_INDEX,
                access_mode: ffi::GX_ACCESS_EXCLUSIVE,
            };
            unsafe {
                gx_check(
                    ffi::GXOpenDevice(&param, &mut self.handle),
                    "GXOpenDevice(index)",
                )?;
            }
        } else {
            let sn = CString::new(self.serial_number.as_str())
                .map_err(|_| MmError::InvalidPropertyValue)?;
            let param = ffi::GxOpenParam {
                content: sn.as_ptr(),
                open_mode: ffi::GX_OPEN_SN,
                access_mode: ffi::GX_ACCESS_EXCLUSIVE,
            };
            unsafe {
                gx_check(
                    ffi::GXOpenDevice(&param, &mut self.handle),
                    "GXOpenDevice(SN)",
                )?;
            }
        }

        // Disable trigger (free-running) for snap_image
        unsafe {
            let _ = ffi::GXSetEnum(
                self.handle,
                ffi::GX_ENUM_TRIGGER_MODE,
                ffi::GX_TRIGGER_MODE_OFF,
            );
        }

        // Apply pre-init settings
        self.write_exposure(self.exposure_ms);
        let gain = self
            .props
            .get("Gain")
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        self.write_gain(gain);
        self.write_binning(self.binning);
        self.write_pixel_format(self.pixel_format);

        // Clear ROI to full sensor
        self.clear_roi().ok();
        self.sync_dimensions();

        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.capturing {
            self.stop_sequence_acquisition()?;
        }
        if !self.handle.is_null() {
            unsafe {
                let _ = ffi::GXCloseDevice(self.handle);
            }
            self.handle = ptr::null_mut();
        }
        if self.lib_initialized {
            unsafe {
                let _ = ffi::GXCloseLib();
            }
            self.lib_initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Exposure" => Ok(PropertyValue::Float(self.exposure_ms)),
            "Gain" => {
                if !self.handle.is_null() {
                    let mut val: f64 = 0.0;
                    unsafe {
                        if ffi::GXGetFloat(self.handle, ffi::GX_FLOAT_GAIN, &mut val)
                            == ffi::GX_STATUS_SUCCESS
                        {
                            return Ok(PropertyValue::Float(val));
                        }
                    }
                }
                self.props.get(name).cloned()
            }
            "PixelFormat" => Ok(PropertyValue::String(
                pixel_format_name(self.pixel_format).to_string(),
            )),
            "Binning" => Ok(PropertyValue::Integer(self.binning as i64)),
            "SerialNumber" => Ok(PropertyValue::String(self.serial_number.clone())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "SerialNumber" => {
                if !self.handle.is_null() {
                    return Err(MmError::LocallyDefined(
                        "SerialNumber cannot be changed after initialize()".into(),
                    ));
                }
                self.serial_number = val.as_str().to_string();
                self.props.set(name, val)
            }
            "Exposure" => {
                self.exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.exposure_ms))?;
                if !self.handle.is_null() {
                    self.write_exposure(self.exposure_ms);
                }
                Ok(())
            }
            "Gain" => {
                let g = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(g))?;
                if !self.handle.is_null() {
                    self.write_gain(g);
                }
                Ok(())
            }
            "PixelFormat" => {
                let fmt_name = val.as_str().to_string();
                self.pixel_format = pixel_format_from_name(&fmt_name);
                self.props.set(name, val)?;
                if !self.handle.is_null() {
                    self.write_pixel_format(self.pixel_format);
                }
                self.sync_dimensions();
                Ok(())
            }
            "Binning" => {
                self.binning = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props
                    .set(name, PropertyValue::Integer(self.binning as i64))?;
                if !self.handle.is_null() {
                    self.write_binning(self.binning);
                }
                self.clear_roi().ok();
                self.sync_dimensions();
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
        self.props
            .entry(name)
            .map(|e| e.read_only)
            .unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Camera
    }
    fn busy(&self) -> bool {
        false
    }
}

// ─── Camera trait ────────────────────────────────────────────────────────────

impl Camera for DahengCamera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_open()?;
        if self.capturing {
            return self.fetch_frame();
        }
        // Single shot: enable software trigger, stream on, trigger, get image, stream off
        unsafe {
            ffi::GXSetEnum(
                self.handle,
                ffi::GX_ENUM_TRIGGER_MODE,
                ffi::GX_TRIGGER_MODE_ON,
            );
            ffi::GXSetEnum(
                self.handle,
                ffi::GX_ENUM_TRIGGER_SOURCE,
                ffi::GX_TRIGGER_SOURCE_SOFTWARE,
            );
            gx_check(ffi::GXStreamOn(self.handle), "GXStreamOn")?;
            ffi::GXSendCommand(self.handle, ffi::GX_COMMAND_TRIGGER_SOFTWARE);
        }
        let result = self.fetch_frame();
        unsafe {
            let _ = ffi::GXStreamOff(self.handle);
            // Restore free-running mode
            ffi::GXSetEnum(
                self.handle,
                ffi::GX_ENUM_TRIGGER_MODE,
                ffi::GX_TRIGGER_MODE_OFF,
            );
        }
        result
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if self.img_buf.is_empty() {
            Err(MmError::LocallyDefined("No image captured yet".into()))
        } else {
            Ok(&self.img_buf)
        }
    }

    fn get_image_width(&self) -> u32 {
        self.width
    }
    fn get_image_height(&self) -> u32 {
        self.height
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

    fn set_exposure(&mut self, exp_ms: f64) {
        self.exposure_ms = exp_ms;
        self.props
            .set("Exposure", PropertyValue::Float(exp_ms))
            .ok();
        if !self.handle.is_null() {
            self.write_exposure(exp_ms);
        }
    }

    fn get_binning(&self) -> i32 {
        self.binning
    }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        self.binning = bin;
        self.props
            .set("Binning", PropertyValue::Integer(bin as i64))?;
        if !self.handle.is_null() {
            self.write_binning(bin);
        }
        self.clear_roi().ok();
        self.sync_dimensions();
        Ok(())
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        if self.handle.is_null() {
            return Ok(ImageRoi::new(0, 0, self.width, self.height));
        }
        unsafe {
            let (mut x, mut y, mut w, mut h) = (0i64, 0i64, 0i64, 0i64);
            ffi::GXGetInt(self.handle, ffi::GX_INT_OFFSET_X, &mut x);
            ffi::GXGetInt(self.handle, ffi::GX_INT_OFFSET_Y, &mut y);
            ffi::GXGetInt(self.handle, ffi::GX_INT_WIDTH, &mut w);
            ffi::GXGetInt(self.handle, ffi::GX_INT_HEIGHT, &mut h);
            Ok(ImageRoi::new(x as u32, y as u32, w as u32, h as u32))
        }
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        self.check_open()?;
        unsafe {
            // Width/Height before OffsetX/Y
            let _ = ffi::GXSetInt(self.handle, ffi::GX_INT_WIDTH, roi.width as i64);
            let _ = ffi::GXSetInt(self.handle, ffi::GX_INT_HEIGHT, roi.height as i64);
            let _ = ffi::GXSetInt(self.handle, ffi::GX_INT_OFFSET_X, roi.x as i64);
            let _ = ffi::GXSetInt(self.handle, ffi::GX_INT_OFFSET_Y, roi.y as i64);
        }
        self.sync_dimensions();
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        if self.handle.is_null() {
            return Ok(());
        }
        unsafe {
            let _ = ffi::GXSetInt(self.handle, ffi::GX_INT_OFFSET_X, 0);
            let _ = ffi::GXSetInt(self.handle, ffi::GX_INT_OFFSET_Y, 0);
            let mut max_w: i64 = 0;
            let mut max_h: i64 = 0;
            ffi::GXGetInt(self.handle, ffi::GX_INT_WIDTH_MAX, &mut max_w);
            ffi::GXGetInt(self.handle, ffi::GX_INT_HEIGHT_MAX, &mut max_h);
            let _ = ffi::GXSetInt(self.handle, ffi::GX_INT_WIDTH, max_w);
            let _ = ffi::GXSetInt(self.handle, ffi::GX_INT_HEIGHT, max_h);
        }
        self.sync_dimensions();
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, _count: i64, _interval_ms: f64) -> MmResult<()> {
        self.check_open()?;
        if self.capturing {
            return Ok(());
        }
        // Free-running continuous mode
        unsafe {
            ffi::GXSetEnum(
                self.handle,
                ffi::GX_ENUM_TRIGGER_MODE,
                ffi::GX_TRIGGER_MODE_OFF,
            );
            gx_check(ffi::GXStreamOn(self.handle), "GXStreamOn")?;
        }
        self.capturing = true;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        if !self.capturing {
            return Ok(());
        }
        if !self.handle.is_null() {
            unsafe {
                let _ = ffi::GXStreamOff(self.handle);
            }
        }
        self.capturing = false;
        Ok(())
    }

    fn is_capturing(&self) -> bool {
        self.capturing
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_format_helpers() {
        assert_eq!(pixel_format_bpp(ffi::GX_PIXEL_FORMAT_MONO8), 1);
        assert_eq!(pixel_format_bpp(ffi::GX_PIXEL_FORMAT_MONO16), 2);
        assert_eq!(pixel_format_bpp(ffi::GX_PIXEL_FORMAT_BAYER_RG12), 2);

        assert_eq!(pixel_format_depth(ffi::GX_PIXEL_FORMAT_MONO8), 8);
        assert_eq!(pixel_format_depth(ffi::GX_PIXEL_FORMAT_MONO12), 12);
        assert_eq!(pixel_format_depth(ffi::GX_PIXEL_FORMAT_MONO16), 16);

        assert_eq!(pixel_format_name(ffi::GX_PIXEL_FORMAT_MONO8), "Mono8");
        assert_eq!(pixel_format_name(ffi::GX_PIXEL_FORMAT_BAYER_RG8), "BayerRG8");

        assert_eq!(pixel_format_from_name("Mono8"), ffi::GX_PIXEL_FORMAT_MONO8);
        assert_eq!(
            pixel_format_from_name("Mono16"),
            ffi::GX_PIXEL_FORMAT_MONO16
        );
    }

    #[test]
    fn default_properties() {
        let d = DahengCamera::new();
        assert_eq!(d.device_type(), DeviceType::Camera);
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(d.get_binning(), 1);
        assert!(!d.is_capturing());
    }

    #[test]
    fn set_serial_number_pre_init() {
        let mut d = DahengCamera::new();
        d.set_property("SerialNumber", PropertyValue::String("ABC123".into()))
            .unwrap();
        assert_eq!(d.serial_number, "ABC123");
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = DahengCamera::new();
        d.set_property("Exposure", PropertyValue::Float(25.0))
            .unwrap();
        assert_eq!(d.exposure_ms, 25.0);
    }

    #[test]
    fn no_image_before_snap() {
        let d = DahengCamera::new();
        assert!(d.get_image_buffer().is_err());
    }

    #[test]
    fn snap_without_init_errors() {
        let mut d = DahengCamera::new();
        assert!(d.snap_image().is_err());
    }
}
