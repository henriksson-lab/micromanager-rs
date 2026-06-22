/// Daheng Galaxy camera adapter.
///
/// Wraps the Daheng GxIAPI C library via raw FFI bindings.
/// Exposure is in milliseconds (MicroManager convention), converted to
/// microseconds for the Daheng API.
use std::ffi::{CStr, CString};
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
        ffi::GX_PIXEL_FORMAT_MONO8
        | ffi::GX_PIXEL_FORMAT_BAYER_GR8
        | ffi::GX_PIXEL_FORMAT_BAYER_RG8
        | ffi::GX_PIXEL_FORMAT_BAYER_GB8
        | ffi::GX_PIXEL_FORMAT_BAYER_BG8 => 1,
        ffi::GX_PIXEL_FORMAT_MONO10
        | ffi::GX_PIXEL_FORMAT_MONO12
        | ffi::GX_PIXEL_FORMAT_MONO16
        | ffi::GX_PIXEL_FORMAT_MONO14
        | ffi::GX_PIXEL_FORMAT_BAYER_GR10
        | ffi::GX_PIXEL_FORMAT_BAYER_RG10
        | ffi::GX_PIXEL_FORMAT_BAYER_GB10
        | ffi::GX_PIXEL_FORMAT_BAYER_BG10
        | ffi::GX_PIXEL_FORMAT_BAYER_GR12
        | ffi::GX_PIXEL_FORMAT_BAYER_RG12
        | ffi::GX_PIXEL_FORMAT_BAYER_GB12
        | ffi::GX_PIXEL_FORMAT_BAYER_BG12
        | ffi::GX_PIXEL_FORMAT_BAYER_GR16
        | ffi::GX_PIXEL_FORMAT_BAYER_RG16
        | ffi::GX_PIXEL_FORMAT_BAYER_GB16
        | ffi::GX_PIXEL_FORMAT_BAYER_BG16 => 2,
        _ => 1,
    }
}

fn pixel_format_depth(fmt: i64) -> u32 {
    match fmt {
        ffi::GX_PIXEL_FORMAT_MONO10
        | ffi::GX_PIXEL_FORMAT_BAYER_GR10
        | ffi::GX_PIXEL_FORMAT_BAYER_RG10
        | ffi::GX_PIXEL_FORMAT_BAYER_GB10
        | ffi::GX_PIXEL_FORMAT_BAYER_BG10 => 10,
        ffi::GX_PIXEL_FORMAT_MONO12
        | ffi::GX_PIXEL_FORMAT_BAYER_GR12
        | ffi::GX_PIXEL_FORMAT_BAYER_RG12
        | ffi::GX_PIXEL_FORMAT_BAYER_GB12
        | ffi::GX_PIXEL_FORMAT_BAYER_BG12 => 12,
        ffi::GX_PIXEL_FORMAT_MONO14 => 14,
        ffi::GX_PIXEL_FORMAT_MONO16
        | ffi::GX_PIXEL_FORMAT_BAYER_GR16
        | ffi::GX_PIXEL_FORMAT_BAYER_RG16
        | ffi::GX_PIXEL_FORMAT_BAYER_GB16
        | ffi::GX_PIXEL_FORMAT_BAYER_BG16 => 16,
        _ => 8,
    }
}

fn is_bayer_format(fmt: i64) -> bool {
    matches!(
        fmt,
        ffi::GX_PIXEL_FORMAT_BAYER_GR8
            | ffi::GX_PIXEL_FORMAT_BAYER_RG8
            | ffi::GX_PIXEL_FORMAT_BAYER_GB8
            | ffi::GX_PIXEL_FORMAT_BAYER_BG8
            | ffi::GX_PIXEL_FORMAT_BAYER_GR10
            | ffi::GX_PIXEL_FORMAT_BAYER_RG10
            | ffi::GX_PIXEL_FORMAT_BAYER_GB10
            | ffi::GX_PIXEL_FORMAT_BAYER_BG10
            | ffi::GX_PIXEL_FORMAT_BAYER_GR12
            | ffi::GX_PIXEL_FORMAT_BAYER_RG12
            | ffi::GX_PIXEL_FORMAT_BAYER_GB12
            | ffi::GX_PIXEL_FORMAT_BAYER_BG12
            | ffi::GX_PIXEL_FORMAT_BAYER_GR16
            | ffi::GX_PIXEL_FORMAT_BAYER_RG16
            | ffi::GX_PIXEL_FORMAT_BAYER_GB16
            | ffi::GX_PIXEL_FORMAT_BAYER_BG16
    )
}

fn bayer_channel(fmt: i64, x: usize, y: usize) -> usize {
    let even_x = x % 2 == 0;
    let even_y = y % 2 == 0;
    match fmt {
        ffi::GX_PIXEL_FORMAT_BAYER_RG8
        | ffi::GX_PIXEL_FORMAT_BAYER_RG10
        | ffi::GX_PIXEL_FORMAT_BAYER_RG12
        | ffi::GX_PIXEL_FORMAT_BAYER_RG16 => match (even_x, even_y) {
            (true, true) => 0,
            (false, false) => 2,
            _ => 1,
        },
        ffi::GX_PIXEL_FORMAT_BAYER_BG8
        | ffi::GX_PIXEL_FORMAT_BAYER_BG10
        | ffi::GX_PIXEL_FORMAT_BAYER_BG12
        | ffi::GX_PIXEL_FORMAT_BAYER_BG16 => match (even_x, even_y) {
            (true, true) => 2,
            (false, false) => 0,
            _ => 1,
        },
        ffi::GX_PIXEL_FORMAT_BAYER_GB8
        | ffi::GX_PIXEL_FORMAT_BAYER_GB10
        | ffi::GX_PIXEL_FORMAT_BAYER_GB12
        | ffi::GX_PIXEL_FORMAT_BAYER_GB16 => match (even_x, even_y) {
            (false, true) => 2,
            (true, false) => 0,
            _ => 1,
        },
        ffi::GX_PIXEL_FORMAT_BAYER_GR8
        | ffi::GX_PIXEL_FORMAT_BAYER_GR10
        | ffi::GX_PIXEL_FORMAT_BAYER_GR12
        | ffi::GX_PIXEL_FORMAT_BAYER_GR16 => match (even_x, even_y) {
            (false, true) => 0,
            (true, false) => 2,
            _ => 1,
        },
        _ => 1,
    }
}

fn bayer_sample_to_u8(
    src: &[u8],
    idx: usize,
    bytes_per_pixel: usize,
    bit_depth: u32,
) -> Option<u8> {
    match bytes_per_pixel {
        1 => src.get(idx).copied(),
        2 => {
            let byte_idx = idx.checked_mul(2)?;
            let bytes = [*src.get(byte_idx)?, *src.get(byte_idx + 1)?];
            let value = u16::from_le_bytes(bytes);
            let shift = bit_depth.saturating_sub(8).min(8);
            Some((value >> shift) as u8)
        }
        _ => None,
    }
}

fn bayer_to_bgra(src: &[u8], width: u32, height: u32, fmt: i64) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let bytes_per_pixel = pixel_format_bpp(fmt) as usize;
    let bit_depth = pixel_format_depth(fmt);
    let mut dst = vec![0u8; w.saturating_mul(h).saturating_mul(4)];
    for y in 0..h {
        for x in 0..w {
            let mut sums = [0u32; 3];
            let mut counts = [0u32; 3];
            let y0 = y.saturating_sub(1);
            let y1 = (y + 1).min(h.saturating_sub(1));
            let x0 = x.saturating_sub(1);
            let x1 = (x + 1).min(w.saturating_sub(1));
            for yy in y0..=y1 {
                for xx in x0..=x1 {
                    let idx = yy * w + xx;
                    let channel = bayer_channel(fmt, xx, yy);
                    let Some(sample) = bayer_sample_to_u8(src, idx, bytes_per_pixel, bit_depth)
                    else {
                        continue;
                    };
                    sums[channel] += sample as u32;
                    counts[channel] += 1;
                }
            }
            let r = if counts[0] == 0 {
                0
            } else {
                (sums[0] / counts[0]) as u8
            };
            let g = if counts[1] == 0 {
                0
            } else {
                (sums[1] / counts[1]) as u8
            };
            let b = if counts[2] == 0 {
                0
            } else {
                (sums[2] / counts[2]) as u8
            };
            let out = (y * w + x) * 4;
            dst[out] = b;
            dst[out + 1] = g;
            dst[out + 2] = r;
            dst[out + 3] = 255;
        }
    }
    dst
}

fn bayer8_to_bgra(src: &[u8], width: u32, height: u32, fmt: i64) -> Vec<u8> {
    bayer_to_bgra(src, width, height, fmt)
}

fn pixel_format_name(fmt: i64) -> &'static str {
    match fmt {
        ffi::GX_PIXEL_FORMAT_MONO8 => "Mono8",
        ffi::GX_PIXEL_FORMAT_MONO10 => "Mono10",
        ffi::GX_PIXEL_FORMAT_MONO12 => "Mono12",
        ffi::GX_PIXEL_FORMAT_MONO14 => "Mono14",
        ffi::GX_PIXEL_FORMAT_MONO16 => "Mono16",
        ffi::GX_PIXEL_FORMAT_BAYER_GR8 => "BayerGR8",
        ffi::GX_PIXEL_FORMAT_BAYER_RG8 => "BayerRG8",
        ffi::GX_PIXEL_FORMAT_BAYER_GB8 => "BayerGB8",
        ffi::GX_PIXEL_FORMAT_BAYER_BG8 => "BayerBG8",
        ffi::GX_PIXEL_FORMAT_BAYER_GR10 => "BayerGR10",
        ffi::GX_PIXEL_FORMAT_BAYER_RG10 => "BayerRG10",
        ffi::GX_PIXEL_FORMAT_BAYER_GB10 => "BayerGB10",
        ffi::GX_PIXEL_FORMAT_BAYER_BG10 => "BayerBG10",
        ffi::GX_PIXEL_FORMAT_BAYER_GR12 => "BayerGR12",
        ffi::GX_PIXEL_FORMAT_BAYER_RG12 => "BayerRG12",
        ffi::GX_PIXEL_FORMAT_BAYER_GB12 => "BayerGB12",
        ffi::GX_PIXEL_FORMAT_BAYER_BG12 => "BayerBG12",
        ffi::GX_PIXEL_FORMAT_BAYER_GR16 => "BayerGR16",
        ffi::GX_PIXEL_FORMAT_BAYER_RG16 => "BayerRG16",
        ffi::GX_PIXEL_FORMAT_BAYER_GB16 => "BayerGB16",
        ffi::GX_PIXEL_FORMAT_BAYER_BG16 => "BayerBG16",
        _ => "Unknown",
    }
}

fn captured_pixel_type_name(fmt: i64) -> &'static str {
    match fmt {
        ffi::GX_PIXEL_FORMAT_MONO8 => "8bit mono",
        ffi::GX_PIXEL_FORMAT_MONO10
        | ffi::GX_PIXEL_FORMAT_MONO12
        | ffi::GX_PIXEL_FORMAT_MONO14
        | ffi::GX_PIXEL_FORMAT_MONO16 => "16bit mono",
        fmt if is_bayer_format(fmt) => "8bitBGRA",
        _ => pixel_format_name(fmt),
    }
}

fn pixel_format_from_name(name: &str) -> Option<i64> {
    match name {
        "Mono8" => Some(ffi::GX_PIXEL_FORMAT_MONO8),
        "Mono10" => Some(ffi::GX_PIXEL_FORMAT_MONO10),
        "Mono12" => Some(ffi::GX_PIXEL_FORMAT_MONO12),
        "Mono14" => Some(ffi::GX_PIXEL_FORMAT_MONO14),
        "Mono16" => Some(ffi::GX_PIXEL_FORMAT_MONO16),
        "BayerGR8" => Some(ffi::GX_PIXEL_FORMAT_BAYER_GR8),
        "BayerRG8" => Some(ffi::GX_PIXEL_FORMAT_BAYER_RG8),
        "BayerGB8" => Some(ffi::GX_PIXEL_FORMAT_BAYER_GB8),
        "BayerBG8" => Some(ffi::GX_PIXEL_FORMAT_BAYER_BG8),
        "BayerGR10" => Some(ffi::GX_PIXEL_FORMAT_BAYER_GR10),
        "BayerRG10" => Some(ffi::GX_PIXEL_FORMAT_BAYER_RG10),
        "BayerGB10" => Some(ffi::GX_PIXEL_FORMAT_BAYER_GB10),
        "BayerBG10" => Some(ffi::GX_PIXEL_FORMAT_BAYER_BG10),
        "BayerGR12" => Some(ffi::GX_PIXEL_FORMAT_BAYER_GR12),
        "BayerRG12" => Some(ffi::GX_PIXEL_FORMAT_BAYER_RG12),
        "BayerGB12" => Some(ffi::GX_PIXEL_FORMAT_BAYER_GB12),
        "BayerBG12" => Some(ffi::GX_PIXEL_FORMAT_BAYER_BG12),
        "BayerGR16" => Some(ffi::GX_PIXEL_FORMAT_BAYER_GR16),
        "BayerRG16" => Some(ffi::GX_PIXEL_FORMAT_BAYER_RG16),
        "BayerGB16" => Some(ffi::GX_PIXEL_FORMAT_BAYER_GB16),
        "BayerBG16" => Some(ffi::GX_PIXEL_FORMAT_BAYER_BG16),
        _ => None,
    }
}

