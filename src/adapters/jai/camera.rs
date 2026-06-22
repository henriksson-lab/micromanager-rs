use std::ffi::{CStr, CString};
use std::sync::Arc;

use crate::circular_buffer::ImageFrame;
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Camera, Device, SequenceImageSink};
use crate::types::{DeviceType, ImageRoi, PropertyValue};

use super::ffi;

// SAFETY: JAICamera holds raw pointers into eBUS SDK objects.
// The eBUS SDK is thread-safe for separate camera handles; we guarantee that
// only one Rust thread accesses each JAICamera at a time by requiring `&mut
// self` on all mutating methods.
unsafe impl Send for JAICamera {}

// ── String helpers ────────────────────────────────────────────────────────────

const BUF: usize = 256;

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

// ── Pixel format helpers ──────────────────────────────────────────────────────

fn bpp_to_bytes(bpp: u32) -> u32 {
    (bpp + 7) / 8
}

fn pixel_type_to_format(pixel_type: &str) -> Option<&'static str> {
    match pixel_type {
        "8bit" => Some("Mono8"),
        "10bit" => Some("Mono10"),
        "12bit" => Some("Mono12"),
        "16bit" => Some("Mono16"),
        "32bitRGB" => Some("BGR8"),
        "64bitRGB-10bit" => Some("BGR10p"),
        "64bitRGB-12bit" => Some("BGR12p"),
        _ => None,
    }
}

fn pixel_format_to_pixel_type(format: &str) -> &'static str {
    match format {
        "Mono10" | "Mono10p" => "10bit",
        "Mono12" | "Mono12p" => "12bit",
        "Mono16" => "16bit",
        "BGR8" => "32bitRGB",
        "BGR10p" => "64bitRGB-10bit",
        "BGR12p" => "64bitRGB-12bit",
        _ => "8bit",
    }
}

fn bgr8_to_bgra32(src: &[u8], width: u32, height: u32, padding_x: u32) -> Vec<u8> {
    let npix = (width as usize).saturating_mul(height as usize);
    let mut dst = vec![0u8; npix.saturating_mul(4)];
    let src_row_bytes = (width as usize)
        .saturating_mul(3)
        .saturating_add(padding_x as usize);
    for row in 0..height as usize {
        let start = row.saturating_mul(src_row_bytes);
        let end = src.len().min(start.saturating_add(width as usize * 3));
        let row_src = src.get(start..end).unwrap_or(&[]);
        for (col, bgr) in row_src.chunks_exact(3).take(width as usize).enumerate() {
            let out = &mut dst[(row * width as usize + col) * 4..][..4];
            out[0] = bgr[0];
            out[1] = bgr[1];
            out[2] = bgr[2];
            out[3] = 0;
        }
    }
    dst
}

fn read_u16_window(src: &[u8], byte_index: usize) -> u16 {
    let lo = src.get(byte_index).copied().unwrap_or_default() as u16;
    let hi = src.get(byte_index + 1).copied().unwrap_or_default() as u16;
    lo | (hi << 8)
}

fn packed_bgrp_to_bgra64(
    src: &[u8],
    width: u32,
    height: u32,
    bits_per_component: u32,
    padding_x: u32,
) -> Vec<u8> {
    let bits_per_pixel = 3usize.saturating_mul(bits_per_component as usize);
    let bytes_per_row = (width as usize)
        .saturating_mul(bits_per_pixel)
        .saturating_add(7)
        / 8;
    let padded_bytes_per_row = bytes_per_row.saturating_add(padding_x as usize);
    let npix = (width as usize).saturating_mul(height as usize);
    let mut dst = vec![0u8; npix.saturating_mul(8)];
    let mask = ((1u32 << bits_per_component) - 1) as u16;

    for row in 0..height as usize {
        let start = row.saturating_mul(padded_bytes_per_row);
        let end = src.len().min(start.saturating_add(bytes_per_row));
        let row_src = src.get(start..end).unwrap_or(&[]);
        for col in 0..width as usize {
            let base_bit = col.saturating_mul(bits_per_pixel);
            let out = &mut dst[(row * width as usize + col) * 8..][..8];
            for component in 0..3usize {
                let bit = base_bit + component * bits_per_component as usize;
                let byte = bit / 8;
                let shift = bit % 8;
                let value = (read_u16_window(row_src, byte) >> shift) & mask;
                out[component * 2..component * 2 + 2].copy_from_slice(&value.to_le_bytes());
            }
            out[6..8].copy_from_slice(&0u16.to_le_bytes());
        }
    }
    dst
}

// ── Camera struct ─────────────────────────────────────────────────────────────

/// Sequence state: one stream + N pre-allocated PvBuffers for continuous grab.
struct SequenceState {
    stream: *mut ffi::JaiStream,
    buffers: Vec<*mut ffi::JaiBuffer>,
}

pub struct JAICamera {
    props: PropertyMap,

    // Raw SDK handles (null when not initialized).
    system: *mut ffi::JaiSystem,
    device: *mut ffi::JaiDevice,
    seq: Option<SequenceState>,

    // Cached image data from the last snap.
    image_buf: Vec<u8>,
    width: u32,
    height: u32,
    bytes_per_pixel: u32,
    bit_depth: u32,
    num_components: u32,
    sequence_image_sink: Option<Arc<dyn SequenceImageSink>>,

    // Pre-init settings.
    camera_index: i32,
    serial_number: String,
    exposure_ms: f64,
    gain: f64,
    pixel_format: String,
    pixel_type: String,
    binning: i32,
    frame_rate: f64,
    gamma: f64,
    black_level: f64,
    test_pattern: String,
    white_balance: String,
    common_exposure_selector: Option<String>,
    individual_exposure_selectors: Vec<String>,
    common_gain_selector: Option<String>,
    individual_gain_selectors: Vec<String>,
    black_level_selectors: Vec<String>,
}