const PIXEL_FORMAT_NAMES: &[&str] = &[
    "Mono8",
    "Mono10",
    "Mono12",
    "Mono14",
    "Mono16",
    "BayerGR8",
    "BayerRG8",
    "BayerGB8",
    "BayerBG8",
    "BayerGR10",
    "BayerRG10",
    "BayerGB10",
    "BayerBG10",
    "BayerGR12",
    "BayerRG12",
    "BayerGB12",
    "BayerBG12",
    "BayerGR16",
    "BayerRG16",
    "BayerGB16",
    "BayerBG16",
];

fn trigger_source_from_name(name: &str) -> Option<i64> {
    match name {
        "Software" => Some(ffi::GX_TRIGGER_SOURCE_SOFTWARE),
        "Line0" => Some(ffi::GX_TRIGGER_SOURCE_LINE0),
        "Line1" => Some(ffi::GX_TRIGGER_SOURCE_LINE1),
        "Line2" => Some(ffi::GX_TRIGGER_SOURCE_LINE2),
        "Line3" => Some(ffi::GX_TRIGGER_SOURCE_LINE3),
        _ => None,
    }
}

fn trigger_source_name(value: i64) -> &'static str {
    match value {
        ffi::GX_TRIGGER_SOURCE_LINE0 => "Line0",
        ffi::GX_TRIGGER_SOURCE_LINE1 => "Line1",
        ffi::GX_TRIGGER_SOURCE_LINE2 => "Line2",
        ffi::GX_TRIGGER_SOURCE_LINE3 => "Line3",
        _ => "Software",
    }
}

const TRIGGER_SOURCE_NAMES: &[&str] = &["Software", "Line0", "Line1", "Line2", "Line3"];

fn trigger_activation_name(value: i64) -> &'static str {
    match value {
        ffi::GX_TRIGGER_ACTIVATION_FALLING_EDGE => "FallingEdge",
        _ => "RisingEdge",
    }
}

fn acquisition_frame_rate_mode_from_name(name: &str) -> Option<i64> {
    match name {
        "Off" => Some(ffi::GX_ACQUISITION_FRAME_RATE_MODE_OFF),
        "On" => Some(ffi::GX_ACQUISITION_FRAME_RATE_MODE_ON),
        _ => None,
    }
}

fn acquisition_frame_rate_mode_name(value: i64) -> &'static str {
    match value {
        ffi::GX_ACQUISITION_FRAME_RATE_MODE_ON => "On",
        _ => "Off",
    }
}

const ACQUISITION_FRAME_RATE_MODE_NAMES: &[&str] = &["Off", "On"];

fn user_output_selector_from_name(name: &str) -> Option<i64> {
    match name {
        "UserOutput0" => Some(ffi::GX_USER_OUTPUT_SELECTOR_OUTPUT0),
        "UserOutput1" => Some(ffi::GX_USER_OUTPUT_SELECTOR_OUTPUT1),
        "UserOutput2" => Some(ffi::GX_USER_OUTPUT_SELECTOR_OUTPUT2),
        _ => None,
    }
}

fn user_output_selector_name(value: i64) -> &'static str {
    match value {
        ffi::GX_USER_OUTPUT_SELECTOR_OUTPUT1 => "UserOutput1",
        ffi::GX_USER_OUTPUT_SELECTOR_OUTPUT2 => "UserOutput2",
        _ => "UserOutput0",
    }
}

const USER_OUTPUT_SELECTOR_NAMES: &[&str] = &["UserOutput0", "UserOutput1", "UserOutput2"];

fn line_selector_feature(line: usize) -> Option<i64> {
    match line {
        0 => Some(ffi::GX_LINE_SELECTOR_LINE0),
        1 => Some(ffi::GX_LINE_SELECTOR_LINE1),
        _ => None,
    }
}

fn line_mode_from_name(name: &str) -> Option<i64> {
    match name {
        "Input" => Some(ffi::GX_LINE_MODE_INPUT),
        "Output" => Some(ffi::GX_LINE_MODE_OUTPUT),
        _ => None,
    }
}

fn line_mode_name(value: i64) -> &'static str {
    match value {
        ffi::GX_LINE_MODE_OUTPUT => "Output",
        _ => "Input",
    }
}

const LINE_MODE_NAMES: &[&str] = &["Input", "Output"];

fn line_source_from_name(name: &str) -> Option<i64> {
    match name {
        "Off" => Some(ffi::GX_LINE_SOURCE_OFF),
        "ExposureActive" => Some(ffi::GX_LINE_SOURCE_EXPOSURE_ACTIVE),
        "UserOutput0" => Some(ffi::GX_LINE_SOURCE_USER_OUTPUT0),
        _ => None,
    }
}

fn line_source_name(value: i64) -> &'static str {
    match value {
        ffi::GX_LINE_SOURCE_EXPOSURE_ACTIVE => "ExposureActive",
        ffi::GX_LINE_SOURCE_USER_OUTPUT0 => "UserOutput0",
        _ => "Off",
    }
}

const LINE_SOURCE_NAMES: &[&str] = &["Off", "ExposureActive", "UserOutput0"];

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

fn validate_nonnegative_finite(value: f64) -> MmResult<f64> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(MmError::InvalidPropertyValue)
    }
}

fn upstream_atoi_float(value: f64) -> f64 {
    value.trunc()
}

fn exposure_timeout_ms(timeout_s: i64) -> u32 {
    timeout_s.saturating_mul(1000).clamp(0, u32::MAX as i64) as u32
}

fn validated_frame_raw_size(
    width: i32,
    height: i32,
    pixel_format: i64,
    image_size: i32,
) -> MmResult<usize> {
    if width <= 0 || height <= 0 || image_size < 0 {
        return Err(MmError::SnapImageFailed);
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(pixel_format_bpp(pixel_format) as usize))
        .ok_or(MmError::SnapImageFailed)?;
    if image_size as usize != expected {
        return Err(MmError::SnapImageFailed);
    }
    Ok(expected)
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
    components: u32,
    bit_depth: u32,
    pixel_format: i64,
    exposure_us: f64,
    binning: i32,
    capturing: bool,
    serial_number: String,
    trigger_mode: String,
    trigger_source: String,
    trigger_activation: String,
    exposure_timeout_s: i64,
    acquisition_frame_rate_mode: String,
    acquisition_frame_rate: f64,
    trigger_delay: f64,
    trigger_filter_raising_edge: f64,
    user_output_selector: String,
    line_modes: [String; 2],
    line_sources: [String; 2],
    sequence_remaining: Option<i64>,
}

impl DahengCamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("SerialNumber", PropertyValue::String("".into()), false)
            .unwrap();
        props
            .define_property("CameraID", PropertyValue::String("".into()), true)
            .unwrap();
        props
            .define_property("Exposure(us)", PropertyValue::Float(10_000.0), false)
            .unwrap();
        props
            .define_property("Gain", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("PixelType", PropertyValue::String("Mono8".into()), false)
            .unwrap();
        props
            .set_allowed_values("PixelType", PIXEL_FORMAT_NAMES)
            .unwrap();
        props
            .define_property("Binning", PropertyValue::Integer(1), false)
            .unwrap();
        props.set_allowed_values("Binning", &["1", "2"]).unwrap();
        props
            .define_property("TriggerMode", PropertyValue::String("Off".into()), false)
            .unwrap();
        props
            .set_allowed_values("TriggerMode", &["Off", "On"])
            .unwrap();
        props
            .define_property(
                "TriggerSource",
                PropertyValue::String("Software".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("TriggerSource", TRIGGER_SOURCE_NAMES)
            .unwrap();
        props
            .define_property(
                "TriggerActivation",
                PropertyValue::String("RisingEdge".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("TriggerActivation", &["FallingEdge", "RisingEdge"])
            .unwrap();
        props
            .define_property("Width", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("Height", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("SensorWidth", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("SensorHeight", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("ExposureTimeoutSeconds", PropertyValue::Integer(5), false)
            .unwrap();
        props
            .define_property(
                "AcquisitionFrameRateMode",
                PropertyValue::String("Off".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values(
                "AcquisitionFrameRateMode",
                ACQUISITION_FRAME_RATE_MODE_NAMES,
            )
            .unwrap();
        props
            .define_property("AcquisitionFrameRate", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("TriggerDelay", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("TriggerFilterRaisingEdge", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property(
                "UserOutputSelector",
                PropertyValue::String("UserOutput0".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("UserOutputSelector", USER_OUTPUT_SELECTOR_NAMES)
            .unwrap();
        for line in 0..2 {
            let mode_name = format!("Line{}-Mode", line);
            props
                .define_property(
                    mode_name.as_str(),
                    PropertyValue::String("Input".into()),
                    false,
                )
                .unwrap();
            props
                .set_allowed_values(mode_name.as_str(), LINE_MODE_NAMES)
                .unwrap();
            let source_name = format!("Line{}-Source", line);
            props
                .define_property(
                    source_name.as_str(),
                    PropertyValue::String("Off".into()),
                    false,
                )
                .unwrap();
            props
                .set_allowed_values(source_name.as_str(), LINE_SOURCE_NAMES)
                .unwrap();
        }

        Self {
            props,
            handle: ptr::null_mut(),
            lib_initialized: false,
            img_buf: Vec::new(),
            width: 0,
            height: 0,
            bytes_per_pixel: 1,
            components: 1,
            bit_depth: 8,
            pixel_format: ffi::GX_PIXEL_FORMAT_MONO8,
            exposure_us: 10_000.0,
            binning: 1,
            capturing: false,
            serial_number: String::new(),
            trigger_mode: "Off".into(),
            trigger_source: "Software".into(),
            trigger_activation: "RisingEdge".into(),
            exposure_timeout_s: 5,
            acquisition_frame_rate_mode: "Off".into(),
            acquisition_frame_rate: 0.0,
            trigger_delay: 0.0,
            trigger_filter_raising_edge: 0.0,
            user_output_selector: "UserOutput0".into(),
            line_modes: ["Input".into(), "Input".into()],
            line_sources: ["Off".into(), "Off".into()],
            sequence_remaining: None,
        }
    }

    fn check_open(&self) -> MmResult<()> {
        if self.handle.is_null() {
            Err(MmError::NotConnected)
        } else {
            Ok(())
        }
    }

    fn write_exposure(&self, us: f64) -> MmResult<()> {
        unsafe {
            gx_check(
                ffi::GXSetFloat(self.handle, ffi::GX_FLOAT_EXPOSURE_TIME, us),
                "GXSetFloat(ExposureTime)",
            )
        }
    }

    fn write_gain(&self, gain: f64) -> MmResult<()> {
        unsafe {
            gx_check(
                ffi::GXSetFloat(self.handle, ffi::GX_FLOAT_GAIN, gain),
                "GXSetFloat(Gain)",
            )
        }
    }

    fn write_binning(&self, bin: i32) -> MmResult<()> {
        unsafe {
            gx_check(
                ffi::GXSetInt(self.handle, ffi::GX_INT_BINNING_HORIZONTAL, bin as i64),
                "GXSetInt(BinningHorizontal)",
            )?;
            gx_check(
                ffi::GXSetInt(self.handle, ffi::GX_INT_BINNING_VERTICAL, bin as i64),
                "GXSetInt(BinningVertical)",
            )
        }
    }

    fn write_pixel_format(&self, fmt: i64) -> MmResult<()> {
        unsafe {
            gx_check(
                ffi::GXSetEnum(self.handle, ffi::GX_ENUM_PIXEL_FORMAT, fmt),
                "GXSetEnum(PixelFormat)",
            )
        }
    }

    fn write_float_feature(&self, feature_id: i32, value: f64, context: &str) -> MmResult<()> {
        if self.handle.is_null() {
            return Ok(());
        }
        unsafe { gx_check(ffi::GXSetFloat(self.handle, feature_id, value), context) }
    }

    fn read_float_feature(&self, feature_id: i32) -> Option<f64> {
        if self.handle.is_null() {
            return None;
        }
        let mut value = 0.0;
        let status = unsafe { ffi::GXGetFloat(self.handle, feature_id, &mut value) };
        (status == ffi::GX_STATUS_SUCCESS).then_some(value)
    }

    fn read_int_feature(&self, feature_id: i32) -> Option<i64> {
        if self.handle.is_null() {
            return None;
        }
        let mut value = 0;
        let status = unsafe { ffi::GXGetInt(self.handle, feature_id, &mut value) };
        (status == ffi::GX_STATUS_SUCCESS).then_some(value)
    }

    fn write_enum_feature(&self, feature_id: i32, value: i64, context: &str) -> MmResult<()> {
        if self.handle.is_null() {
            return Ok(());
        }
        unsafe { gx_check(ffi::GXSetEnum(self.handle, feature_id, value), context) }
    }

    fn read_enum_feature(&self, feature_id: i32) -> Option<i64> {
        if self.handle.is_null() {
            return None;
        }
        let mut value = 0;
        let status = unsafe { ffi::GXGetEnum(self.handle, feature_id, &mut value) };
        (status == ffi::GX_STATUS_SUCCESS).then_some(value)
    }

    fn with_selected_line<T>(
        &self,
        line: usize,
        context: &str,
        f: impl FnOnce() -> MmResult<T>,
    ) -> MmResult<T> {
        if self.handle.is_null() {
            return f();
        }
        let selector = line_selector_feature(line).ok_or(MmError::InvalidPropertyValue)?;
        let previous = self.read_enum_feature(ffi::GX_ENUM_LINE_SELECTOR);
        self.write_enum_feature(ffi::GX_ENUM_LINE_SELECTOR, selector, context)?;
        let result = f();
        if let Some(previous) = previous {
            let _ = self.write_enum_feature(
                ffi::GX_ENUM_LINE_SELECTOR,
                previous,
                "GXSetEnum(LineSelector restore)",
            );
        }
        result
    }

    fn write_line_mode(&self, line: usize, mode: i64) -> MmResult<()> {
        self.with_selected_line(line, "GXSetEnum(LineSelector)", || {
            self.write_enum_feature(ffi::GX_ENUM_LINE_MODE, mode, "GXSetEnum(LineMode)")
        })
    }

    fn read_line_mode(&self, line: usize) -> Option<i64> {
        self.with_selected_line(line, "GXSetEnum(LineSelector)", || {
            self.read_enum_feature(ffi::GX_ENUM_LINE_MODE)
                .ok_or(MmError::InvalidPropertyValue)
        })
        .ok()
    }

    fn write_line_source(&self, line: usize, source: i64) -> MmResult<()> {
        self.with_selected_line(line, "GXSetEnum(LineSelector)", || {
            self.write_enum_feature(ffi::GX_ENUM_LINE_SOURCE, source, "GXSetEnum(LineSource)")
        })
    }

    fn read_line_source(&self, line: usize) -> Option<i64> {
        self.with_selected_line(line, "GXSetEnum(LineSelector)", || {
            self.read_enum_feature(ffi::GX_ENUM_LINE_SOURCE)
                .ok_or(MmError::InvalidPropertyValue)
        })
        .ok()
    }

    fn write_trigger_mode(&self) -> MmResult<()> {
        if self.handle.is_null() {
            return Ok(());
        }
        let mode = if self.trigger_mode == "On" {
            ffi::GX_TRIGGER_MODE_ON
        } else {
            ffi::GX_TRIGGER_MODE_OFF
        };
        let source =
            trigger_source_from_name(&self.trigger_source).ok_or(MmError::InvalidPropertyValue)?;
        unsafe {
            let _ = ffi::GXSetEnum(
                self.handle,
                ffi::GX_ENUM_TRIGGER_SELECTOR,
                ffi::GX_TRIGGER_SELECTOR_FRAME_START,
            );
            gx_check(
                ffi::GXSetEnum(self.handle, ffi::GX_ENUM_TRIGGER_MODE, mode),
                "GXSetEnum(TriggerMode)",
            )?;
            gx_check(
                ffi::GXSetEnum(self.handle, ffi::GX_ENUM_TRIGGER_SOURCE, source),
                "GXSetEnum(TriggerSource)",
            )?;
            let activation = if self.trigger_activation == "FallingEdge" {
                ffi::GX_TRIGGER_ACTIVATION_FALLING_EDGE
            } else {
                ffi::GX_TRIGGER_ACTIVATION_RISING_EDGE
            };
            gx_check(
                ffi::GXSetEnum(self.handle, ffi::GX_ENUM_TRIGGER_ACTIVATION, activation),
                "GXSetEnum(TriggerActivation)",
            )
        }
    }

    fn read_device_serial_number(&self) -> Option<String> {
        if self.handle.is_null() {
            return None;
        }
        let mut buf = [0_i8; 64];
        let mut size = buf.len();
        let status = unsafe {
            ffi::GXGetString(
                self.handle,
                ffi::GX_STRING_DEVICE_SERIAL_NUMBER,
                buf.as_mut_ptr(),
                &mut size,
            )
        };
        if status != ffi::GX_STATUS_SUCCESS {
            return None;
        }
        unsafe { CStr::from_ptr(buf.as_ptr()) }
            .to_str()
            .ok()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    fn discover_serial_numbers(device_num: u32) -> Vec<String> {
        if device_num == 0 {
            return Vec::new();
        }
        let mut infos = vec![ffi::GxDeviceBaseInfo::default(); device_num as usize];
        let mut buffer_size = infos.len() * std::mem::size_of::<ffi::GxDeviceBaseInfo>();
        let status = unsafe { ffi::GXGetAllDeviceBaseInfo(infos.as_mut_ptr(), &mut buffer_size) };
        if status != ffi::GX_STATUS_SUCCESS {
            return Vec::new();
        }
        infos
            .iter()
            .filter_map(|info| {
                unsafe { CStr::from_ptr(info.serial_number.as_ptr()) }
                    .to_str()
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            })
            .collect()
    }

    fn apply_discovered_serial_numbers(&mut self, serials: &[String]) {
        if serials.is_empty() {
            return;
        }
        if let Some(entry) = self.props.entry_mut("SerialNumber") {
            entry.allowed_values = serials.to_vec();
        }
        if self.serial_number.is_empty() {
            self.serial_number = serials[0].clone();
            if let Some(entry) = self.props.entry_mut("SerialNumber") {
                entry.value = PropertyValue::String(self.serial_number.clone());
            }
        }
    }

    fn send_software_trigger_if_configured(&self) -> MmResult<()> {
        if self.trigger_mode == "On" && self.trigger_source == "Software" {
            unsafe {
                gx_check(
                    ffi::GXSendCommand(self.handle, ffi::GX_COMMAND_TRIGGER_SOFTWARE),
                    "GXSendCommand(TriggerSoftware)",
                )?;
            }
        }
        Ok(())
    }

    fn int_range(&self, feature_id: i32) -> MmResult<ffi::GxIntRange> {
        let mut range = ffi::GxIntRange::default();
        unsafe {
            gx_check(
                ffi::GXGetIntRange(self.handle, feature_id, &mut range),
                "GXGetIntRange",
            )?;
        }
        if range.inc <= 0 {
            range.inc = 1;
        }
        Ok(range)
    }

    fn round_to_integer_range(range: ffi::GxIntRange, value: i64) -> i64 {
        let inc = range.inc.max(1);
        let rounded = value - value.rem_euclid(inc);
        rounded.clamp(range.min, range.max)
    }

    fn clamp_integer_to_range(&self, feature_id: i32, value: i64) -> MmResult<i64> {
        let range = self.int_range(feature_id)?;
        Ok(Self::round_to_integer_range(range, value))
    }

    fn apply_roi_values_ordered(
        &self,
        current_roi: ImageRoi,
        target_roi: ImageRoi,
        sensor_width: i64,
        sensor_height: i64,
    ) -> MmResult<()> {
        let x = target_roi.x as i64;
        let y = target_roi.y as i64;
        let width = target_roi.width as i64;
        let height = target_roi.height as i64;
        let set_int = |feature_id, value, context| unsafe {
            gx_check(ffi::GXSetInt(self.handle, feature_id, value), context)
        };
        if x + current_roi.width as i64 <= sensor_width {
            set_int(ffi::GX_INT_OFFSET_X, x, "GXSetInt(OffsetX)")
                .and_then(|_| set_int(ffi::GX_INT_WIDTH, width, "GXSetInt(Width)"))?;
        } else {
            set_int(ffi::GX_INT_WIDTH, width, "GXSetInt(Width)")
                .and_then(|_| set_int(ffi::GX_INT_OFFSET_X, x, "GXSetInt(OffsetX)"))?;
        }
        if y + current_roi.height as i64 <= sensor_height {
            set_int(ffi::GX_INT_OFFSET_Y, y, "GXSetInt(OffsetY)")
                .and_then(|_| set_int(ffi::GX_INT_HEIGHT, height, "GXSetInt(Height)"))
        } else {
            set_int(ffi::GX_INT_HEIGHT, height, "GXSetInt(Height)")
                .and_then(|_| set_int(ffi::GX_INT_OFFSET_Y, y, "GXSetInt(OffsetY)"))
        }
    }

    fn require_not_capturing(&self) -> MmResult<()> {
        if self.capturing {
            Err(MmError::CameraBusyAcquiring)
        } else {
            Ok(())
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
                self.update_pixel_metadata(fmt);
            }
        }
        self.props
            .entry_mut("Width")
            .map(|e| e.value = PropertyValue::Integer(self.width as i64));
        self.props
            .entry_mut("Height")
            .map(|e| e.value = PropertyValue::Integer(self.height as i64));
        self.props
            .entry_mut("SensorWidth")
            .map(|e| e.value = PropertyValue::Integer(self.width as i64));
        self.props
            .entry_mut("SensorHeight")
            .map(|e| e.value = PropertyValue::Integer(self.height as i64));
        self.props.entry_mut("PixelType").map(|e| {
            e.value = PropertyValue::String(pixel_format_name(self.pixel_format).to_string())
        });
    }

    fn update_pixel_metadata(&mut self, fmt: i64) {
        self.pixel_format = fmt;
        if is_bayer_format(fmt) {
            self.bytes_per_pixel = 4;
            self.components = 4;
            self.bit_depth = 8;
        } else {
            self.bytes_per_pixel = pixel_format_bpp(fmt);
            self.components = 1;
            self.bit_depth = pixel_format_depth(fmt);
        }
    }

    fn fetch_frame(&mut self) -> MmResult<()> {
        let mut frame = ffi::GxFrameData::default();
        unsafe {
            gx_check(
                ffi::GXGetImage(
                    self.handle,
                    &mut frame,
                    exposure_timeout_ms(self.exposure_timeout_s),
                ),
                "GXGetImage",
            )?;
        }
        if frame.status != ffi::GX_STATUS_SUCCESS || frame.image_buf.is_null() {
            return Err(MmError::SnapImageFailed);
        }

        let size = validated_frame_raw_size(
            frame.width,
            frame.height,
            frame.pixel_format as i64,
            frame.image_size,
        )?;
        let mut raw = vec![0u8; size];
        unsafe {
            ptr::copy_nonoverlapping(frame.image_buf as *const u8, raw.as_mut_ptr(), size);
        }

        self.width = frame.width as u32;
        self.height = frame.height as u32;
        self.pixel_format = frame.pixel_format as i64;
        self.bit_depth = pixel_format_depth(self.pixel_format);
        if is_bayer_format(self.pixel_format) {
            self.img_buf = bayer_to_bgra(&raw, self.width, self.height, self.pixel_format);
            self.bytes_per_pixel = 4;
            self.components = 4;
            self.bit_depth = 8;
        } else {
            self.img_buf = raw;
            self.bytes_per_pixel = pixel_format_bpp(self.pixel_format);
            self.components = 1;
        }
        self.props
            .entry_mut("Width")
            .map(|e| e.value = PropertyValue::Integer(self.width as i64));
        self.props
            .entry_mut("Height")
            .map(|e| e.value = PropertyValue::Integer(self.height as i64));
        self.props.entry_mut("PixelType").map(|e| {
            e.value = PropertyValue::String(captured_pixel_type_name(self.pixel_format).to_string())
        });

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
        "Daheng Camera device adapter"
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
            return Err(MmError::LocallyDefined("No Daheng cameras found".into()));
        }
        let discovered_serials = Self::discover_serial_numbers(device_num);
        self.apply_discovered_serial_numbers(&discovered_serials);

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
        let camera_id = self
            .read_device_serial_number()
            .or_else(|| {
                if self.serial_number.is_empty() {
                    discovered_serials.first().cloned()
                } else {
                    Some(self.serial_number.clone())
                }
            })
            .unwrap_or_default();
        if let Some(entry) = self.props.entry_mut("CameraID") {
            entry.value = PropertyValue::String(camera_id);
        }

        // Apply pre-init settings
        self.write_exposure(self.exposure_us)?;
        let gain = self
            .props
            .get("Gain")
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        self.write_gain(gain)?;
        self.write_binning(self.binning)?;
        self.write_pixel_format(self.pixel_format)?;
        self.write_trigger_mode()?;
        if let Some(mode) = acquisition_frame_rate_mode_from_name(&self.acquisition_frame_rate_mode)
        {
            self.write_enum_feature(
                ffi::GX_ENUM_ACQUISITION_FRAME_RATE_MODE,
                mode,
                "GXSetEnum(AcquisitionFrameRateMode)",
            )?;
        }
        self.write_float_feature(
            ffi::GX_FLOAT_ACQUISITION_FRAME_RATE,
            upstream_atoi_float(self.acquisition_frame_rate),
            "GXSetFloat(AcquisitionFrameRate)",
        )?;
        self.write_float_feature(
            ffi::GX_FLOAT_TRIGGER_DELAY,
            upstream_atoi_float(self.trigger_delay),
            "GXSetFloat(TriggerDelay)",
        )?;
        self.write_float_feature(
            ffi::GX_FLOAT_TRIGGER_FILTER_RAISING,
            upstream_atoi_float(self.trigger_filter_raising_edge),
            "GXSetFloat(TriggerFilterRaisingEdge)",
        )?;
        if let Some(selector) = user_output_selector_from_name(&self.user_output_selector) {
            self.write_enum_feature(
                ffi::GX_ENUM_USER_OUTPUT_SELECTOR,
                selector,
                "GXSetEnum(UserOutputSelector)",
            )?;
        }
        for line in 0..2 {
            if let Some(mode) = line_mode_from_name(&self.line_modes[line]) {
                self.write_line_mode(line, mode)?;
            }
            if let Some(source) = line_source_from_name(&self.line_sources[line]) {
                self.write_line_source(line, source)?;
            }
        }

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
                let _ = ffi::GXStreamOff(self.handle);
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
            "Exposure(us)" => {
                if let Some(value) = self.read_float_feature(ffi::GX_FLOAT_EXPOSURE_TIME) {
                    return Ok(PropertyValue::Float(value));
                }
                Ok(PropertyValue::Float(self.exposure_us))
            }
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
            "PixelType" => self.props.get(name).cloned(),
            "Binning" => Ok(PropertyValue::Integer(
                self.read_int_feature(ffi::GX_INT_BINNING_HORIZONTAL)
                    .unwrap_or(self.binning as i64),
            )),
            "SerialNumber" => Ok(PropertyValue::String(self.serial_number.clone())),
            "CameraID" => self.props.get(name).cloned(),
            "TriggerMode" => {
                if let Some(value) = self.read_enum_feature(ffi::GX_ENUM_TRIGGER_MODE) {
                    let mode = if value == ffi::GX_TRIGGER_MODE_ON {
                        "On"
                    } else {
                        "Off"
                    };
                    return Ok(PropertyValue::String(mode.to_string()));
                }
                Ok(PropertyValue::String(self.trigger_mode.clone()))
            }
            "TriggerSource" => {
                if let Some(value) = self.read_enum_feature(ffi::GX_ENUM_TRIGGER_SOURCE) {
                    return Ok(PropertyValue::String(
                        trigger_source_name(value).to_string(),
                    ));
                }
                Ok(PropertyValue::String(self.trigger_source.clone()))
            }
            "TriggerActivation" => {
                if let Some(value) = self.read_enum_feature(ffi::GX_ENUM_TRIGGER_ACTIVATION) {
                    return Ok(PropertyValue::String(
                        trigger_activation_name(value).to_string(),
                    ));
                }
                Ok(PropertyValue::String(self.trigger_activation.clone()))
            }
            "ExposureTimeoutSeconds" => Ok(PropertyValue::Integer(self.exposure_timeout_s)),
            "AcquisitionFrameRateMode" => {
                if let Some(value) =
                    self.read_enum_feature(ffi::GX_ENUM_ACQUISITION_FRAME_RATE_MODE)
                {
                    return Ok(PropertyValue::String(
                        acquisition_frame_rate_mode_name(value).to_string(),
                    ));
                }
                Ok(PropertyValue::String(
                    self.acquisition_frame_rate_mode.clone(),
                ))
            }
            "AcquisitionFrameRate" => Ok(PropertyValue::Float(
                self.read_float_feature(ffi::GX_FLOAT_ACQUISITION_FRAME_RATE)
                    .unwrap_or(self.acquisition_frame_rate),
            )),
            "TriggerDelay" => Ok(PropertyValue::Float(
                self.read_float_feature(ffi::GX_FLOAT_TRIGGER_DELAY)
                    .unwrap_or(self.trigger_delay),
            )),
            "TriggerFilterRaisingEdge" => Ok(PropertyValue::Float(
                self.read_float_feature(ffi::GX_FLOAT_TRIGGER_FILTER_RAISING)
                    .unwrap_or(self.trigger_filter_raising_edge),
            )),
            "UserOutputSelector" => {
                if let Some(value) = self.read_enum_feature(ffi::GX_ENUM_USER_OUTPUT_SELECTOR) {
                    return Ok(PropertyValue::String(
                        user_output_selector_name(value).to_string(),
                    ));
                }
                Ok(PropertyValue::String(self.user_output_selector.clone()))
            }
            "Line0-Mode" => Ok(PropertyValue::String(
                self.read_line_mode(0)
                    .map(line_mode_name)
                    .unwrap_or(self.line_modes[0].as_str())
                    .to_string(),
            )),
            "Line1-Mode" => Ok(PropertyValue::String(
                self.read_line_mode(1)
                    .map(line_mode_name)
                    .unwrap_or(self.line_modes[1].as_str())
                    .to_string(),
            )),
            "Line0-Source" => Ok(PropertyValue::String(
                self.read_line_source(0)
                    .map(line_source_name)
                    .unwrap_or(self.line_sources[0].as_str())
                    .to_string(),
            )),
            "Line1-Source" => Ok(PropertyValue::String(
                self.read_line_source(1)
                    .map(line_source_name)
                    .unwrap_or(self.line_sources[1].as_str())
                    .to_string(),
            )),
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
                let serial_number = val.as_str().to_string();
                self.props.set(name, val)?;
                self.serial_number = serial_number;
                Ok(())
            }
            "Exposure(us)" => {
                self.require_not_capturing()?;
                let exposure_us = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                let exposure_us = validate_nonnegative_finite(exposure_us)?;
                let old_exposure_us = self.exposure_us;
                self.exposure_us = exposure_us;
                self.props.set(name, PropertyValue::Float(exposure_us))?;
                if !self.handle.is_null() {
                    if let Err(err) = self.write_exposure(self.exposure_us) {
                        self.exposure_us = old_exposure_us;
                        let _ = self.props.set(name, PropertyValue::Float(old_exposure_us));
                        return Err(err);
                    }
                }
                Ok(())
            }
            "Gain" => {
                self.require_not_capturing()?;
                let raw_gain = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !raw_gain.is_finite() {
                    return Err(MmError::InvalidPropertyValue);
                }
                let g = raw_gain.max(0.0);
                let old = self.props.get(name).cloned().ok();
                self.props.set(name, PropertyValue::Float(g))?;
                if !self.handle.is_null() {
                    if let Err(err) = self.write_gain(g) {
                        if let Some(old) = old {
                            let _ = self.props.set(name, old);
                        }
                        return Err(err);
                    }
                }
                Ok(())
            }
            "PixelType" => {
                self.require_not_capturing()?;
                let fmt_name = val.as_str().to_string();
                let pixel_format =
                    pixel_format_from_name(&fmt_name).ok_or(MmError::InvalidPropertyValue)?;
                let old_pixel_format = self.pixel_format;
                let old = self.props.get(name).cloned().ok();
                self.props.set(name, val)?;
                self.pixel_format = pixel_format;
                if !self.handle.is_null() {
                    if let Err(err) = self.write_pixel_format(self.pixel_format) {
                        self.pixel_format = old_pixel_format;
                        if let Some(old) = old {
                            let _ = self.props.set(name, old);
                        }
                        return Err(err);
                    }
                }
                self.sync_dimensions();
                Ok(())
            }
            "Binning" => {
                self.require_not_capturing()?;
                let binning = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                let old_binning = self.binning;
                let old = self.props.get(name).cloned().ok();
                self.props.set(name, PropertyValue::Integer(binning))?;
                self.binning = binning as i32;
                if !self.handle.is_null() {
                    if let Err(err) = self.write_binning(binning as i32) {
                        self.binning = old_binning;
                        if let Some(old) = old {
                            let _ = self.props.set(name, old);
                        }
                        return Err(err);
                    }
                }
                self.clear_roi().ok();
                self.sync_dimensions();
                Ok(())
            }
            "SensorWidth" => {
                self.require_not_capturing()?;
                self.check_open()?;
                let width = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if width <= 0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                let width = self.clamp_integer_to_range(ffi::GX_INT_WIDTH, width)?;
                unsafe {
                    gx_check(
                        ffi::GXSetInt(self.handle, ffi::GX_INT_WIDTH, width),
                        "GXSetInt(Width)",
                    )?;
                }
                self.sync_dimensions();
                Ok(())
            }
            "SensorHeight" => {
                self.require_not_capturing()?;
                self.check_open()?;
                let height = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if height <= 0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                let height = self.clamp_integer_to_range(ffi::GX_INT_HEIGHT, height)?;
                unsafe {
                    gx_check(
                        ffi::GXSetInt(self.handle, ffi::GX_INT_HEIGHT, height),
                        "GXSetInt(Height)",
                    )?;
                }
                self.sync_dimensions();
                Ok(())
            }
            "TriggerMode" => {
                self.require_not_capturing()?;
                let trigger_mode = val.as_str().to_string();
                let old_trigger_mode = self.trigger_mode.clone();
                let old = self.props.get(name).cloned().ok();
                self.props.set(name, val)?;
                self.trigger_mode = trigger_mode;
                if let Err(err) = self.write_trigger_mode() {
                    self.trigger_mode = old_trigger_mode;
                    if let Some(old) = old {
                        let _ = self.props.set(name, old);
                    }
                    return Err(err);
                }
                Ok(())
            }
            "TriggerSource" => {
                self.require_not_capturing()?;
                let trigger_source = val.as_str().to_string();
                let old_trigger_source = self.trigger_source.clone();
                let old = self.props.get(name).cloned().ok();
                self.props.set(name, val)?;
                self.trigger_source = trigger_source;
                if let Err(err) = self.write_trigger_mode() {
                    self.trigger_source = old_trigger_source;
                    if let Some(old) = old {
                        let _ = self.props.set(name, old);
                    }
                    return Err(err);
                }
                Ok(())
            }
            "TriggerActivation" => {
                self.require_not_capturing()?;
                let trigger_activation = val.as_str().to_string();
                let old_trigger_activation = self.trigger_activation.clone();
                let old = self.props.get(name).cloned().ok();
                self.props.set(name, val)?;
                self.trigger_activation = trigger_activation;
                if let Err(err) = self.write_trigger_mode() {
                    self.trigger_activation = old_trigger_activation;
                    if let Some(old) = old {
                        let _ = self.props.set(name, old);
                    }
                    return Err(err);
                }
                Ok(())
            }
            "ExposureTimeoutSeconds" => {
                self.require_not_capturing()?;
                let timeout_s = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if timeout_s < 0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.props.set(name, PropertyValue::Integer(timeout_s))?;
                self.exposure_timeout_s = timeout_s;
                Ok(())
            }
            "AcquisitionFrameRateMode" => {
                self.require_not_capturing()?;
                let mode_name = val.as_str().to_string();
                let mode = acquisition_frame_rate_mode_from_name(&mode_name)
                    .ok_or(MmError::InvalidPropertyValue)?;
                let old_mode = self.acquisition_frame_rate_mode.clone();
                let old = self.props.get(name).cloned().ok();
                self.props.set(name, val)?;
                self.acquisition_frame_rate_mode = mode_name;
                if let Err(err) = self.write_enum_feature(
                    ffi::GX_ENUM_ACQUISITION_FRAME_RATE_MODE,
                    mode,
                    "GXSetEnum(AcquisitionFrameRateMode)",
                ) {
                    self.acquisition_frame_rate_mode = old_mode;
                    if let Some(old) = old {
                        let _ = self.props.set(name, old);
                    }
                    return Err(err);
                }
                Ok(())
            }
            "AcquisitionFrameRate" => {
                self.require_not_capturing()?;
                let frame_rate = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                let frame_rate = validate_nonnegative_finite(frame_rate)?;
                let old_frame_rate = self.acquisition_frame_rate;
                let old = self.props.get(name).cloned().ok();
                self.props.set(name, PropertyValue::Float(frame_rate))?;
                self.acquisition_frame_rate = frame_rate;
                if let Err(err) = self.write_float_feature(
                    ffi::GX_FLOAT_ACQUISITION_FRAME_RATE,
                    upstream_atoi_float(frame_rate),
                    "GXSetFloat(AcquisitionFrameRate)",
                ) {
                    self.acquisition_frame_rate = old_frame_rate;
                    if let Some(old) = old {
                        let _ = self.props.set(name, old);
                    }
                    return Err(err);
                }
                Ok(())
            }
            "TriggerDelay" => {
                self.require_not_capturing()?;
                let trigger_delay = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                let trigger_delay = validate_nonnegative_finite(trigger_delay)?;
                let old_trigger_delay = self.trigger_delay;
                let old = self.props.get(name).cloned().ok();
                self.props.set(name, PropertyValue::Float(trigger_delay))?;
                self.trigger_delay = trigger_delay;
                if let Err(err) = self.write_float_feature(
                    ffi::GX_FLOAT_TRIGGER_DELAY,
                    upstream_atoi_float(trigger_delay),
                    "GXSetFloat(TriggerDelay)",
                ) {
                    self.trigger_delay = old_trigger_delay;
                    if let Some(old) = old {
                        let _ = self.props.set(name, old);
                    }
                    return Err(err);
                }
                Ok(())
            }
            "TriggerFilterRaisingEdge" => {
                self.require_not_capturing()?;
                let trigger_filter = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                let trigger_filter = validate_nonnegative_finite(trigger_filter)?;
                let old_trigger_filter = self.trigger_filter_raising_edge;
                let old = self.props.get(name).cloned().ok();
                self.props.set(name, PropertyValue::Float(trigger_filter))?;
                self.trigger_filter_raising_edge = trigger_filter;
                if let Err(err) = self.write_float_feature(
                    ffi::GX_FLOAT_TRIGGER_FILTER_RAISING,
                    upstream_atoi_float(trigger_filter),
                    "GXSetFloat(TriggerFilterRaisingEdge)",
                ) {
                    self.trigger_filter_raising_edge = old_trigger_filter;
                    if let Some(old) = old {
                        let _ = self.props.set(name, old);
                    }
                    return Err(err);
                }
                Ok(())
            }
            "UserOutputSelector" => {
                self.require_not_capturing()?;
                let selector_name = val.as_str().to_string();
                let selector = user_output_selector_from_name(&selector_name)
                    .ok_or(MmError::InvalidPropertyValue)?;
                let old_selector = self.user_output_selector.clone();
                let old = self.props.get(name).cloned().ok();
                self.props.set(name, val)?;
                self.user_output_selector = selector_name;
                if let Err(err) = self.write_enum_feature(
                    ffi::GX_ENUM_USER_OUTPUT_SELECTOR,
                    selector,
                    "GXSetEnum(UserOutputSelector)",
                ) {
                    self.user_output_selector = old_selector;
                    if let Some(old) = old {
                        let _ = self.props.set(name, old);
                    }
                    return Err(err);
                }
                Ok(())
            }
            "Line0-Mode" | "Line1-Mode" => {
                self.require_not_capturing()?;
                let line = if name.starts_with("Line0") { 0 } else { 1 };
                let mode_name = val.as_str().to_string();
                let mode = line_mode_from_name(&mode_name).ok_or(MmError::InvalidPropertyValue)?;
                let old_mode = self.line_modes[line].clone();
                let old = self.props.get(name).cloned().ok();
                self.props.set(name, val)?;
                self.line_modes[line] = mode_name;
                if let Err(err) = self.write_line_mode(line, mode) {
                    self.line_modes[line] = old_mode;
                    if let Some(old) = old {
                        let _ = self.props.set(name, old);
                    }
                    return Err(err);
                }
                Ok(())
            }
            "Line0-Source" | "Line1-Source" => {
                self.require_not_capturing()?;
                let line = if name.starts_with("Line0") { 0 } else { 1 };
                let source_name = val.as_str().to_string();
                let source =
                    line_source_from_name(&source_name).ok_or(MmError::InvalidPropertyValue)?;
                let old_source = self.line_sources[line].clone();
                let old = self.props.get(name).cloned().ok();
                self.props.set(name, val)?;
                self.line_sources[line] = source_name;
                if let Err(err) = self.write_line_source(line, source) {
                    self.line_sources[line] = old_source;
                    if let Some(old) = old {
                        let _ = self.props.set(name, old);
                    }
                    return Err(err);
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

// ─── Camera trait ────────────────────────────────────────────────────────────

impl Camera for DahengCamera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_open()?;
        if self.capturing {
            self.fetch_frame()?;
            if let Some(remaining) = self.sequence_remaining.as_mut() {
                *remaining -= 1;
                if *remaining <= 0 {
                    self.stop_sequence_acquisition()?;
                }
            }
            return Ok(());
        }
        unsafe {
            gx_check(ffi::GXStreamOn(self.handle), "GXStreamOn")?;
        }
        let result = self
            .send_software_trigger_if_configured()
            .and_then(|_| self.fetch_frame());
        unsafe {
            let _ = ffi::GXStreamOff(self.handle);
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
        self.components
    }
    fn get_number_of_channels(&self) -> u32 {
        1
    }
    fn get_exposure(&self) -> f64 {
        self.exposure_us / 1000.0
    }

    fn set_exposure(&mut self, exp_ms: f64) -> MmResult<()> {
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        let exposure_us = exp_ms * 1000.0;
        validate_nonnegative_finite(exposure_us)?;
        if !self.handle.is_null() {
            self.write_exposure(exposure_us)?;
        }
        self.exposure_us = exposure_us;
        self.props
            .set("Exposure(us)", PropertyValue::Float(exposure_us))?;
        Ok(())
    }

    fn get_binning(&self) -> i32 {
        self.read_int_feature(ffi::GX_INT_BINNING_HORIZONTAL)
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(self.binning)
    }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        self.require_not_capturing()?;
        let old_binning = self.binning;
        let old = self.props.get("Binning").cloned().ok();
        self.props
            .set("Binning", PropertyValue::Integer(bin as i64))?;
        self.binning = bin;
        if !self.handle.is_null() {
            if let Err(err) = self.write_binning(bin) {
                self.binning = old_binning;
                if let Some(old) = old {
                    let _ = self.props.set("Binning", old);
                }
                return Err(err);
            }
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
            gx_check(
                ffi::GXGetInt(self.handle, ffi::GX_INT_OFFSET_X, &mut x),
                "GXGetInt(OffsetX)",
            )?;
            gx_check(
                ffi::GXGetInt(self.handle, ffi::GX_INT_OFFSET_Y, &mut y),
                "GXGetInt(OffsetY)",
            )?;
            gx_check(
                ffi::GXGetInt(self.handle, ffi::GX_INT_WIDTH, &mut w),
                "GXGetInt(Width)",
            )?;
            gx_check(
                ffi::GXGetInt(self.handle, ffi::GX_INT_HEIGHT, &mut h),
                "GXGetInt(Height)",
            )?;
            Ok(ImageRoi::new(x as u32, y as u32, w as u32, h as u32))
        }
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        self.require_not_capturing()?;
        self.check_open()?;
        if roi.width == 0 && roi.height == 0 {
            return self.clear_roi();
        }
        let previous_roi = self.get_roi()?;
        let x = self.clamp_integer_to_range(ffi::GX_INT_OFFSET_X, roi.x as i64)?;
        let y = self.clamp_integer_to_range(ffi::GX_INT_OFFSET_Y, roi.y as i64)?;
        let width = self.clamp_integer_to_range(ffi::GX_INT_WIDTH, roi.width as i64)?;
        let height = self.clamp_integer_to_range(ffi::GX_INT_HEIGHT, roi.height as i64)?;
        let mut sensor_width = 0;
        let mut sensor_height = 0;
        unsafe {
            gx_check(
                ffi::GXGetInt(self.handle, ffi::GX_INT_WIDTH_MAX, &mut sensor_width),
                "GXGetInt(WidthMax)",
            )?;
            gx_check(
                ffi::GXGetInt(self.handle, ffi::GX_INT_HEIGHT_MAX, &mut sensor_height),
                "GXGetInt(HeightMax)",
            )?;
        }
        let target_roi = ImageRoi::new(x as u32, y as u32, width as u32, height as u32);
        let result =
            self.apply_roi_values_ordered(previous_roi, target_roi, sensor_width, sensor_height);
        if let Err(err) = result {
            let current_roi = self.get_roi().unwrap_or(target_roi);
            let _ = self.apply_roi_values_ordered(
                current_roi,
                previous_roi,
                sensor_width,
                sensor_height,
            );
            self.sync_dimensions();
            return Err(err);
        }
        self.sync_dimensions();
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        self.require_not_capturing()?;
        if self.handle.is_null() {
            return Ok(());
        }
        unsafe {
            let mut max_w: i64 = 0;
            let mut max_h: i64 = 0;
            gx_check(
                ffi::GXGetInt(self.handle, ffi::GX_INT_WIDTH_MAX, &mut max_w),
                "GXGetInt(WidthMax)",
            )?;
            gx_check(
                ffi::GXGetInt(self.handle, ffi::GX_INT_HEIGHT_MAX, &mut max_h),
                "GXGetInt(HeightMax)",
            )?;
            gx_check(
                ffi::GXSetInt(self.handle, ffi::GX_INT_OFFSET_X, 0),
                "GXSetInt(OffsetX)",
            )?;
            gx_check(
                ffi::GXSetInt(self.handle, ffi::GX_INT_OFFSET_Y, 0),
                "GXSetInt(OffsetY)",
            )?;
            gx_check(
                ffi::GXSetInt(self.handle, ffi::GX_INT_WIDTH, max_w),
                "GXSetInt(Width)",
            )?;
            gx_check(
                ffi::GXSetInt(self.handle, ffi::GX_INT_HEIGHT, max_h),
                "GXSetInt(Height)",
            )?;
        }
        self.sync_dimensions();
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
        self.write_trigger_mode()?;
        unsafe {
            gx_check(ffi::GXStreamOn(self.handle), "GXStreamOn")?;
        }
        if let Err(err) = self.send_software_trigger_if_configured() {
            unsafe {
                let _ = ffi::GXStreamOff(self.handle);
            }
            return Err(err);
        }
        self.capturing = true;
        self.sequence_remaining = if count > 0 { Some(count) } else { None };
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
        self.sequence_remaining = None;
        self.write_trigger_mode()?;
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
    use std::sync::{Mutex, OnceLock};

    fn daheng_stub_guard() -> Option<std::sync::MutexGuard<'static, ()>> {
        if std::env::var_os("DAHENG_STUB").is_none() {
            return None;
        }
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        Some(LOCK.get_or_init(|| Mutex::new(())).lock().unwrap())
    }

    #[test]
    fn pixel_format_helpers() {
        assert_eq!(pixel_format_bpp(ffi::GX_PIXEL_FORMAT_MONO8), 1);
        assert_eq!(pixel_format_bpp(ffi::GX_PIXEL_FORMAT_MONO16), 2);
        assert_eq!(pixel_format_bpp(ffi::GX_PIXEL_FORMAT_MONO14), 2);
        assert_eq!(pixel_format_bpp(ffi::GX_PIXEL_FORMAT_BAYER_RG12), 2);
        assert_eq!(pixel_format_bpp(ffi::GX_PIXEL_FORMAT_BAYER_BG8), 1);

        assert_eq!(pixel_format_depth(ffi::GX_PIXEL_FORMAT_MONO8), 8);
        assert_eq!(pixel_format_depth(ffi::GX_PIXEL_FORMAT_MONO12), 12);
        assert_eq!(pixel_format_depth(ffi::GX_PIXEL_FORMAT_MONO14), 14);
        assert_eq!(pixel_format_depth(ffi::GX_PIXEL_FORMAT_MONO16), 16);
        assert_eq!(pixel_format_depth(ffi::GX_PIXEL_FORMAT_BAYER_GB16), 16);

        assert_eq!(pixel_format_name(ffi::GX_PIXEL_FORMAT_MONO8), "Mono8");
        assert_eq!(
            pixel_format_name(ffi::GX_PIXEL_FORMAT_BAYER_RG8),
            "BayerRG8"
        );
        assert_eq!(
            pixel_format_name(ffi::GX_PIXEL_FORMAT_BAYER_BG12),
            "BayerBG12"
        );

        assert_eq!(
            pixel_format_from_name("Mono8"),
            Some(ffi::GX_PIXEL_FORMAT_MONO8)
        );
        assert_eq!(
            pixel_format_from_name("Mono16"),
            Some(ffi::GX_PIXEL_FORMAT_MONO16)
        );
        assert_eq!(
            pixel_format_from_name("BayerGB16"),
            Some(ffi::GX_PIXEL_FORMAT_BAYER_GB16)
        );
        assert_eq!(pixel_format_from_name("Bogus"), None);

        assert_eq!(
            captured_pixel_type_name(ffi::GX_PIXEL_FORMAT_MONO8),
            "8bit mono"
        );
        assert_eq!(
            captured_pixel_type_name(ffi::GX_PIXEL_FORMAT_MONO12),
            "16bit mono"
        );
        assert_eq!(
            captured_pixel_type_name(ffi::GX_PIXEL_FORMAT_MONO16),
            "16bit mono"
        );
        assert_eq!(
            captured_pixel_type_name(ffi::GX_PIXEL_FORMAT_BAYER_RG8),
            "8bitBGRA"
        );
        assert_eq!(
            captured_pixel_type_name(ffi::GX_PIXEL_FORMAT_BAYER_RG12),
            "8bitBGRA"
        );
    }

    #[test]
    fn bayer8_frames_expand_to_upstream_bgra_layout() {
        let bgra = bayer8_to_bgra(&[10, 20, 30, 40], 2, 2, ffi::GX_PIXEL_FORMAT_BAYER_RG8);
        assert_eq!(bgra.len(), 2 * 2 * 4);
        assert_eq!(
            &bgra[0..4],
            &[40, 25, 10, 255],
            "upstream labels converted color frames as 8bitBGRA"
        );
    }

    #[test]
    fn high_bit_bayer_frames_scale_to_bgra_layout() {
        let samples = [0x100u16, 0x200, 0x300, 0x400];
        let raw: Vec<u8> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        let bgra = bayer_to_bgra(&raw, 2, 2, ffi::GX_PIXEL_FORMAT_BAYER_RG12);
        assert_eq!(bgra.len(), 2 * 2 * 4);
        assert_eq!(&bgra[0..4], &[0x40, 0x28, 0x10, 255]);
    }

    #[test]
    fn default_properties() {
        let d = DahengCamera::new();
        assert_eq!(d.device_type(), DeviceType::Camera);
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(
            d.get_property("Exposure(us)").unwrap(),
            PropertyValue::Float(10_000.0)
        );
        assert_eq!(d.get_binning(), 1);
        assert!(!d.is_capturing());
        assert!(d.has_property("CameraID"));
        assert!(d.has_property("SensorWidth"));
        assert!(d.has_property("SensorHeight"));
        assert!(d.is_property_read_only("CameraID"));
        assert_eq!(
            d.get_property("CameraID").unwrap(),
            PropertyValue::String(String::new())
        );
    }

    #[test]
    fn set_serial_number_pre_init() {
        let mut d = DahengCamera::new();
        d.set_property("SerialNumber", PropertyValue::String("ABC123".into()))
            .unwrap();
        assert_eq!(d.serial_number, "ABC123");
    }

    #[test]
    fn discovered_serials_populate_allowed_values_and_default() {
        let mut d = DahengCamera::new();
        let serials = vec!["SN001".to_string(), "SN002".to_string()];
        d.apply_discovered_serial_numbers(&serials);

        assert_eq!(d.serial_number, "SN001");
        assert_eq!(
            d.get_property("SerialNumber").unwrap(),
            PropertyValue::String("SN001".into())
        );
        assert_eq!(
            d.props.entry("SerialNumber").unwrap().allowed_values,
            vec!["SN001".to_string(), "SN002".to_string()]
        );
        assert!(d
            .set_property("SerialNumber", PropertyValue::String("OTHER".into()))
            .is_err());
        assert_eq!(d.serial_number, "SN001");
        assert_eq!(
            d.get_property("SerialNumber").unwrap(),
            PropertyValue::String("SN001".into())
        );
        d.set_property("SerialNumber", PropertyValue::String("SN002".into()))
            .unwrap();
        assert_eq!(d.serial_number, "SN002");
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = DahengCamera::new();
        d.set_property("Exposure(us)", PropertyValue::Float(25_000.0))
            .unwrap();
        assert_eq!(d.exposure_us, 25_000.0);
        assert_eq!(d.get_exposure(), 25.0);
        assert_eq!(
            d.get_property("Exposure(us)").unwrap(),
            PropertyValue::Float(25_000.0)
        );
        d.set_exposure(12.5);
        assert_eq!(d.exposure_us, 12_500.0);
        assert_eq!(
            d.get_property("Exposure(us)").unwrap(),
            PropertyValue::Float(12_500.0)
        );
    }

    #[test]
    fn negative_exposure_is_rejected_before_cache_mutation() {
        let mut d = DahengCamera::new();
        assert!(d
            .set_property("Exposure(us)", PropertyValue::Float(-1.0))
            .is_err());
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(
            d.get_property("Exposure(us)").unwrap(),
            PropertyValue::Float(10_000.0)
        );

        d.set_exposure(-1.0);
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(
            d.get_property("Exposure(us)").unwrap(),
            PropertyValue::Float(10_000.0)
        );
    }

    #[test]
    fn trigger_properties_are_present() {
        let mut d = DahengCamera::new();
        d.set_property("TriggerMode", PropertyValue::String("On".into()))
            .unwrap();
        assert_eq!(
            d.get_property("TriggerMode").unwrap(),
            PropertyValue::String("On".into())
        );
        assert_eq!(
            d.get_property("TriggerSource").unwrap(),
            PropertyValue::String("Software".into())
        );
        assert_eq!(
            d.get_property("TriggerActivation").unwrap(),
            PropertyValue::String("RisingEdge".into())
        );
        d.set_property(
            "TriggerActivation",
            PropertyValue::String("FallingEdge".into()),
        )
        .unwrap();
        assert_eq!(
            d.get_property("TriggerActivation").unwrap(),
            PropertyValue::String("FallingEdge".into())
        );
    }

    #[test]
    fn upstream_scalar_properties_are_present_and_cached_pre_init() {
        let mut d = DahengCamera::new();
        assert!(d.has_property("AcquisitionFrameRateMode"));
        assert!(d.has_property("AcquisitionFrameRate"));
        assert!(d.has_property("TriggerDelay"));
        assert!(d.has_property("TriggerFilterRaisingEdge"));
        assert!(d.has_property("UserOutputSelector"));

        d.set_property(
            "AcquisitionFrameRateMode",
            PropertyValue::String("On".into()),
        )
        .unwrap();
        d.set_property("AcquisitionFrameRate", PropertyValue::Float(42.5))
            .unwrap();
        d.set_property("TriggerDelay", PropertyValue::Float(12.5))
            .unwrap();
        d.set_property("TriggerFilterRaisingEdge", PropertyValue::Float(3.25))
            .unwrap();
        d.set_property(
            "UserOutputSelector",
            PropertyValue::String("UserOutput2".into()),
        )
        .unwrap();

        assert_eq!(
            d.get_property("AcquisitionFrameRateMode").unwrap(),
            PropertyValue::String("On".into())
        );
        assert_eq!(
            d.get_property("AcquisitionFrameRate").unwrap(),
            PropertyValue::Float(42.5)
        );
        assert_eq!(
            d.get_property("TriggerDelay").unwrap(),
            PropertyValue::Float(12.5)
        );
        assert_eq!(
            d.get_property("TriggerFilterRaisingEdge").unwrap(),
            PropertyValue::Float(3.25)
        );
        assert_eq!(
            d.get_property("UserOutputSelector").unwrap(),
            PropertyValue::String("UserOutput2".into())
        );
    }

    #[test]
    fn line_properties_are_present_and_cached_pre_init() {
        let mut d = DahengCamera::new();
        assert!(d.has_property("Line0-Mode"));
        assert!(d.has_property("Line0-Source"));
        assert!(d.has_property("Line1-Mode"));
        assert!(d.has_property("Line1-Source"));

        d.set_property("Line0-Mode", PropertyValue::String("Output".into()))
            .unwrap();
        d.set_property("Line0-Source", PropertyValue::String("UserOutput0".into()))
            .unwrap();
        d.set_property(
            "Line1-Source",
            PropertyValue::String("ExposureActive".into()),
        )
        .unwrap();

        assert_eq!(
            d.get_property("Line0-Mode").unwrap(),
            PropertyValue::String("Output".into())
        );
        assert_eq!(
            d.get_property("Line0-Source").unwrap(),
            PropertyValue::String("UserOutput0".into())
        );
        assert_eq!(
            d.get_property("Line1-Mode").unwrap(),
            PropertyValue::String("Input".into())
        );
        assert_eq!(
            d.get_property("Line1-Source").unwrap(),
            PropertyValue::String("ExposureActive".into())
        );

        assert!(d
            .set_property("Line0-Mode", PropertyValue::String("TriState".into()))
            .is_err());
        assert_eq!(
            d.get_property("Line0-Mode").unwrap(),
            PropertyValue::String("Output".into())
        );
    }

    #[test]
    fn invalid_upstream_scalar_enum_values_do_not_mutate_cached_state() {
        let mut d = DahengCamera::new();
        assert!(d
            .set_property(
                "AcquisitionFrameRateMode",
                PropertyValue::String("Auto".into()),
            )
            .is_err());
        assert_eq!(
            d.get_property("AcquisitionFrameRateMode").unwrap(),
            PropertyValue::String("Off".into())
        );
        assert!(d
            .set_property(
                "UserOutputSelector",
                PropertyValue::String("UserOutput3".into()),
            )
            .is_err());
        assert_eq!(
            d.get_property("UserOutputSelector").unwrap(),
            PropertyValue::String("UserOutput0".into())
        );
    }

    #[test]
    fn negative_upstream_scalar_values_do_not_mutate_cached_state() {
        let mut d = DahengCamera::new();

        assert!(d
            .set_property("AcquisitionFrameRate", PropertyValue::Float(-1.0))
            .is_err());
        assert_eq!(
            d.get_property("AcquisitionFrameRate").unwrap(),
            PropertyValue::Float(0.0)
        );
        assert!(d
            .set_property("TriggerDelay", PropertyValue::Float(-1.0))
            .is_err());
        assert_eq!(
            d.get_property("TriggerDelay").unwrap(),
            PropertyValue::Float(0.0)
        );
        assert!(d
            .set_property("TriggerFilterRaisingEdge", PropertyValue::Float(-1.0))
            .is_err());
        assert_eq!(
            d.get_property("TriggerFilterRaisingEdge").unwrap(),
            PropertyValue::Float(0.0)
        );
    }

    #[test]
    fn nonfinite_float_values_do_not_mutate_cached_state() {
        let mut d = DahengCamera::new();

        assert!(d
            .set_property("Exposure(us)", PropertyValue::Float(f64::NAN))
            .is_err());
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(
            d.get_property("Exposure(us)").unwrap(),
            PropertyValue::Float(10_000.0)
        );
        d.set_exposure(f64::INFINITY);
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(
            d.get_property("Exposure(us)").unwrap(),
            PropertyValue::Float(10_000.0)
        );

        assert!(d
            .set_property("Gain", PropertyValue::Float(f64::NAN))
            .is_err());
        assert_eq!(d.get_property("Gain").unwrap(), PropertyValue::Float(0.0));
        assert!(d
            .set_property("AcquisitionFrameRate", PropertyValue::Float(f64::INFINITY))
            .is_err());
        assert_eq!(
            d.get_property("AcquisitionFrameRate").unwrap(),
            PropertyValue::Float(0.0)
        );
        assert!(d
            .set_property("TriggerDelay", PropertyValue::Float(f64::NAN))
            .is_err());
        assert_eq!(
            d.get_property("TriggerDelay").unwrap(),
            PropertyValue::Float(0.0)
        );
        assert!(d
            .set_property(
                "TriggerFilterRaisingEdge",
                PropertyValue::Float(f64::INFINITY)
            )
            .is_err());
        assert_eq!(
            d.get_property("TriggerFilterRaisingEdge").unwrap(),
            PropertyValue::Float(0.0)
        );
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

    #[test]
    fn invalid_pixel_format_is_rejected() {
        let mut d = DahengCamera::new();
        assert!(d
            .set_property("PixelType", PropertyValue::String("Bogus".into()))
            .is_err());
        assert_eq!(
            d.get_property("PixelType").unwrap(),
            PropertyValue::String("Mono8".into())
        );
    }

    #[test]
    fn invalid_allowed_values_do_not_mutate_cached_state() {
        let mut d = DahengCamera::new();
        assert!(d
            .set_property("Binning", PropertyValue::Integer(3))
            .is_err());
        assert_eq!(d.get_binning(), 1);
        assert_eq!(
            d.get_property("Binning").unwrap(),
            PropertyValue::Integer(1)
        );
        assert!(d.set_binning(3).is_err());
        assert_eq!(d.get_binning(), 1);
        assert_eq!(
            d.get_property("Binning").unwrap(),
            PropertyValue::Integer(1)
        );
        assert!(d
            .set_property("Binning", PropertyValue::Integer(i64::from(i32::MAX) + 2))
            .is_err());
        assert_eq!(d.get_binning(), 1);
        assert_eq!(
            d.get_property("Binning").unwrap(),
            PropertyValue::Integer(1)
        );
        assert!(d
            .set_property("TriggerMode", PropertyValue::String("Pulse".into()))
            .is_err());
        assert_eq!(
            d.get_property("TriggerMode").unwrap(),
            PropertyValue::String("Off".into())
        );
        assert!(d
            .set_property("TriggerSource", PropertyValue::String("Line1".into()))
            .is_ok());
        assert_eq!(
            d.get_property("TriggerSource").unwrap(),
            PropertyValue::String("Line1".into())
        );
        d.set_property("TriggerSource", PropertyValue::String("Software".into()))
            .unwrap();
        assert_eq!(
            d.get_property("TriggerSource").unwrap(),
            PropertyValue::String("Software".into())
        );
        assert_eq!(
            d.get_property("ExposureTimeoutSeconds").unwrap(),
            PropertyValue::Integer(5)
        );
        d.set_property("ExposureTimeoutSeconds", PropertyValue::Integer(7))
            .unwrap();
        assert_eq!(
            d.get_property("ExposureTimeoutSeconds").unwrap(),
            PropertyValue::Integer(7)
        );
        assert!(d
            .set_property("TriggerActivation", PropertyValue::String("AnyEdge".into()))
            .is_err());
        assert_eq!(
            d.get_property("TriggerActivation").unwrap(),
            PropertyValue::String("RisingEdge".into())
        );
    }

    #[test]
    fn exposure_timeout_rejects_negative_values() {
        let mut d = DahengCamera::new();
        assert!(d
            .set_property("ExposureTimeoutSeconds", PropertyValue::Integer(-1))
            .is_err());
        assert_eq!(d.exposure_timeout_s, 5);
    }

    #[test]
    fn exposure_timeout_milliseconds_saturates_before_ffi() {
        assert_eq!(exposure_timeout_ms(-1), 0);
        assert_eq!(exposure_timeout_ms(5), 5_000);
        assert_eq!(exposure_timeout_ms(i64::MAX), u32::MAX);
    }

    #[test]
    fn upstream_atoi_float_truncates_fractional_sdk_writes() {
        assert_eq!(upstream_atoi_float(7.5), 7.0);
        assert_eq!(upstream_atoi_float(3.25), 3.0);
        assert_eq!(upstream_atoi_float(42.0), 42.0);
    }

    #[test]
    fn line_source_values_match_gxiapi_header() {
        assert_eq!(ffi::GX_LINE_SOURCE_OFF, 0);
        assert_eq!(ffi::GX_LINE_SOURCE_USER_OUTPUT0, 2);
        assert_eq!(ffi::GX_LINE_SOURCE_EXPOSURE_ACTIVE, 5);
    }

    #[test]
    fn frame_raw_size_validation_rejects_bad_sdk_metadata() {
        assert_eq!(
            validated_frame_raw_size(64, 32, ffi::GX_PIXEL_FORMAT_MONO8, 64 * 32).unwrap(),
            64 * 32
        );
        assert_eq!(
            validated_frame_raw_size(64, 32, ffi::GX_PIXEL_FORMAT_MONO12, 64 * 32 * 2).unwrap(),
            64 * 32 * 2
        );

        assert!(validated_frame_raw_size(0, 32, ffi::GX_PIXEL_FORMAT_MONO8, 0).is_err());
        assert!(validated_frame_raw_size(64, -1, ffi::GX_PIXEL_FORMAT_MONO8, 64).is_err());
        assert!(validated_frame_raw_size(64, 32, ffi::GX_PIXEL_FORMAT_MONO8, -1).is_err());
        assert!(validated_frame_raw_size(64, 32, ffi::GX_PIXEL_FORMAT_MONO12, 64 * 32).is_err());
    }

    #[test]
    fn invalid_trigger_source_does_not_mutate_cached_state() {
        let mut d = DahengCamera::new();
        assert!(d
            .set_property("TriggerSource", PropertyValue::String("Input0".into()))
            .is_err());
        assert_eq!(
            d.get_property("TriggerSource").unwrap(),
            PropertyValue::String("Software".into())
        );
    }

    #[test]
    fn zero_sized_roi_requires_connected_clear_path() {
        let mut d = DahengCamera::new();
        assert_eq!(
            d.set_roi(ImageRoi::new(0, 0, 0, 0)).unwrap_err(),
            MmError::NotConnected
        );
    }

    #[test]
    fn acquisition_rejects_setting_changes_before_state_mutation() {
        let mut d = DahengCamera::new();
        d.capturing = true;

        assert_eq!(
            d.set_property("Exposure(us)", PropertyValue::Float(25_000.0))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(
            d.set_property("TriggerMode", PropertyValue::String("On".into()))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(
            d.get_property("TriggerMode").unwrap(),
            PropertyValue::String("Off".into())
        );
        assert_eq!(
            d.set_property(
                "TriggerActivation",
                PropertyValue::String("FallingEdge".into()),
            )
            .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(
            d.get_property("TriggerActivation").unwrap(),
            PropertyValue::String("RisingEdge".into())
        );
    }

    #[test]
    fn integer_range_rounding_matches_upstream_increment_rule() {
        let range = ffi::GxIntRange {
            min: 4,
            max: 100,
            inc: 6,
        };
        assert_eq!(
            DahengCamera::round_to_integer_range(range, 3),
            4,
            "below-min values clamp to the SDK minimum"
        );
        assert_eq!(
            DahengCamera::round_to_integer_range(range, 25),
            24,
            "values round down to the nearest increment from zero, matching upstream"
        );
        assert_eq!(
            DahengCamera::round_to_integer_range(range, 103),
            100,
            "above-max values clamp to the SDK maximum"
        );
    }

    #[test]
    fn stub_initialized_camera_exercises_sdk_backed_paths() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        d.set_property("Exposure(us)", PropertyValue::Float(25_000.0))
            .unwrap();
        d.set_property("PixelType", PropertyValue::String("BayerRG8".into()))
            .unwrap();
        d.set_property("TriggerMode", PropertyValue::String("On".into()))
            .unwrap();
        d.set_property("TriggerSource", PropertyValue::String("Software".into()))
            .unwrap();
        d.set_property(
            "AcquisitionFrameRateMode",
            PropertyValue::String("On".into()),
        )
        .unwrap();
        d.set_property("AcquisitionFrameRate", PropertyValue::Float(42.0))
            .unwrap();
        d.set_property("TriggerDelay", PropertyValue::Float(7.5))
            .unwrap();
        d.set_property("TriggerFilterRaisingEdge", PropertyValue::Float(3.25))
            .unwrap();
        d.set_property(
            "UserOutputSelector",
            PropertyValue::String("UserOutput2".into()),
        )
        .unwrap();

        d.initialize().unwrap();
        assert_eq!(
            d.get_property("CameraID").unwrap(),
            PropertyValue::String("GX-STUB-001".into())
        );
        assert_eq!(d.get_exposure(), 25.0);
        assert_eq!(
            d.get_property("PixelType").unwrap(),
            PropertyValue::String("BayerRG8".into())
        );
        assert_eq!(
            d.get_property("AcquisitionFrameRateMode").unwrap(),
            PropertyValue::String("On".into())
        );
        assert_eq!(
            d.get_property("AcquisitionFrameRate").unwrap(),
            PropertyValue::Float(42.0)
        );
        assert_eq!(
            d.get_property("TriggerDelay").unwrap(),
            PropertyValue::Float(7.0)
        );
        assert_eq!(
            d.get_property("TriggerFilterRaisingEdge").unwrap(),
            PropertyValue::Float(3.0)
        );
        assert_eq!(
            d.get_property("UserOutputSelector").unwrap(),
            PropertyValue::String("UserOutput2".into())
        );

        d.set_roi(ImageRoi::new(5, 9, 101, 65)).unwrap();
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(4, 8, 100, 64));
        assert_eq!(
            d.get_property("Width").unwrap(),
            PropertyValue::Integer(100)
        );
        assert_eq!(
            d.get_property("Height").unwrap(),
            PropertyValue::Integer(64)
        );
        assert_eq!(
            d.get_property("SensorWidth").unwrap(),
            PropertyValue::Integer(100)
        );
        assert_eq!(
            d.get_property("SensorHeight").unwrap(),
            PropertyValue::Integer(64)
        );

        d.set_roi(ImageRoi::new(8, 12, 200, 128)).unwrap();
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(8, 12, 200, 128));
        d.set_roi(ImageRoi::new(0, 0, 640, 480)).unwrap();
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(0, 0, 640, 480));
        d.set_roi(ImageRoi::new(8, 12, 200, 128)).unwrap();
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(8, 12, 200, 128));
        d.snap_image().unwrap();
        assert_eq!(d.get_image_width(), 200);
        assert_eq!(d.get_image_height(), 128);
        assert_eq!(d.get_image_bytes_per_pixel(), 4);
        assert_eq!(d.get_number_of_components(), 4);
        assert_eq!(d.get_bit_depth(), 8);
        assert_eq!(d.get_image_buffer().unwrap().len(), 200 * 128 * 4);
        assert_eq!(
            d.get_property("PixelType").unwrap(),
            PropertyValue::String("8bitBGRA".into())
        );
        assert_eq!(
            d.get_property("SensorWidth").unwrap(),
            PropertyValue::Integer(200)
        );
        assert_eq!(
            d.get_property("SensorHeight").unwrap(),
            PropertyValue::Integer(128)
        );

        assert!(d.set_roi(ImageRoi::new(600, 440, 100, 64)).is_err());
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(8, 12, 200, 128));

        d.set_roi(ImageRoi::new(500, 0, 100, 100)).unwrap();
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(500, 0, 100, 100));
        assert!(d.set_roi(ImageRoi::new(0, 440, 200, 64)).is_err());
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(500, 0, 100, 100));

        d.clear_roi().unwrap();
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(0, 0, 640, 480));
        d.shutdown().unwrap();
    }

    #[test]
    fn stub_fractional_upstream_scalars_are_truncated_on_sdk_write() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        d.initialize().unwrap();

        d.set_property("AcquisitionFrameRate", PropertyValue::Float(42.75))
            .unwrap();
        d.set_property("TriggerDelay", PropertyValue::Float(7.5))
            .unwrap();
        d.set_property("TriggerFilterRaisingEdge", PropertyValue::Float(3.25))
            .unwrap();

        assert_eq!(
            d.get_property("AcquisitionFrameRate").unwrap(),
            PropertyValue::Float(42.0)
        );
        assert_eq!(
            d.get_property("TriggerDelay").unwrap(),
            PropertyValue::Float(7.0)
        );
        assert_eq!(
            d.get_property("TriggerFilterRaisingEdge").unwrap(),
            PropertyValue::Float(3.0)
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_mono12_snap_reports_upstream_pixel_type_label() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        d.set_property("PixelType", PropertyValue::String("Mono12".into()))
            .unwrap();

        d.initialize().unwrap();
        d.set_roi(ImageRoi::new(0, 0, 64, 32)).unwrap();
        d.snap_image().unwrap();

        assert_eq!(d.get_image_width(), 64);
        assert_eq!(d.get_image_height(), 32);
        assert_eq!(d.get_image_bytes_per_pixel(), 2);
        assert_eq!(d.get_number_of_components(), 1);
        assert_eq!(d.get_bit_depth(), 12);
        assert_eq!(d.get_image_buffer().unwrap().len(), 64 * 32 * 2);
        assert_eq!(
            d.get_property("PixelType").unwrap(),
            PropertyValue::String("16bit mono".into())
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_mono14_snap_uses_advertised_two_byte_format() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        d.set_property("PixelType", PropertyValue::String("Mono14".into()))
            .unwrap();

        d.initialize().unwrap();
        d.set_roi(ImageRoi::new(0, 0, 64, 32)).unwrap();
        d.snap_image().unwrap();

        assert_eq!(d.get_image_width(), 64);
        assert_eq!(d.get_image_height(), 32);
        assert_eq!(d.get_image_bytes_per_pixel(), 2);
        assert_eq!(d.get_number_of_components(), 1);
        assert_eq!(d.get_bit_depth(), 14);
        assert_eq!(d.get_image_buffer().unwrap().len(), 64 * 32 * 2);
        assert_eq!(
            d.get_property("PixelType").unwrap(),
            PropertyValue::String("16bit mono".into())
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_bayer12_snap_expands_to_upstream_bgra_layout() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        d.set_property("PixelType", PropertyValue::String("BayerRG12".into()))
            .unwrap();

        d.initialize().unwrap();
        d.set_roi(ImageRoi::new(0, 0, 64, 32)).unwrap();
        d.snap_image().unwrap();

        assert_eq!(d.get_image_width(), 64);
        assert_eq!(d.get_image_height(), 32);
        assert_eq!(d.get_image_bytes_per_pixel(), 4);
        assert_eq!(d.get_number_of_components(), 4);
        assert_eq!(d.get_bit_depth(), 8);
        assert_eq!(d.get_image_buffer().unwrap().len(), 64 * 32 * 4);
        assert_eq!(
            d.get_property("PixelType").unwrap(),
            PropertyValue::String("8bitBGRA".into())
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_bayer_metadata_matches_upstream_before_snap() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        d.set_property("PixelType", PropertyValue::String("BayerRG12".into()))
            .unwrap();

        d.initialize().unwrap();

        assert_eq!(d.get_image_bytes_per_pixel(), 4);
        assert_eq!(d.get_number_of_components(), 4);
        assert_eq!(d.get_bit_depth(), 8);
        assert_eq!(
            d.get_property("PixelType").unwrap(),
            PropertyValue::String("BayerRG12".into())
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_finite_sequence_stops_after_requested_frames() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        d.initialize().unwrap();

        assert_eq!(
            d.start_sequence_acquisition(-1, 0.0).unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert!(!d.is_capturing());

        d.start_sequence_acquisition(2, 0.0).unwrap();
        assert!(d.is_capturing());
        assert_eq!(d.sequence_remaining, Some(2));
        assert_eq!(
            d.start_sequence_acquisition(1, 0.0).unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(d.sequence_remaining, Some(2));

        d.snap_image().unwrap();
        assert!(d.is_capturing());
        assert_eq!(d.sequence_remaining, Some(1));

        d.snap_image().unwrap();
        assert!(!d.is_capturing());
        assert_eq!(d.sequence_remaining, None);

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_line_properties_are_line_specific_and_restore_selector() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        d.set_property("Line0-Mode", PropertyValue::String("Output".into()))
            .unwrap();
        d.set_property("Line0-Source", PropertyValue::String("UserOutput0".into()))
            .unwrap();
        d.set_property("Line1-Mode", PropertyValue::String("Input".into()))
            .unwrap();
        d.set_property(
            "Line1-Source",
            PropertyValue::String("ExposureActive".into()),
        )
        .unwrap();

        d.initialize().unwrap();

        assert_eq!(
            d.get_property("Line0-Mode").unwrap(),
            PropertyValue::String("Output".into())
        );
        assert_eq!(
            d.get_property("Line0-Source").unwrap(),
            PropertyValue::String("UserOutput0".into())
        );
        assert_eq!(
            d.get_property("Line1-Mode").unwrap(),
            PropertyValue::String("Input".into())
        );
        assert_eq!(
            d.get_property("Line1-Source").unwrap(),
            PropertyValue::String("ExposureActive".into())
        );

        d.set_property("Line1-Source", PropertyValue::String("Off".into()))
            .unwrap();
        assert_eq!(
            d.get_property("Line0-Source").unwrap(),
            PropertyValue::String("UserOutput0".into())
        );
        assert_eq!(
            d.get_property("Line1-Source").unwrap(),
            PropertyValue::String("Off".into())
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_gain_clamps_negative_values_to_sdk_minimum() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        d.set_property("Gain", PropertyValue::Float(-2.5)).unwrap();
        assert_eq!(d.get_property("Gain").unwrap(), PropertyValue::Float(0.0));

        d.initialize().unwrap();
        assert_eq!(d.get_property("Gain").unwrap(), PropertyValue::Float(0.0));

        d.set_property("Gain", PropertyValue::Float(3.5)).unwrap();
        assert_eq!(d.get_property("Gain").unwrap(), PropertyValue::Float(3.5));
        d.set_property("Gain", PropertyValue::Float(-1.0)).unwrap();
        assert_eq!(d.get_property("Gain").unwrap(), PropertyValue::Float(0.0));

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_set_exposure_does_not_mutate_cache_when_sdk_rejects_value() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        d.initialize().unwrap();

        d.set_exposure(12.5);
        assert_eq!(d.get_exposure(), 12.5);
        assert_eq!(
            d.get_property("Exposure(us)").unwrap(),
            PropertyValue::Float(12_500.0)
        );

        d.set_exposure(-1.0);
        assert_eq!(d.get_exposure(), 12.5);
        assert_eq!(
            d.get_property("Exposure(us)").unwrap(),
            PropertyValue::Float(12_500.0)
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_exposure_property_reads_sdk_value_before_get() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        d.initialize().unwrap();

        unsafe {
            gx_check(
                ffi::GXSetFloat(d.handle, ffi::GX_FLOAT_EXPOSURE_TIME, 22_000.0),
                "GXSetFloat(ExposureTime)",
            )
            .unwrap();
        }

        assert_eq!(
            d.get_property("Exposure(us)").unwrap(),
            PropertyValue::Float(22_000.0)
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_binning_property_and_trait_read_sdk_value_before_get() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        d.initialize().unwrap();

        unsafe {
            gx_check(
                ffi::GXSetInt(d.handle, ffi::GX_INT_BINNING_HORIZONTAL, 2),
                "GXSetInt(BinningHorizontal)",
            )
            .unwrap();
        }

        assert_eq!(
            d.get_property("Binning").unwrap(),
            PropertyValue::Integer(2)
        );
        assert_eq!(d.get_binning(), 2);

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_trigger_properties_read_sdk_values_before_get() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        d.initialize().unwrap();

        unsafe {
            gx_check(
                ffi::GXSetEnum(d.handle, ffi::GX_ENUM_TRIGGER_MODE, ffi::GX_TRIGGER_MODE_ON),
                "GXSetEnum(TriggerMode)",
            )
            .unwrap();
            gx_check(
                ffi::GXSetEnum(
                    d.handle,
                    ffi::GX_ENUM_TRIGGER_SOURCE,
                    ffi::GX_TRIGGER_SOURCE_LINE2,
                ),
                "GXSetEnum(TriggerSource)",
            )
            .unwrap();
            gx_check(
                ffi::GXSetEnum(
                    d.handle,
                    ffi::GX_ENUM_TRIGGER_ACTIVATION,
                    ffi::GX_TRIGGER_ACTIVATION_FALLING_EDGE,
                ),
                "GXSetEnum(TriggerActivation)",
            )
            .unwrap();
        }

        assert_eq!(
            d.get_property("TriggerMode").unwrap(),
            PropertyValue::String("On".into())
        );
        assert_eq!(
            d.get_property("TriggerSource").unwrap(),
            PropertyValue::String("Line2".into())
        );
        assert_eq!(
            d.get_property("TriggerActivation").unwrap(),
            PropertyValue::String("FallingEdge".into())
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_rejected_negative_exposure_does_not_poison_initialize() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        assert!(d
            .set_property("Exposure(us)", PropertyValue::Float(-1.0))
            .is_err());
        d.initialize().unwrap();
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(
            d.get_property("Exposure(us)").unwrap(),
            PropertyValue::Float(10_000.0)
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_rejected_negative_upstream_scalars_do_not_poison_initialize() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        assert!(d
            .set_property("AcquisitionFrameRate", PropertyValue::Float(-1.0))
            .is_err());
        assert!(d
            .set_property("TriggerDelay", PropertyValue::Float(-1.0))
            .is_err());
        assert!(d
            .set_property("TriggerFilterRaisingEdge", PropertyValue::Float(-1.0))
            .is_err());

        d.initialize().unwrap();
        assert_eq!(
            d.get_property("AcquisitionFrameRate").unwrap(),
            PropertyValue::Float(0.0)
        );
        assert_eq!(
            d.get_property("TriggerDelay").unwrap(),
            PropertyValue::Float(0.0)
        );
        assert_eq!(
            d.get_property("TriggerFilterRaisingEdge").unwrap(),
            PropertyValue::Float(0.0)
        );

        d.shutdown().unwrap();
    }

    #[test]
    fn stub_rejects_nonfinite_float_values_without_cache_mutation() {
        let Some(_guard) = daheng_stub_guard() else {
            return;
        };

        let mut d = DahengCamera::new();
        d.initialize().unwrap();

        d.set_property("Exposure(us)", PropertyValue::Float(12_500.0))
            .unwrap();
        d.set_property("Gain", PropertyValue::Float(2.0)).unwrap();
        d.set_property("AcquisitionFrameRate", PropertyValue::Float(41.0))
            .unwrap();
        d.set_property("TriggerDelay", PropertyValue::Float(7.0))
            .unwrap();
        d.set_property("TriggerFilterRaisingEdge", PropertyValue::Float(3.0))
            .unwrap();

        assert!(d
            .set_property("Exposure(us)", PropertyValue::Float(f64::NAN))
            .is_err());
        assert!(d
            .set_property("Gain", PropertyValue::Float(f64::INFINITY))
            .is_err());
        assert!(d
            .set_property("AcquisitionFrameRate", PropertyValue::Float(f64::NAN))
            .is_err());
        assert!(d
            .set_property("TriggerDelay", PropertyValue::Float(f64::INFINITY))
            .is_err());
        assert!(d
            .set_property("TriggerFilterRaisingEdge", PropertyValue::Float(f64::NAN))
            .is_err());

        assert_eq!(
            d.get_property("Exposure(us)").unwrap(),
            PropertyValue::Float(12_500.0)
        );
        assert_eq!(d.get_property("Gain").unwrap(), PropertyValue::Float(2.0));
        assert_eq!(
            d.get_property("AcquisitionFrameRate").unwrap(),
            PropertyValue::Float(41.0)
        );
        assert_eq!(
            d.get_property("TriggerDelay").unwrap(),
            PropertyValue::Float(7.0)
        );
        assert_eq!(
            d.get_property("TriggerFilterRaisingEdge").unwrap(),
            PropertyValue::Float(3.0)
        );

        d.shutdown().unwrap();
    }
}