impl JAICamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("CameraID", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("CameraIndex", PropertyValue::Integer(0), false)
            .unwrap();
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
            .define_property("PixelFormat", PropertyValue::String("BGR8".into()), false)
            .unwrap();
        props
            .define_property("PixelType", PropertyValue::String("32bitRGB".into()), false)
            .unwrap();
        props
            .set_allowed_values(
                "PixelType",
                &["32bitRGB", "64bitRGB-10bit", "64bitRGB-12bit"],
            )
            .unwrap();
        props
            .define_property("Binning", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .define_property("FrameRate", PropertyValue::Float(30.0), false)
            .unwrap();
        props
            .define_property("Gamma", PropertyValue::Float(1.0), false)
            .unwrap();
        props
            .define_property("BlackLevel", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("TestPattern", PropertyValue::String("Off".into()), false)
            .unwrap();
        props
            .define_property("WhiteBalance", PropertyValue::String("Off".into()), false)
            .unwrap();
        props
            .define_property("Width", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("Height", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("Temperature", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("Model", PropertyValue::String("".into()), true)
            .unwrap();

        Self {
            props,
            system: std::ptr::null_mut(),
            device: std::ptr::null_mut(),
            seq: None,
            image_buf: Vec::new(),
            width: 0,
            height: 0,
            bytes_per_pixel: 1,
            bit_depth: 8,
            num_components: 1,
            sequence_image_sink: None,
            camera_index: 0,
            serial_number: String::new(),
            exposure_ms: 10.0,
            gain: 0.0,
            pixel_format: "BGR8".into(),
            pixel_type: "32bitRGB".into(),
            binning: 1,
            frame_rate: 30.0,
            gamma: 1.0,
            black_level: 0.0,
            test_pattern: "Off".into(),
            white_balance: "Off".into(),
            common_exposure_selector: None,
            individual_exposure_selectors: Vec::new(),
            common_gain_selector: None,
            individual_gain_selectors: Vec::new(),
            black_level_selectors: Vec::new(),
        }
    }

    fn check_open(&self) -> MmResult<()> {
        if self.device.is_null() {
            Err(MmError::NotConnected)
        } else {
            Ok(())
        }
    }

    // ── Device parameter helpers ───────────────────────────────────────────────

    fn dev_set_float(&self, name: &str, v: f64) {
        if self.device.is_null() {
            return;
        }
        let n = cstr(name);
        unsafe {
            ffi::jai_device_set_float(self.device, n.as_ptr(), v);
        }
    }

    fn dev_set_int(&self, name: &str, v: i64) {
        if self.device.is_null() {
            return;
        }
        let n = cstr(name);
        unsafe {
            ffi::jai_device_set_int(self.device, n.as_ptr(), v);
        }
    }

    fn dev_set_enum(&self, name: &str, v: &str) {
        if self.device.is_null() {
            return;
        }
        let n = cstr(name);
        let val = cstr(v);
        unsafe {
            ffi::jai_device_set_enum(self.device, n.as_ptr(), val.as_ptr());
        }
    }

    fn dev_execute(&self, name: &str) {
        if self.device.is_null() {
            return;
        }
        let n = cstr(name);
        unsafe {
            ffi::jai_device_execute(self.device, n.as_ptr());
        }
    }

    fn dev_get_float(&self, name: &str) -> Option<f64> {
        if self.device.is_null() {
            return None;
        }
        let n = cstr(name);
        let mut v: f64 = 0.0;
        let rc = unsafe { ffi::jai_device_get_float(self.device, n.as_ptr(), &mut v) };
        if rc == 0 {
            Some(v)
        } else {
            None
        }
    }

    fn dev_get_int(&self, name: &str) -> Option<i64> {
        if self.device.is_null() {
            return None;
        }
        let n = cstr(name);
        let mut v: i64 = 0;
        let rc = unsafe { ffi::jai_device_get_int(self.device, n.as_ptr(), &mut v) };
        if rc == 0 {
            Some(v)
        } else {
            None
        }
    }

    fn dev_get_int_increment(&self, name: &str) -> i64 {
        if self.device.is_null() {
            return 1;
        }
        let n = cstr(name);
        let mut v: i64 = 1;
        let rc = unsafe { ffi::jai_device_get_int_increment(self.device, n.as_ptr(), &mut v) };
        if rc == 0 && v > 0 {
            v
        } else {
            1
        }
    }

    fn dev_get_string(&self, name: &str) -> Option<String> {
        if self.device.is_null() {
            return None;
        }
        let n = cstr(name);
        let mut buf = [0i8; BUF];
        let rc = unsafe {
            ffi::jai_device_get_string(self.device, n.as_ptr(), buf.as_mut_ptr(), BUF as i32)
        };
        if rc != 0 {
            return None;
        }
        let s = unsafe { CStr::from_ptr(buf.as_ptr()) };
        Some(s.to_string_lossy().into_owned())
    }

    fn dev_get_enum(&self, name: &str) -> Option<String> {
        if self.device.is_null() {
            return None;
        }
        let n = cstr(name);
        let mut buf = [0i8; BUF];
        let rc = unsafe {
            ffi::jai_device_get_enum(self.device, n.as_ptr(), buf.as_mut_ptr(), BUF as i32)
        };
        if rc != 0 {
            return None;
        }
        let s = unsafe { CStr::from_ptr(buf.as_ptr()) };
        Some(s.to_string_lossy().into_owned())
    }

    fn dev_get_enum_entries(&self, name: &str) -> Vec<String> {
        if self.device.is_null() {
            return Vec::new();
        }
        let n = cstr(name);
        let mut buf = [0i8; 2048];
        let rc = unsafe {
            ffi::jai_device_get_enum_entries(
                self.device,
                n.as_ptr(),
                buf.as_mut_ptr(),
                buf.len() as i32,
            )
        };
        if rc != 0 {
            return Vec::new();
        }
        let s = unsafe { CStr::from_ptr(buf.as_ptr()) };
        s.to_string_lossy()
            .split(';')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    }

    fn define_property_if_missing(
        &mut self,
        name: &str,
        value: PropertyValue,
        read_only: bool,
    ) -> MmResult<()> {
        if !self.props.has_property(name) {
            self.props.define_property(name, value, read_only)?;
        }
        Ok(())
    }

    fn set_allowed_values_from_strings(&mut self, name: &str, values: &[String]) -> MmResult<()> {
        let refs = values.iter().map(String::as_str).collect::<Vec<_>>();
        self.props.set_allowed_values(name, &refs)
    }

    fn get_selector_float(
        &self,
        selector_node: &str,
        selector: &str,
        restore_selector: Option<&str>,
        value_node: &str,
        scale: f64,
    ) -> Option<f64> {
        self.dev_set_enum(selector_node, selector);
        let value = self.dev_get_float(value_node).map(|v| v * scale);
        if let Some(restore_selector) = restore_selector {
            self.dev_set_enum(selector_node, restore_selector);
        }
        value
    }

    fn set_selector_float(
        &self,
        selector_node: &str,
        selector: &str,
        restore_selector: Option<&str>,
        value_node: &str,
        value: f64,
        inverse_scale: f64,
    ) {
        self.dev_set_enum(selector_node, selector);
        self.dev_set_float(value_node, value * inverse_scale);
        if let Some(restore_selector) = restore_selector {
            self.dev_set_enum(selector_node, restore_selector);
        }
    }

    fn sync_dynamic_properties(&mut self) -> MmResult<()> {
        let pixel_types = self
            .dev_get_enum_entries("PixelFormat")
            .into_iter()
            .filter_map(|format| match format.as_str() {
                "BGR8" | "BGR10p" | "BGR12p" => Some(pixel_format_to_pixel_type(&format).into()),
                _ => None,
            })
            .collect::<Vec<String>>();
        if !pixel_types.is_empty() {
            self.set_allowed_values_from_strings("PixelType", &pixel_types)?;
            if !pixel_types.iter().any(|v| v == &self.pixel_type) {
                self.pixel_type = pixel_types[0].clone();
                if let Some(format) = pixel_type_to_format(&self.pixel_type) {
                    self.pixel_format = format.to_string();
                    self.dev_set_enum("PixelFormat", &self.pixel_format);
                }
                self.props
                    .set("PixelType", PropertyValue::String(self.pixel_type.clone()))?;
                self.props.set(
                    "PixelFormat",
                    PropertyValue::String(self.pixel_format.clone()),
                )?;
            }
        }

        let exposure_selectors = self.dev_get_enum_entries("ExposureTimeSelector");
        self.common_exposure_selector = exposure_selectors
            .iter()
            .find(|selector| selector.as_str() == "Common")
            .cloned();
        self.individual_exposure_selectors = exposure_selectors
            .iter()
            .filter(|selector| Some(selector.as_str()) != self.common_exposure_selector.as_deref())
            .cloned()
            .collect();
        if self.common_exposure_selector.is_some() && !self.individual_exposure_selectors.is_empty()
        {
            let mode = match self.dev_get_enum("ExposureTimeMode").as_deref() {
                Some("Individual") => "On",
                _ => "Off",
            };
            self.define_property_if_missing(
                "ExposureIsIndividual",
                PropertyValue::String(mode.into()),
                false,
            )?;
            self.props
                .set_allowed_values("ExposureIsIndividual", &["Off", "On"])?;
            self.props
                .set("ExposureIsIndividual", PropertyValue::String(mode.into()))?;
            for selector in self.individual_exposure_selectors.clone() {
                let name = format!("Exposure_{selector}");
                let value = self
                    .get_selector_float(
                        "ExposureTimeSelector",
                        &selector,
                        self.common_exposure_selector.as_deref(),
                        "ExposureTime",
                        0.001,
                    )
                    .unwrap_or(self.exposure_ms);
                self.define_property_if_missing(&name, PropertyValue::Float(value), false)?;
                self.props.set(&name, PropertyValue::Float(value))?;
            }
        }

        let gain_selectors = self.dev_get_enum_entries("GainSelector");
        self.common_gain_selector = gain_selectors
            .iter()
            .find(|selector| selector.as_str() == "AnalogAll")
            .cloned();
        self.individual_gain_selectors = gain_selectors
            .iter()
            .filter(|selector| Some(selector.as_str()) != self.common_gain_selector.as_deref())
            .cloned()
            .collect();
        if self.common_gain_selector.is_some() && !self.individual_gain_selectors.is_empty() {
            let mode = match self.dev_get_enum("IndividualGainMode").as_deref() {
                Some("On") => "On",
                _ => "Off",
            };
            self.define_property_if_missing(
                "GainIsIndividual",
                PropertyValue::String(mode.into()),
                false,
            )?;
            self.props
                .set_allowed_values("GainIsIndividual", &["Off", "On"])?;
            self.props
                .set("GainIsIndividual", PropertyValue::String(mode.into()))?;
            for selector in self.individual_gain_selectors.clone() {
                let name = format!("Gain_{selector}");
                let value = self
                    .get_selector_float(
                        "GainSelector",
                        &selector,
                        self.common_gain_selector.as_deref(),
                        "Gain",
                        1.0,
                    )
                    .unwrap_or(self.gain);
                self.define_property_if_missing(&name, PropertyValue::Float(value), false)?;
                self.props.set(&name, PropertyValue::Float(value))?;
            }
        }

        self.black_level_selectors = self.dev_get_enum_entries("BlackLevelSelector");
        for selector in self.black_level_selectors.clone() {
            let name = format!("BlackLevel_{selector}");
            let value = self
                .get_selector_float("BlackLevelSelector", &selector, None, "BlackLevel", 1.0)
                .unwrap_or(self.black_level);
            self.define_property_if_missing(&name, PropertyValue::Float(value), false)?;
            self.props.set(&name, PropertyValue::Float(value))?;
        }

        Ok(())
    }

    // ── Sync dimensions from camera ───────────────────────────────────────────

    fn sync_dimensions(&mut self) {
        if let Some(w) = self.dev_get_int("Width") {
            self.width = w as u32;
        }
        if let Some(h) = self.dev_get_int("Height") {
            self.height = h as u32;
        }
        if let Some(fmt) = self.dev_get_enum("PixelFormat") {
            self.pixel_format = fmt;
        }
        self.pixel_type = pixel_format_to_pixel_type(&self.pixel_format).to_string();
        self.bytes_per_pixel =
            bpp_to_bytes(unsafe { ffi::jai_buffer_bits_per_pixel(std::ptr::null_mut()) }.max(8));
        // Fallback: use bits-per-pixel from current format string
        self.bytes_per_pixel = if self.pixel_format == "BGR8" {
            4
        } else if self.pixel_format == "BGR10p" || self.pixel_format == "BGR12p" {
            8
        } else if self.pixel_format.contains("16") {
            2
        } else if self.pixel_format.contains("10") || self.pixel_format.contains("12") {
            2
        } else {
            1
        };
        self.bit_depth = if self.pixel_format.contains("12") {
            12
        } else if self.pixel_format.contains("10") {
            10
        } else if self.pixel_format.contains("16") {
            16
        } else {
            8
        };
        self.num_components =
            if self.pixel_format.contains("RGB") || self.pixel_format.contains("BGR") {
                4
            } else {
                1
            };
        self.props
            .entry_mut("Width")
            .map(|e| e.value = PropertyValue::Integer(self.width as i64));
        self.props
            .entry_mut("Height")
            .map(|e| e.value = PropertyValue::Integer(self.height as i64));
        self.props
            .entry_mut("PixelFormat")
            .map(|e| e.value = PropertyValue::String(self.pixel_format.clone()));
        self.props
            .entry_mut("PixelType")
            .map(|e| e.value = PropertyValue::String(self.pixel_type.clone()));
    }

    // ── Apply pre-init settings to open device ────────────────────────────────

    fn apply_settings(&mut self) {
        let ms = self.exposure_ms;
        let g = self.gain;
        let bin = self.binning;
        let fmt = self.pixel_format.clone();

        // Reset to factory defaults first, matching the upstream initialization
        // order.  Cached pre-init settings are applied after this point.
        self.dev_set_int("UserSetSelector", 0);
        self.dev_execute("UserSetLoad");
        self.dev_set_enum("ExposureMode", "Timed");
        self.dev_set_enum("MultiRoiMode", "Off");

        // Exposure: ExposureTimeAbs in µs (most JAI cameras use this node).
        self.dev_set_float("ExposureTimeAbs", ms * 1_000.0);
        // Try the GenICam standard node name as well.
        self.dev_set_float("ExposureTime", ms * 1_000.0);

        // Gain
        if self.dev_set_float_check("Gain", g).is_err() {
            self.dev_set_int("GainRaw", g as i64);
        }

        // Binning (symmetric)
        self.dev_set_int("BinningHorizontal", bin as i64);
        self.dev_set_int("BinningVertical", bin as i64);

        // Pixel format
        self.dev_set_enum("PixelFormat", &fmt);

        self.dev_set_float("AcquisitionFrameRate", self.frame_rate);
        self.dev_set_float("Gamma", self.gamma);
        self.dev_set_float("BlackLevel", self.black_level);
        self.dev_set_enum("TestPattern", &self.test_pattern);
        self.dev_set_enum("BalanceWhiteAuto", &self.white_balance);
    }

    fn dev_set_float_check(&self, name: &str, v: f64) -> MmResult<()> {
        if self.device.is_null() {
            return Err(MmError::NotConnected);
        }
        let n = cstr(name);
        let rc = unsafe { ffi::jai_device_set_float(self.device, n.as_ptr(), v) };
        if rc == 0 {
            Ok(())
        } else {
            Err(MmError::Err)
        }
    }

    fn ensure_not_capturing_for_property(&self) -> MmResult<()> {
        if self.seq.is_some() {
            Err(MmError::LocallyDefined(
                "This operation is not allowed during live streaming".into(),
            ))
        } else {
            Ok(())
        }
    }

    // ── Single-frame grab ─────────────────────────────────────────────────────

    fn snap_one_frame(&mut self) -> MmResult<()> {
        // 1. Set single-frame mode.
        self.dev_set_enum("AcquisitionMode", "SingleFrame");

        // 2. Get connection ID to open the matching stream.
        let mut conn_buf = [0i8; BUF];
        let rc = unsafe {
            ffi::jai_device_get_connection_id(self.device, conn_buf.as_mut_ptr(), BUF as i32)
        };
        if rc != 0 {
            return Err(MmError::NotConnected);
        }
        let conn = unsafe { CStr::from_ptr(conn_buf.as_ptr()) };
        let conn_cstr = CString::new(conn.to_bytes()).map_err(|_| MmError::Err)?;

        // 3. Open stream.
        let stream = unsafe { ffi::jai_stream_open(conn_cstr.as_ptr()) };
        if stream.is_null() {
            return Err(MmError::LocallyDefined("JAI: failed to open stream".into()));
        }

        let result = (|| {
            // 4. Allocate one buffer.
            let payload = unsafe { ffi::jai_device_payload_size(self.device) };
            let buf = unsafe { ffi::jai_buffer_alloc(payload) };
            if buf.is_null() {
                return Err(MmError::LocallyDefined("JAI: buffer alloc failed".into()));
            }

            // 5. Queue buffer + start acquisition.
            unsafe { ffi::jai_stream_queue(stream, buf) };
            unsafe { ffi::jai_device_stream_enable(self.device) };
            self.dev_execute("AcquisitionStart");

            // 6. Wait for frame (4 second timeout).
            let grabbed = unsafe { ffi::jai_stream_retrieve(stream, 4000) };

            // 7. Stop acquisition.
            self.dev_execute("AcquisitionStop");
            unsafe { ffi::jai_device_stream_disable(self.device) };

            if grabbed.is_null() {
                unsafe { ffi::jai_buffer_free(buf) };
                return Err(MmError::SnapImageFailed);
            }

            // 8. Copy pixel data.
            self.copy_from_buffer(grabbed);

            // 9. Release (grabbed is non-owned; free the wrapper but not the PvBuffer
            //    since stream is about to close).
            unsafe { ffi::jai_buffer_free(grabbed) };
            unsafe { ffi::jai_buffer_free(buf) };
            Ok(())
        })();

        unsafe { ffi::jai_stream_free(stream) };
        result
    }

    /// Copy pixel data from a retrieved buffer into `self.image_buf`.
    fn copy_from_buffer(&mut self, buf: *mut ffi::JaiBuffer) {
        let size = unsafe { ffi::jai_buffer_data_size(buf) } as usize;
        let data = unsafe { ffi::jai_buffer_data(buf) };
        if data.is_null() || size == 0 {
            return;
        }

        self.width = unsafe { ffi::jai_buffer_width(buf) };
        self.height = unsafe { ffi::jai_buffer_height(buf) };
        let bpp = unsafe { ffi::jai_buffer_bits_per_pixel(buf) };
        let bpc = unsafe { ffi::jai_buffer_bits_per_component(buf) };
        let padding_x = unsafe { ffi::jai_buffer_padding_x(buf) };
        let is_color = unsafe { ffi::jai_buffer_is_color(buf) } != 0;
        let raw = unsafe { std::slice::from_raw_parts(data, size) };
        if self.pixel_format == "BGR8" || (is_color && bpp == 24 && bpc == 8) {
            self.image_buf = bgr8_to_bgra32(raw, self.width, self.height, padding_x);
            self.bytes_per_pixel = 4;
            self.bit_depth = 8;
            self.num_components = 4;
        } else if self.pixel_format == "BGR10p" || (is_color && bpc == 10) {
            self.image_buf = packed_bgrp_to_bgra64(raw, self.width, self.height, 10, padding_x);
            self.bytes_per_pixel = 8;
            self.bit_depth = 10;
            self.num_components = 4;
        } else if self.pixel_format == "BGR12p" || (is_color && bpc == 12) {
            self.image_buf = packed_bgrp_to_bgra64(raw, self.width, self.height, 12, padding_x);
            self.bytes_per_pixel = 8;
            self.bit_depth = 12;
            self.num_components = 4;
        } else {
            self.bytes_per_pixel = bpp_to_bytes(bpp);
            self.bit_depth = bpc;
            self.num_components = if is_color { 3 } else { 1 };
            self.image_buf.clear();
            self.image_buf.extend_from_slice(raw);
        }

        self.props
            .entry_mut("Width")
            .map(|e| e.value = PropertyValue::Integer(self.width as i64));
        self.props
            .entry_mut("Height")
            .map(|e| e.value = PropertyValue::Integer(self.height as i64));
    }

    // ── Sequence: dequeue one frame from the continuous stream ────────────────

    fn snap_from_sequence(&mut self) -> MmResult<()> {
        let seq = self.seq.as_ref().ok_or(MmError::NotConnected)?;
        let grabbed = unsafe { ffi::jai_stream_retrieve(seq.stream, 4000) };
        if grabbed.is_null() {
            return Err(MmError::SnapImageFailed);
        }
        self.copy_from_buffer(grabbed);
        // Re-queue the underlying buffer for reuse.
        let seq = self.seq.as_ref().unwrap();
        unsafe { ffi::jai_stream_requeue(seq.stream, grabbed) };
        // Free the non-owning wrapper.
        unsafe { ffi::jai_buffer_free(grabbed) };
        self.emit_sequence_frame_to_sink()?;
        Ok(())
    }

    fn emit_sequence_frame_to_sink(&mut self) -> MmResult<()> {
        if let Some(sink) = &self.sequence_image_sink {
            if sink.insert_sequence_image(ImageFrame::new(
                self.image_buf.clone(),
                self.width,
                self.height,
                self.bytes_per_pixel,
            )) {
                self.stop_sequence_acquisition()?;
                return Err(MmError::BufferOverflow);
            }
        }
        Ok(())
    }
}

impl Default for JAICamera {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for JAICamera {
    fn drop(&mut self) {
        // Stop any active sequence.
        let _ = self.stop_sequence_acquisition();
        // Disconnect device.
        if !self.device.is_null() {
            unsafe { ffi::jai_device_free(self.device) };
            self.device = std::ptr::null_mut();
        }
        // Free system.
        if !self.system.is_null() {
            unsafe { ffi::jai_system_free(self.system) };
            self.system = std::ptr::null_mut();
        }
    }
}

// ── Device trait ──────────────────────────────────────────────────────────────

impl Device for JAICamera {
    fn name(&self) -> &str {
        "JAICamera"
    }
    fn description(&self) -> &str {
        "JAI camera"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.device.is_null() {
            return Ok(());
        }

        // Create system + find cameras.
        let sys = unsafe { ffi::jai_system_new() };
        if sys.is_null() {
            return Err(MmError::LocallyDefined(
                "JAI: failed to create PvSystem".into(),
            ));
        }
        self.system = sys;

        let count = unsafe { ffi::jai_system_find(sys) };
        if count < 0 {
            return Err(MmError::LocallyDefined(
                "JAI: device enumeration failed".into(),
            ));
        }
        if count == 0 {
            return Err(MmError::LocallyDefined("JAI: no cameras found".into()));
        }

        // Find the connection ID: match by serial number, or fall back to index.
        let mut conn_buf = [0i8; BUF];
        let target_idx: i32 = if !self.serial_number.is_empty() {
            let mut found = -1i32;
            let sn_cmp = self.serial_number.clone();
            for i in 0..count {
                let mut sn_buf = [0i8; BUF];
                let rc = unsafe {
                    ffi::jai_system_get_device_serial(sys, i, sn_buf.as_mut_ptr(), BUF as i32)
                };
                if rc != 0 {
                    continue;
                }
                let sn = unsafe { CStr::from_ptr(sn_buf.as_ptr()) };
                if sn.to_string_lossy() == sn_cmp.as_str() {
                    found = i;
                    break;
                }
            }
            if found < 0 {
                return Err(MmError::LocallyDefined(format!(
                    "JAI: camera with serial '{}' not found",
                    self.serial_number
                )));
            }
            found
        } else {
            self.camera_index.min(count - 1)
        };

        let rc = unsafe {
            ffi::jai_system_get_device_id(sys, target_idx, conn_buf.as_mut_ptr(), BUF as i32)
        };
        if rc != 0 {
            return Err(MmError::LocallyDefined(
                "JAI: failed to get connection ID".into(),
            ));
        }
        let conn = unsafe { CStr::from_ptr(conn_buf.as_ptr()) };
        let conn_cstr = CString::new(conn.to_bytes()).map_err(|_| MmError::Err)?;

        // Connect.
        let dev = unsafe { ffi::jai_device_connect(conn_cstr.as_ptr()) };
        if dev.is_null() {
            return Err(MmError::LocallyDefined(
                "JAI: failed to connect to camera".into(),
            ));
        }
        self.device = dev;

        // Apply pre-init settings.
        self.apply_settings();
        self.sync_dimensions();
        self.sync_dynamic_properties()?;

        // Read back model name.
        if let Some(model) = self.dev_get_string("DeviceModelName") {
            self.props
                .entry_mut("Model")
                .map(|e| e.value = PropertyValue::String(model));
        }
        // Read back serial number.
        if self.serial_number.is_empty() {
            if let Some(sn) = self.dev_get_string("DeviceSerialNumber") {
                self.serial_number = sn.clone();
                self.props
                    .entry_mut("SerialNumber")
                    .map(|e| e.value = PropertyValue::String(sn));
            }
        }

        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        let _ = self.stop_sequence_acquisition();
        if !self.device.is_null() {
            unsafe { ffi::jai_device_free(self.device) };
            self.device = std::ptr::null_mut();
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if name == "ExposureIsIndividual" {
            let value = match self.dev_get_enum("ExposureTimeMode").as_deref() {
                Some("Individual") => "On",
                _ => "Off",
            };
            return Ok(PropertyValue::String(value.into()));
        }
        if name == "GainIsIndividual" {
            let value = match self.dev_get_enum("IndividualGainMode").as_deref() {
                Some("On") => "On",
                _ => "Off",
            };
            return Ok(PropertyValue::String(value.into()));
        }
        if let Some(selector) = name.strip_prefix("Exposure_") {
            let value = self
                .get_selector_float(
                    "ExposureTimeSelector",
                    selector,
                    self.common_exposure_selector.as_deref(),
                    "ExposureTime",
                    0.001,
                )
                .or_else(|| self.props.get(name).ok().and_then(PropertyValue::as_f64))
                .ok_or_else(|| MmError::UnknownLabel(name.to_string()))?;
            return Ok(PropertyValue::Float(value));
        }
        if let Some(selector) = name.strip_prefix("Gain_") {
            let value = self
                .get_selector_float(
                    "GainSelector",
                    selector,
                    self.common_gain_selector.as_deref(),
                    "Gain",
                    1.0,
                )
                .or_else(|| self.props.get(name).ok().and_then(PropertyValue::as_f64))
                .ok_or_else(|| MmError::UnknownLabel(name.to_string()))?;
            return Ok(PropertyValue::Float(value));
        }
        if let Some(selector) = name.strip_prefix("BlackLevel_") {
            let value = self
                .get_selector_float("BlackLevelSelector", selector, None, "BlackLevel", 1.0)
                .or_else(|| self.props.get(name).ok().and_then(PropertyValue::as_f64))
                .ok_or_else(|| MmError::UnknownLabel(name.to_string()))?;
            return Ok(PropertyValue::Float(value));
        }

        match name {
            "Exposure" => Ok(PropertyValue::Float(self.exposure_ms)),
            "Gain" => Ok(PropertyValue::Float(self.gain)),
            "PixelFormat" => Ok(PropertyValue::String(self.pixel_format.clone())),
            "PixelType" => Ok(PropertyValue::String(self.pixel_type.clone())),
            "Binning" => Ok(PropertyValue::Integer(self.binning as i64)),
            "FrameRate" => Ok(PropertyValue::Float(
                self.dev_get_float("AcquisitionFrameRate")
                    .unwrap_or(self.frame_rate),
            )),
            "Gamma" => Ok(PropertyValue::Float(
                self.dev_get_float("Gamma").unwrap_or(self.gamma),
            )),
            "BlackLevel" => Ok(PropertyValue::Float(
                self.dev_get_float("BlackLevel").unwrap_or(self.black_level),
            )),
            "TestPattern" => Ok(PropertyValue::String(
                self.dev_get_enum("TestPattern")
                    .unwrap_or_else(|| self.test_pattern.clone()),
            )),
            "WhiteBalance" => Ok(PropertyValue::String(
                self.dev_get_enum("BalanceWhiteAuto")
                    .unwrap_or_else(|| self.white_balance.clone()),
            )),
            "CameraIndex" => Ok(PropertyValue::Integer(self.camera_index as i64)),
            "CameraID" => Ok(PropertyValue::Integer(self.camera_index as i64)),
            "SerialNumber" => Ok(PropertyValue::String(self.serial_number.clone())),
            "Temperature" => Ok(PropertyValue::Float(
                self.dev_get_float("DeviceTemperature").unwrap_or(0.0),
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "ExposureIsIndividual" {
            self.ensure_not_capturing_for_property()?;
            let mode = match val.as_str() {
                "On" => "Individual",
                "Off" => "Common",
                _ => return Err(MmError::InvalidPropertyValue),
            };
            self.dev_set_enum("ExposureTimeMode", mode);
            return self.props.set(name, val);
        }
        if name == "GainIsIndividual" {
            self.ensure_not_capturing_for_property()?;
            let mode = match val.as_str() {
                "On" => "On",
                "Off" => "Off",
                _ => return Err(MmError::InvalidPropertyValue),
            };
            self.dev_set_enum("IndividualGainMode", mode);
            return self.props.set(name, val);
        }
        if let Some(selector) = name.strip_prefix("Exposure_") {
            self.ensure_not_capturing_for_property()?;
            let value = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            self.set_selector_float(
                "ExposureTimeSelector",
                selector,
                self.common_exposure_selector.as_deref(),
                "ExposureTime",
                value,
                1_000.0,
            );
            return self.props.set(name, PropertyValue::Float(value));
        }
        if let Some(selector) = name.strip_prefix("Gain_") {
            self.ensure_not_capturing_for_property()?;
            let value = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            self.set_selector_float(
                "GainSelector",
                selector,
                self.common_gain_selector.as_deref(),
                "Gain",
                value,
                1.0,
            );
            return self.props.set(name, PropertyValue::Float(value));
        }
        if let Some(selector) = name.strip_prefix("BlackLevel_") {
            self.ensure_not_capturing_for_property()?;
            let value = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            self.set_selector_float(
                "BlackLevelSelector",
                selector,
                None,
                "BlackLevel",
                value,
                1.0,
            );
            return self.props.set(name, PropertyValue::Float(value));
        }

        match name {
            "SerialNumber" => {
                if !self.device.is_null() {
                    return Err(MmError::LocallyDefined(
                        "SerialNumber cannot be changed after initialize()".into(),
                    ));
                }
                self.serial_number = val.as_str().to_string();
                self.props.set(name, val)
            }
            "CameraIndex" | "CameraID" => {
                if !self.device.is_null() {
                    return Err(MmError::LocallyDefined(format!(
                        "{name} cannot be changed after initialize()"
                    )));
                }
                self.camera_index = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props.set(
                    "CameraIndex",
                    PropertyValue::Integer(self.camera_index as i64),
                )?;
                self.props
                    .set("CameraID", PropertyValue::Integer(self.camera_index as i64))?;
                Ok(())
            }
            "Exposure" => {
                self.ensure_not_capturing_for_property()?;
                self.exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.exposure_ms))?;
                let ms = self.exposure_ms;
                self.dev_set_float("ExposureTimeAbs", ms * 1_000.0);
                self.dev_set_float("ExposureTime", ms * 1_000.0);
                Ok(())
            }
            "Gain" => {
                self.ensure_not_capturing_for_property()?;
                self.gain = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.gain))?;
                let g = self.gain;
                if self.dev_set_float_check("Gain", g).is_err() {
                    self.dev_set_int("GainRaw", g as i64);
                }
                Ok(())
            }
            "PixelFormat" => {
                self.ensure_not_capturing_for_property()?;
                self.pixel_format = val.as_str().to_string();
                self.pixel_type = pixel_format_to_pixel_type(&self.pixel_format).to_string();
                self.props.set(name, val)?;
                self.props
                    .entry_mut("PixelType")
                    .map(|e| e.value = PropertyValue::String(self.pixel_type.clone()));
                let fmt = self.pixel_format.clone();
                self.dev_set_enum("PixelFormat", &fmt);
                self.sync_dimensions();
                Ok(())
            }
            "PixelType" => {
                self.ensure_not_capturing_for_property()?;
                self.pixel_type = val.as_str().to_string();
                let fmt =
                    pixel_type_to_format(&self.pixel_type).ok_or(MmError::InvalidPropertyValue)?;
                self.pixel_format = fmt.to_string();
                self.props.set(name, val)?;
                self.props.set(
                    "PixelFormat",
                    PropertyValue::String(self.pixel_format.clone()),
                )?;
                self.dev_set_enum("PixelFormat", &self.pixel_format);
                self.sync_dimensions();
                Ok(())
            }
            "Binning" => {
                self.ensure_not_capturing_for_property()?;
                self.binning = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props
                    .set(name, PropertyValue::Integer(self.binning as i64))?;
                let bin = self.binning;
                self.dev_set_int("BinningHorizontal", bin as i64);
                self.dev_set_int("BinningVertical", bin as i64);
                self.sync_dimensions();
                Ok(())
            }
            "FrameRate" => {
                self.ensure_not_capturing_for_property()?;
                self.frame_rate = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.frame_rate))?;
                self.dev_set_float("AcquisitionFrameRate", self.frame_rate);
                Ok(())
            }
            "Gamma" => {
                self.ensure_not_capturing_for_property()?;
                self.gamma = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.gamma))?;
                self.dev_set_float("Gamma", self.gamma);
                Ok(())
            }
            "BlackLevel" => {
                self.ensure_not_capturing_for_property()?;
                self.black_level = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.black_level))?;
                self.dev_set_float("BlackLevel", self.black_level);
                Ok(())
            }
            "TestPattern" => {
                self.ensure_not_capturing_for_property()?;
                self.test_pattern = val.as_str().to_string();
                self.props.set(name, val)?;
                self.dev_set_enum("TestPattern", &self.test_pattern);
                Ok(())
            }
            "WhiteBalance" => {
                self.ensure_not_capturing_for_property()?;
                self.white_balance = val.as_str().to_string();
                self.props.set(name, val)?;
                self.dev_set_enum("BalanceWhiteAuto", &self.white_balance);
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

impl Camera for JAICamera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_open()?;
        if self.seq.is_some() {
            return self.snap_from_sequence();
        }
        self.snap_one_frame()
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if self.image_buf.is_empty() {
            Err(MmError::LocallyDefined("No image captured yet".into()))
        } else {
            Ok(&self.image_buf)
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
        self.num_components
    }
    fn get_number_of_channels(&self) -> u32 {
        1
    }
    fn get_exposure(&self) -> f64 {
        self.exposure_ms
    }

    fn set_exposure(&mut self, exp_ms: f64) -> MmResult<()> {
        if self.seq.is_some() {
            return Err(MmError::CameraBusyAcquiring);
        }
        self.props.set("Exposure", PropertyValue::Float(exp_ms))?;
        self.exposure_ms = exp_ms;
        self.dev_set_float("ExposureTimeAbs", exp_ms * 1_000.0);
        self.dev_set_float("ExposureTime", exp_ms * 1_000.0);
        Ok(())
    }

    fn get_binning(&self) -> i32 {
        self.binning
    }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        self.ensure_not_capturing_for_property()?;
        self.binning = bin;
        self.props
            .set("Binning", PropertyValue::Integer(bin as i64))?;
        self.dev_set_int("BinningHorizontal", bin as i64);
        self.dev_set_int("BinningVertical", bin as i64);
        self.sync_dimensions();
        Ok(())
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        Ok(ImageRoi::new(
            self.dev_get_int("OffsetX").unwrap_or(0).max(0) as u32,
            self.dev_get_int("OffsetY").unwrap_or(0).max(0) as u32,
            self.width,
            self.height,
        ))
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        self.check_open()?;
        self.ensure_not_capturing_for_property()?;
        let width_inc = self.dev_get_int_increment("Width");
        let height_inc = self.dev_get_int_increment("Height");
        let x_inc = self.dev_get_int_increment("OffsetX");
        let y_inc = self.dev_get_int_increment("OffsetY");
        let width = (roi.width as i64 / width_inc) * width_inc;
        let height = (roi.height as i64 / height_inc) * height_inc;
        let x = (roi.x as i64 / x_inc) * x_inc;
        let y = (roi.y as i64 / y_inc) * y_inc;
        // Width/Height before OffsetX/Y (standard GenICam ordering).
        self.dev_set_int("Width", width);
        self.dev_set_int("Height", height);
        self.dev_set_int("OffsetX", x);
        self.dev_set_int("OffsetY", y);
        self.sync_dimensions();
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        self.check_open()?;
        self.ensure_not_capturing_for_property()?;
        self.dev_set_int("OffsetX", 0);
        self.dev_set_int("OffsetY", 0);
        // Set width/height to their hardware maxima via the camera parameter.
        if let Some(max_w) = self.dev_get_int("WidthMax") {
            self.dev_set_int("Width", max_w);
        }
        if let Some(max_h) = self.dev_get_int("HeightMax") {
            self.dev_set_int("Height", max_h);
        }
        self.sync_dimensions();
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, _count: i64, _interval_ms: f64) -> MmResult<()> {
        self.check_open()?;
        if self.seq.is_some() {
            return Ok(());
        }

        self.dev_set_enum("AcquisitionMode", "Continuous");

        // Get connection ID.
        let mut conn_buf = [0i8; BUF];
        let rc = unsafe {
            ffi::jai_device_get_connection_id(self.device, conn_buf.as_mut_ptr(), BUF as i32)
        };
        if rc != 0 {
            return Err(MmError::NotConnected);
        }
        let conn = unsafe { CStr::from_ptr(conn_buf.as_ptr()) };
        let conn_cstr = CString::new(conn.to_bytes()).map_err(|_| MmError::Err)?;

        // Open stream.
        let stream = unsafe { ffi::jai_stream_open(conn_cstr.as_ptr()) };
        if stream.is_null() {
            return Err(MmError::LocallyDefined(
                "JAI: failed to open sequence stream".into(),
            ));
        }

        // Allocate and queue 8 buffers.
        let payload = unsafe { ffi::jai_device_payload_size(self.device) };
        let mut buffers: Vec<*mut ffi::JaiBuffer> = Vec::new();
        for _ in 0..8 {
            let b = unsafe { ffi::jai_buffer_alloc(payload) };
            if b.is_null() {
                break;
            }
            unsafe { ffi::jai_stream_queue(stream, b) };
            buffers.push(b);
        }
        if buffers.is_empty() {
            unsafe { ffi::jai_stream_free(stream) };
            return Err(MmError::LocallyDefined(
                "JAI: buffer allocation failed".into(),
            ));
        }

        unsafe { ffi::jai_device_stream_enable(self.device) };
        self.dev_execute("AcquisitionStart");

        self.seq = Some(SequenceState { stream, buffers });
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        if self.seq.is_none() {
            return Ok(());
        }

        self.dev_execute("AcquisitionStop");
        if !self.device.is_null() {
            unsafe { ffi::jai_device_stream_disable(self.device) };
        }

        if let Some(seq) = self.seq.take() {
            unsafe { ffi::jai_stream_abort(seq.stream) };
            unsafe { ffi::jai_stream_free(seq.stream) };
            for b in seq.buffers {
                unsafe { ffi::jai_buffer_free(b) };
            }
        }
        Ok(())
    }

    fn is_capturing(&self) -> bool {
        self.seq.is_some()
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
    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn jai_stub_env_guard() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn default_properties() {
        let d = JAICamera::new();
        assert_eq!(d.device_type(), DeviceType::Camera);
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(d.get_binning(), 1);
        assert!(!d.is_capturing());
        assert_eq!(d.get_number_of_channels(), 1);
    }

    #[test]
    fn set_camera_index_pre_init() {
        let mut d = JAICamera::new();
        d.set_property("CameraIndex", PropertyValue::Integer(2))
            .unwrap();
        assert_eq!(d.camera_index, 2);
    }

    #[test]
    fn set_serial_number_pre_init() {
        let mut d = JAICamera::new();
        d.set_property("SerialNumber", PropertyValue::String("ABCDEF".into()))
            .unwrap();
        assert_eq!(d.serial_number, "ABCDEF");
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = JAICamera::new();
        d.set_property("Exposure", PropertyValue::Float(50.0))
            .unwrap();
        assert_eq!(d.exposure_ms, 50.0);
        assert_eq!(d.get_exposure(), 50.0);
    }

    #[test]
    fn set_gain_pre_init() {
        let mut d = JAICamera::new();
        d.set_property("Gain", PropertyValue::Float(3.0)).unwrap();
        assert_eq!(d.gain, 3.0);
    }

    #[test]
    fn no_image_before_snap() {
        let d = JAICamera::new();
        assert!(d.get_image_buffer().is_err());
    }

    #[test]
    fn snap_without_init_errors() {
        let mut d = JAICamera::new();
        assert!(d.snap_image().is_err());
    }

    #[test]
    fn initialize_no_camera_fails() {
        let _guard = jai_stub_env_guard();
        std::env::remove_var("JAI_STUB_CAMERA");
        let mut d = JAICamera::new();
        // No eBUS cameras present — expect a meaningful error.
        assert!(d.initialize().is_err());
    }

    #[test]
    fn stub_dynamic_selector_properties_are_created_and_settable() {
        if std::env::var_os("JAI_STUB").is_none() {
            return;
        }
        let _guard = jai_stub_env_guard();
        std::env::set_var("JAI_STUB_CAMERA", "1");
        let mut d = JAICamera::new();
        d.initialize().unwrap();
        std::env::remove_var("JAI_STUB_CAMERA");

        assert!(d.has_property("ExposureIsIndividual"));
        assert!(d.has_property("Exposure_Red"));
        assert!(d.has_property("GainIsIndividual"));
        assert!(d.has_property("Gain_Blue"));
        assert!(d.has_property("BlackLevel_DigitalAll"));

        d.set_property("ExposureIsIndividual", PropertyValue::String("On".into()))
            .unwrap();
        assert_eq!(
            d.get_property("ExposureIsIndividual").unwrap(),
            PropertyValue::String("On".into())
        );
        d.set_property("Exposure_Red", PropertyValue::Float(12.5))
            .unwrap();
        assert_eq!(
            d.get_property("Exposure_Red").unwrap(),
            PropertyValue::Float(12.5)
        );
        d.set_property("Gain_Blue", PropertyValue::Float(3.25))
            .unwrap();
        assert_eq!(
            d.get_property("Gain_Blue").unwrap(),
            PropertyValue::Float(3.25)
        );
    }

    #[test]
    fn readonly_properties() {
        let d = JAICamera::new();
        assert!(d.is_property_read_only("Width"));
        assert!(d.is_property_read_only("Height"));
        assert!(d.is_property_read_only("Model"));
        assert!(!d.is_property_read_only("Exposure"));
    }

    #[test]
    fn bgr8_conversion_matches_upstream_bgra32_layout() {
        let converted = bgr8_to_bgra32(&[1, 2, 3, 99, 4, 5, 6, 88], 1, 2, 1);
        assert_eq!(converted, vec![1, 2, 3, 0, 4, 5, 6, 0]);
    }

    #[test]
    fn packed_bgrp_conversion_matches_upstream_bgra64_layout() {
        let bits = 10u32;
        let b = 0x001u32;
        let g = 0x155u32;
        let r = 0x2aau32;
        let packed = b | (g << bits) | (r << (2 * bits));
        let src = packed.to_le_bytes();
        let converted = packed_bgrp_to_bgra64(&src, 1, 1, bits, 0);
        assert_eq!(&converted[0..2], &(b as u16).to_le_bytes());
        assert_eq!(&converted[2..4], &(g as u16).to_le_bytes());
        assert_eq!(&converted[4..6], &(r as u16).to_le_bytes());
        assert_eq!(&converted[6..8], &0u16.to_le_bytes());
    }

    #[test]
    fn color_pixel_formats_report_converted_buffer_geometry_after_sync() {
        let mut d = JAICamera::new();

        d.pixel_format = "BGR8".into();
        d.sync_dimensions();
        assert_eq!(d.get_image_bytes_per_pixel(), 4);
        assert_eq!(d.get_number_of_components(), 4);

        d.pixel_format = "BGR10p".into();
        d.sync_dimensions();
        assert_eq!(d.get_image_bytes_per_pixel(), 8);
        assert_eq!(d.get_number_of_components(), 4);
        assert_eq!(d.get_bit_depth(), 10);

        d.pixel_format = "BGR12p".into();
        d.sync_dimensions();
        assert_eq!(d.get_image_bytes_per_pixel(), 8);
        assert_eq!(d.get_number_of_components(), 4);
        assert_eq!(d.get_bit_depth(), 12);
    }
}
