use std::collections::BTreeSet;
use std::ffi::{CStr, CString};
use std::sync::Arc;

use crate::circular_buffer::ImageFrame;
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Camera, Device, SequenceImageSink};
use crate::types::{DeviceType, ImageRoi, PropertyValue};

use super::ffi;

// SAFETY: Andor3Camera holds a raw pointer to Andor3Ctx.  The SDK is not
// internally thread-safe per handle; `&mut self` enforces single-thread access.
unsafe impl Send for Andor3Camera {}

const BUF: usize = 256;
const VALS_BUF: usize = 2048;
const DEFAULT_SNAP_TIMEOUT_MS: i64 = 5_000;
const SRRF_UNAVAILABLE_STATUS: &str = "SRRF library/API not available in this Rust adapter";
const AT_ERR_HARDWARE_OVERFLOW: i64 = 100;

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

fn read_str<F: FnOnce(*mut i8, i32) -> i32>(f: F) -> Option<String> {
    let mut buf = [0i8; BUF];
    if f(buf.as_mut_ptr(), BUF as i32) != 0 {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(buf.as_ptr()) }
            .to_string_lossy()
            .into_owned(),
    )
}

fn read_enum(ctx: *mut ffi::Andor3Ctx, feature: &str) -> Option<String> {
    let feat = cstr(feature);
    read_str(|b, l| unsafe { ffi::andor3_get_enum(ctx, feat.as_ptr(), b, l) })
}

fn enum_values(ctx: *mut ffi::Andor3Ctx, feature: &str) -> Vec<String> {
    let feat = cstr(feature);
    let mut buf = vec![0i8; VALS_BUF];
    let rc =
        unsafe { ffi::andor3_enum_values(ctx, feat.as_ptr(), buf.as_mut_ptr(), VALS_BUF as i32) };
    if rc <= 0 {
        return vec![];
    }
    let s = unsafe { CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    s.split('\n')
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

fn sdk_enum_feature_for_property(prop: &str) -> &str {
    match prop {
        "Binning" => "AOIBinning",
        "PixelReadoutRate" => "PixelReadoutRate",
        "Sensitivity/DynamicRange" => "SimplePreAmpGainControl",
        _ => prop,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FeatureKind {
    Enum,
    Float,
    Integer,
    Bool,
}

#[derive(Clone, Copy)]
struct FeatureSpec {
    prop: &'static str,
    primary_feature: &'static str,
    fallback_feature: Option<&'static str>,
    kind: FeatureKind,
    read_only: bool,
}

const SIMPLE_FEATURES: &[FeatureSpec] = &[
    FeatureSpec {
        prop: "PixelReadoutRate",
        primary_feature: "PixelReadoutRateMapper",
        fallback_feature: Some("PixelReadoutRate"),
        kind: FeatureKind::Enum,
        read_only: false,
    },
    FeatureSpec {
        prop: "ElectronicShutteringMode",
        primary_feature: "ElectronicShutteringMode",
        fallback_feature: None,
        kind: FeatureKind::Enum,
        read_only: false,
    },
    FeatureSpec {
        prop: "Sensitivity/DynamicRange",
        primary_feature: "GainMode",
        fallback_feature: Some("SimplePreAmpGainControl"),
        kind: FeatureKind::Enum,
        read_only: false,
    },
    FeatureSpec {
        prop: "TemperatureControl",
        primary_feature: "TemperatureControl",
        fallback_feature: None,
        kind: FeatureKind::Enum,
        read_only: false,
    },
    FeatureSpec {
        prop: "TemperatureStatus",
        primary_feature: "TemperatureStatus",
        fallback_feature: None,
        kind: FeatureKind::Enum,
        read_only: true,
    },
    FeatureSpec {
        prop: "FanSpeed",
        primary_feature: "FanSpeed",
        fallback_feature: None,
        kind: FeatureKind::Enum,
        read_only: false,
    },
    FeatureSpec {
        prop: "FrameRate",
        primary_feature: "FrameRate",
        fallback_feature: None,
        kind: FeatureKind::Float,
        read_only: false,
    },
    FeatureSpec {
        prop: "FanSpeedRPM",
        primary_feature: "FanSpeedRPM",
        fallback_feature: None,
        kind: FeatureKind::Float,
        read_only: true,
    },
    FeatureSpec {
        prop: "SensorTemperature",
        primary_feature: "SensorTemperature",
        fallback_feature: None,
        kind: FeatureKind::Float,
        read_only: false,
    },
    FeatureSpec {
        prop: "ExternalTriggerDelay",
        primary_feature: "ExternalTriggerDelay",
        fallback_feature: None,
        kind: FeatureKind::Float,
        read_only: false,
    },
    FeatureSpec {
        prop: "AccumulateCount",
        primary_feature: "AccumulateCount",
        fallback_feature: None,
        kind: FeatureKind::Integer,
        read_only: false,
    },
    FeatureSpec {
        prop: "SensorCooling",
        primary_feature: "SensorCooling",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
    FeatureSpec {
        prop: "RollingShutterGlobalClear",
        primary_feature: "RollingShutterGlobalClear",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
    FeatureSpec {
        prop: "Overlap",
        primary_feature: "Overlap",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
    FeatureSpec {
        prop: "SpuriousNoiseFilter",
        primary_feature: "SpuriousNoiseFilter",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
    FeatureSpec {
        prop: "StaticBlemishCorrection",
        primary_feature: "StaticBlemishCorrection",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
    FeatureSpec {
        prop: "AuxOut1 (TTL I/O)",
        primary_feature: "AuxOut1",
        fallback_feature: Some("AuxiliaryOutSource"),
        kind: FeatureKind::Enum,
        read_only: false,
    },
    FeatureSpec {
        prop: "AuxOut2 (TTL I/O)",
        primary_feature: "AuxOut2",
        fallback_feature: Some("AuxOutSourceTwo"),
        kind: FeatureKind::Enum,
        read_only: false,
    },
    FeatureSpec {
        prop: "AuxOut3 (TTL I/O)",
        primary_feature: "AuxOut3",
        fallback_feature: None,
        kind: FeatureKind::Enum,
        read_only: false,
    },
    FeatureSpec {
        prop: "ShutterOutputMode",
        primary_feature: "ShutterOutputMode",
        fallback_feature: None,
        kind: FeatureKind::Enum,
        read_only: false,
    },
    FeatureSpec {
        prop: "ShutterTransferTime [s]",
        primary_feature: "ShutterTransferTime",
        fallback_feature: None,
        kind: FeatureKind::Float,
        read_only: false,
    },
    FeatureSpec {
        prop: "LightScanPlus-SensorReadoutMode",
        primary_feature: "SensorReadoutMode",
        fallback_feature: None,
        kind: FeatureKind::Enum,
        read_only: false,
    },
    FeatureSpec {
        prop: "LightScanPlus-AlternatingReadoutDirection",
        primary_feature: "AlternatingReadoutDirection",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
    FeatureSpec {
        prop: "LightScanPlus-ExposedPixelHeight",
        primary_feature: "ExposedPixelHeight",
        fallback_feature: None,
        kind: FeatureKind::Integer,
        read_only: false,
    },
    FeatureSpec {
        prop: "LightScanPlus-ScanSpeedControlEnable",
        primary_feature: "ScanSpeedControlEnable",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
    FeatureSpec {
        prop: "LightScanPlus-LineScanSpeed [lines/sec]",
        primary_feature: "LineScanSpeed",
        fallback_feature: None,
        kind: FeatureKind::Float,
        read_only: false,
    },
    FeatureSpec {
        prop: "RowReadTime",
        primary_feature: "RowReadTime",
        fallback_feature: None,
        kind: FeatureKind::Float,
        read_only: false,
    },
    FeatureSpec {
        prop: "PreTriggerEnable",
        primary_feature: "PreTriggerEnable",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-PIV",
        primary_feature: "PIVEnable",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-GateMode",
        primary_feature: "GateMode",
        fallback_feature: None,
        kind: FeatureKind::Enum,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-MCPIntelligate",
        primary_feature: "MCPIntelligate",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-MCPGain",
        primary_feature: "MCPGain",
        fallback_feature: None,
        kind: FeatureKind::Integer,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-MCPVoltage",
        primary_feature: "MCPVoltage",
        fallback_feature: None,
        kind: FeatureKind::Integer,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-InsertionDelay",
        primary_feature: "InsertionDelay",
        fallback_feature: None,
        kind: FeatureKind::Enum,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-DDGIOCEnable",
        primary_feature: "DDGIOCEnable",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-DDGIOCNumberOfPulses",
        primary_feature: "DDGIOCNumberOfPulses",
        fallback_feature: None,
        kind: FeatureKind::Integer,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-DDGIOCPeriod",
        primary_feature: "DDGIOCPeriod",
        fallback_feature: None,
        kind: FeatureKind::Integer,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-DDGOutputDelay",
        primary_feature: "DDGOutputDelay",
        fallback_feature: None,
        kind: FeatureKind::Integer,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-DDGOutputEnable",
        primary_feature: "DDGOutputEnable",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-DDGOutputStepEnable",
        primary_feature: "DDGOutputStepEnable",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-DDGStepEnabled",
        primary_feature: "DDGStepEnabled",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-DDGOpticalWidthEnable",
        primary_feature: "DDGOpticalWidthEnable",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-DDGOutputPolarity",
        primary_feature: "DDGOutputPolarity",
        fallback_feature: None,
        kind: FeatureKind::Enum,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-DDGOutputSelector",
        primary_feature: "DDGOutputSelector",
        fallback_feature: None,
        kind: FeatureKind::Enum,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-DDGOutputWidth",
        primary_feature: "DDGOutputWidth",
        fallback_feature: None,
        kind: FeatureKind::Integer,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-DDGStepCount",
        primary_feature: "DDGStepCount",
        fallback_feature: None,
        kind: FeatureKind::Integer,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-DDGStepDelayCoefficientA",
        primary_feature: "DDGStepDelayCoefficientA",
        fallback_feature: None,
        kind: FeatureKind::Float,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-DDGStepDelayCoefficientB",
        primary_feature: "DDGStepDelayCoefficientB",
        fallback_feature: None,
        kind: FeatureKind::Float,
        read_only: false,
    },
    FeatureSpec {
        prop: "iStar-DDGStepWidthMode",
        primary_feature: "DDGStepWidthMode",
        fallback_feature: None,
        kind: FeatureKind::Enum,
        read_only: false,
    },
    FeatureSpec {
        prop: "LowDarkCurrentEnable",
        primary_feature: "LowDarkCurrentEnable",
        fallback_feature: None,
        kind: FeatureKind::Bool,
        read_only: false,
    },
];

fn simple_feature_spec(prop: &str) -> Option<FeatureSpec> {
    SIMPLE_FEATURES
        .iter()
        .copied()
        .find(|spec| spec.prop == prop)
}

fn bool_property_value(value: bool) -> PropertyValue {
    PropertyValue::String(if value { "On" } else { "Off" }.into())
}

fn property_value_to_bool(value: &PropertyValue) -> MmResult<bool> {
    match value.to_string().as_str() {
        "On" | "on" | "True" | "true" | "Yes" | "yes" | "1" => Ok(true),
        "Off" | "off" | "False" | "false" | "No" | "no" | "0" => Ok(false),
        _ => Err(MmError::InvalidPropertyValue),
    }
}

fn sdk_trigger_mode(value: &str) -> &str {
    match value {
        "Internal (Recommended for fast acquisitions)" => "Internal",
        "Software (Recommended for Live Mode)" => "Software",
        _ => value,
    }
}

fn normalize_binning_value(value: &str) -> MmResult<(String, String)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(MmError::InvalidPropertyValue);
    }

    if let Some((x, y)) = trimmed.split_once('x') {
        let x = x
            .parse::<i32>()
            .map_err(|_| MmError::InvalidPropertyValue)?;
        let y = y
            .parse::<i32>()
            .map_err(|_| MmError::InvalidPropertyValue)?;
        if x <= 0 || x != y {
            return Err(MmError::InvalidPropertyValue);
        }
        return Ok((x.to_string(), format!("{}x{}", x, x)));
    }

    let bin = trimmed
        .parse::<i32>()
        .map_err(|_| MmError::InvalidPropertyValue)?;
    if bin <= 0 {
        return Err(MmError::InvalidPropertyValue);
    }
    Ok((bin.to_string(), format!("{}x{}", bin, bin)))
}

fn mm_roi_origin_to_sdk_zero_based(coord: u32, binning: i32) -> i32 {
    checked_mm_roi_origin_to_sdk_zero_based(coord, binning).unwrap_or(i32::MAX - 1)
}

fn checked_mm_roi_origin_to_sdk_zero_based(coord: u32, binning: i32) -> Option<i32> {
    let binning = binning.max(1) as u32;
    let sdk_coord = coord.checked_add(1)?.checked_mul(binning)?.checked_sub(1)?;
    i32::try_from(sdk_coord).ok().filter(|v| *v < i32::MAX)
}

fn sdk_zero_based_to_mm_roi_origin(coord: i32, binning: i32) -> u32 {
    let binning = binning.max(1) as u32;
    ((coord.max(0) as u32 + 1) / binning).saturating_sub(1)
}

// ── Camera struct ──────────────────────────────────────────────────────────────

pub struct Andor3Camera {
    props: PropertyMap,
    ctx: *mut ffi::Andor3Ctx,
    sdk_open: bool,
    img_buf: Vec<u8>,

    // Pre-init
    camera_index: i32,
    exposure_ms: f64,
    pixel_enc: String,    // "Mono16" default
    binning: String,      // "1x1" default
    trigger_mode: String, // "Internal" default
    pixel_readout_rate: String,
    electronic_shuttering_mode: String,
    pending_simple_features: BTreeSet<&'static str>,

    // Post-init (refreshed after snap / ROI changes)
    img_width: u32,
    img_height: u32,
    bytes_per_pixel: u32,
    bit_depth: u32,
    last_timestamp: Option<i64>,
    sequence_start_timestamp: Option<i64>,
    fpga_ts_clock_frequency: i64,

    capturing: bool,
    sequence_remaining: Option<i64>,
    sequence_image_sink: Option<Arc<dyn SequenceImageSink>>,
}

impl Andor3Camera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("CameraIndex", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("Exposure", PropertyValue::Float(10.0), false)
            .unwrap();
        props
            .define_property(
                "PixelEncoding",
                PropertyValue::String("Mono16".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values(
                "PixelEncoding",
                &["Mono16", "Mono12", "Mono12Packed", "Mono32"],
            )
            .unwrap();
        props
            .define_property("Binning", PropertyValue::String("1x1".into()), false)
            .unwrap();
        props
            .set_allowed_values("Binning", &["1", "1x1", "2", "2x2", "4", "4x4"])
            .unwrap();
        props
            .define_property(
                "TriggerMode",
                PropertyValue::String("Internal".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values(
                "TriggerMode",
                &[
                    "Internal",
                    "Internal (Recommended for fast acquisitions)",
                    "Software",
                    "Software (Recommended for Live Mode)",
                    "External",
                    "External Exposure",
                    "External Start",
                ],
            )
            .unwrap();
        props
            .define_property("PixelReadoutRate", PropertyValue::String("".into()), false)
            .unwrap();
        props
            .define_property(
                "ElectronicShutteringMode",
                PropertyValue::String("".into()),
                false,
            )
            .unwrap();
        props
            .define_property("FrameRateLimits", PropertyValue::String("".into()), true)
            .unwrap();
        for spec in SIMPLE_FEATURES {
            if props.has_property(spec.prop) {
                continue;
            }
            let default = match spec.kind {
                FeatureKind::Enum => PropertyValue::String("".into()),
                FeatureKind::Float => PropertyValue::Float(0.0),
                FeatureKind::Integer => PropertyValue::Integer(0),
                FeatureKind::Bool => {
                    bool_property_value(matches!(spec.prop, "SensorCooling" | "Overlap"))
                }
            };
            props
                .define_property(spec.prop, default, spec.read_only)
                .unwrap();
            if spec.kind == FeatureKind::Bool {
                props.set_allowed_values(spec.prop, &["Off", "On"]).unwrap();
            }
        }
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
            .define_property("SensorWidth", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("SensorHeight", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("Temperature", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("SerialNumber", PropertyValue::String("".into()), true)
            .unwrap();
        props
            .define_property("CameraID", PropertyValue::String("".into()), true)
            .unwrap();
        props
            .define_property("CameraModel", PropertyValue::String("".into()), true)
            .unwrap();
        props
            .define_property("FirmwareVersion", PropertyValue::String("".into()), true)
            .unwrap();
        props
            .define_property("CameraFirmware", PropertyValue::String("".into()), true)
            .unwrap();
        props
            .define_property("CurrentSoftware", PropertyValue::String("".into()), true)
            .unwrap();
        props
            .define_property(
                "Ext (Exp) Trigger Timeout[ms]",
                PropertyValue::Integer(5_000),
                false,
            )
            .unwrap();
        props
            .define_property(
                "Description",
                PropertyValue::String("SDK3 Device Adapter for sCMOS cameras".into()),
                true,
            )
            .unwrap();
        props
            .define_property("LastFPGAFrameTimestamp", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("ElapsedTime-ms", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("LastWaitError", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("TimestampClockFrequency", PropertyValue::Integer(0), true)
            .unwrap();
        define_srrf_stub_properties(&mut props);

        Self {
            props,
            ctx: std::ptr::null_mut(),
            sdk_open: false,
            img_buf: Vec::new(),
            camera_index: 0,
            exposure_ms: 10.0,
            pixel_enc: "Mono16".into(),
            binning: "1x1".into(),
            trigger_mode: "Internal".into(),
            pixel_readout_rate: String::new(),
            electronic_shuttering_mode: String::new(),
            pending_simple_features: BTreeSet::new(),
            img_width: 0,
            img_height: 0,
            bytes_per_pixel: 2,
            bit_depth: 16,
            last_timestamp: None,
            sequence_start_timestamp: None,
            fpga_ts_clock_frequency: 0,
            capturing: false,
            sequence_remaining: None,
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

    fn sync_dims(&mut self) {
        if self.ctx.is_null() {
            return;
        }
        self.img_width = unsafe { ffi::andor3_get_image_width(self.ctx) } as u32;
        self.img_height = unsafe { ffi::andor3_get_image_height(self.ctx) } as u32;
        self.bytes_per_pixel = unsafe { ffi::andor3_get_bytes_per_pixel(self.ctx) } as u32;
        self.bit_depth = unsafe { ffi::andor3_get_bit_depth(self.ctx) } as u32;
        if self.bytes_per_pixel == 0 {
            self.bytes_per_pixel = 2;
        }
        if self.bit_depth == 0 {
            self.bit_depth = 16;
        }
        self.props
            .entry_mut("Width")
            .map(|e| e.value = PropertyValue::Integer(self.img_width as i64));
        self.props
            .entry_mut("Height")
            .map(|e| e.value = PropertyValue::Integer(self.img_height as i64));
        self.props
            .entry_mut("BitDepth")
            .map(|e| e.value = PropertyValue::Integer(self.bit_depth as i64));
    }

    fn close_sdk(&mut self) {
        if self.sdk_open {
            unsafe { ffi::andor3_sdk_close() };
            self.sdk_open = false;
        }
    }

    fn snap_timeout_ms(&self) -> i32 {
        let extra = if self.trigger_mode.starts_with("External") {
            self.props
                .get("Ext (Exp) Trigger Timeout[ms]")
                .ok()
                .and_then(|v| v.as_i64())
                .unwrap_or(DEFAULT_SNAP_TIMEOUT_MS)
        } else {
            DEFAULT_SNAP_TIMEOUT_MS
        };
        let timeout = self.exposure_ms.ceil().max(0.0) + extra.max(0) as f64;
        timeout.clamp(1.0, i32::MAX as f64) as i32
    }

    fn copy_frame_from_shim(&mut self) -> MmResult<()> {
        let ptr = unsafe { ffi::andor3_get_frame_ptr(self.ctx) };
        if ptr.is_null() {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        let bytes = unsafe { ffi::andor3_get_frame_bytes(self.ctx) } as usize;
        if bytes == 0 {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        let src = unsafe { std::slice::from_raw_parts(ptr, bytes) };
        self.img_buf.clear();
        self.img_buf.extend_from_slice(src);
        self.refresh_frame_metadata();
        Ok(())
    }

    fn refresh_frame_metadata(&mut self) {
        if self.ctx.is_null() {
            return;
        }
        if unsafe { ffi::andor3_has_last_timestamp(self.ctx) } != 0 {
            let timestamp = unsafe { ffi::andor3_get_last_timestamp(self.ctx) };
            self.last_timestamp = Some(timestamp);
            if self.sequence_start_timestamp.is_none() {
                self.sequence_start_timestamp = Some(timestamp);
            }
            self.props
                .entry_mut("LastFPGAFrameTimestamp")
                .map(|e| e.value = PropertyValue::Integer(timestamp));
            if let Some(start) = self.sequence_start_timestamp {
                let frequency = self.fpga_ts_clock_frequency.max(1) as f64;
                self.props.entry_mut("ElapsedTime-ms").map(|e| {
                    e.value = PropertyValue::Float((timestamp - start) as f64 / frequency * 1_000.0)
                });
            }
        }
        let wait_error = unsafe { ffi::andor3_get_last_wait_error(self.ctx) } as i64;
        self.props
            .entry_mut("LastWaitError")
            .map(|e| e.value = PropertyValue::Integer(wait_error));
    }

    fn last_wait_error(&self) -> i64 {
        if self.ctx.is_null() {
            0
        } else {
            (unsafe { ffi::andor3_get_last_wait_error(self.ctx) }) as i64
        }
    }

    fn frame_wait_error(&mut self) -> MmError {
        let wait_error = self.last_wait_error();
        self.props
            .entry_mut("LastWaitError")
            .map(|e| e.value = PropertyValue::Integer(wait_error));
        if wait_error == AT_ERR_HARDWARE_OVERFLOW {
            MmError::BufferOverflow
        } else {
            MmError::SnapImageFailed
        }
    }

    fn require_not_capturing(&self) -> MmResult<()> {
        if self.capturing {
            Err(MmError::CameraBusyAcquiring)
        } else {
            Ok(())
        }
    }

    fn write_enum_feature(&self, feature: &str, value: &str) -> MmResult<()> {
        if self.ctx.is_null() {
            return Ok(());
        }
        let f = cstr(feature);
        let v = cstr(value);
        if unsafe { ffi::andor3_set_enum(self.ctx, f.as_ptr(), v.as_ptr()) } != 0 {
            return Err(MmError::InvalidPropertyValue);
        }
        Ok(())
    }

    fn sdk_feature_for_spec(&self, spec: FeatureSpec) -> &'static str {
        if !self.ctx.is_null() {
            let f = cstr(spec.primary_feature);
            if unsafe { ffi::andor3_is_implemented(self.ctx, f.as_ptr()) } != 0 {
                return spec.primary_feature;
            }
        }
        spec.fallback_feature.unwrap_or(spec.primary_feature)
    }

    fn sdk_feature_for_property<'a>(&self, prop: &'a str) -> &'a str {
        simple_feature_spec(prop)
            .map(|spec| self.sdk_feature_for_spec(spec))
            .unwrap_or_else(|| sdk_enum_feature_for_property(prop))
    }

    fn read_float_feature(&self, feature: &str) -> Option<f64> {
        if self.ctx.is_null() {
            return None;
        }
        let f = cstr(feature);
        let mut value = 0.0;
        if unsafe { ffi::andor3_get_float(self.ctx, f.as_ptr(), &mut value) } == 0 {
            Some(value)
        } else {
            None
        }
    }

    fn read_int_feature(&self, feature: &str) -> Option<i64> {
        if self.ctx.is_null() {
            return None;
        }
        let f = cstr(feature);
        let mut value = 0i64;
        if unsafe { ffi::andor3_get_int(self.ctx, f.as_ptr(), &mut value) } == 0 {
            Some(value)
        } else {
            None
        }
    }

    fn read_bool_feature(&self, feature: &str) -> Option<bool> {
        if self.ctx.is_null() {
            return None;
        }
        let f = cstr(feature);
        let mut value = 0;
        if unsafe { ffi::andor3_get_bool(self.ctx, f.as_ptr(), &mut value) } == 0 {
            Some(value != 0)
        } else {
            None
        }
    }

    fn write_float_feature(&self, feature: &str, value: f64) -> MmResult<()> {
        if self.ctx.is_null() {
            return Ok(());
        }
        let f = cstr(feature);
        if unsafe { ffi::andor3_set_float(self.ctx, f.as_ptr(), value) } != 0 {
            return Err(MmError::InvalidPropertyValue);
        }
        Ok(())
    }

    fn write_exposure_ms(&self, exposure_ms: f64) -> MmResult<f64> {
        if !exposure_ms.is_finite() || exposure_ms < 0.0 {
            return Err(MmError::InvalidPropertyValue);
        }
        let mut exposure_s = exposure_ms / 1_000.0;
        if !self.ctx.is_null() {
            let f = cstr("ExposureTime");
            let mut min = 0.0;
            let mut max = 0.0;
            if unsafe { ffi::andor3_get_float_limits(self.ctx, f.as_ptr(), &mut min, &mut max) }
                == 0
            {
                exposure_s = exposure_s.clamp(min, max);
            }
        }
        if !self.ctx.is_null() && unsafe { ffi::andor3_set_exposure_s(self.ctx, exposure_s) } != 0 {
            return Err(MmError::InvalidPropertyValue);
        }
        Ok(exposure_s * 1_000.0)
    }

    fn write_int_feature(&self, feature: &str, value: i64) -> MmResult<()> {
        if self.ctx.is_null() {
            return Ok(());
        }
        let f = cstr(feature);
        if unsafe { ffi::andor3_set_int(self.ctx, f.as_ptr(), value) } != 0 {
            return Err(MmError::InvalidPropertyValue);
        }
        Ok(())
    }

    fn write_bool_feature(&self, feature: &str, value: bool) -> MmResult<()> {
        if self.ctx.is_null() {
            return Ok(());
        }
        let f = cstr(feature);
        let sdk_value = if value { 1 } else { 0 };
        if unsafe { ffi::andor3_set_bool(self.ctx, f.as_ptr(), sdk_value) } != 0 {
            return Err(MmError::InvalidPropertyValue);
        }
        Ok(())
    }

    fn set_default_bool_feature(&self, feature: &str, value: bool) {
        if !self.ctx.is_null() {
            let f = cstr(feature);
            unsafe {
                ffi::andor3_set_bool(self.ctx, f.as_ptr(), if value { 1 } else { 0 });
            }
        }
    }

    fn refresh_simple_feature_property(&mut self, spec: FeatureSpec) {
        if self.ctx.is_null() {
            return;
        }
        let feature = self.sdk_feature_for_spec(spec);
        let f = cstr(feature);
        if let Some(entry) = self.props.entry_mut(spec.prop) {
            if unsafe { ffi::andor3_is_read_only(self.ctx, f.as_ptr()) } != 0 {
                entry.read_only = true;
            }
        }
        match spec.kind {
            FeatureKind::Enum => {
                let vals = enum_values(self.ctx, feature);
                if !vals.is_empty() {
                    let refs: Vec<&str> = vals.iter().map(|s| s.as_str()).collect();
                    self.props.set_allowed_values(spec.prop, &refs).ok();
                }
                if let Some(value) = read_enum(self.ctx, feature) {
                    self.props
                        .entry_mut(spec.prop)
                        .map(|e| e.value = PropertyValue::String(value.clone()));
                    match spec.prop {
                        "PixelReadoutRate" => self.pixel_readout_rate = value,
                        "ElectronicShutteringMode" => self.electronic_shuttering_mode = value,
                        _ => {}
                    }
                }
            }
            FeatureKind::Float => {
                if let Some(value) = self.read_float_feature(feature) {
                    self.props
                        .entry_mut(spec.prop)
                        .map(|e| e.value = PropertyValue::Float(value));
                }
                let mut min = 0.0;
                let mut max = 0.0;
                if unsafe { ffi::andor3_get_float_limits(self.ctx, f.as_ptr(), &mut min, &mut max) }
                    == 0
                {
                    self.props.set_property_limits(spec.prop, min, max).ok();
                }
            }
            FeatureKind::Integer => {
                if let Some(value) = self.read_int_feature(feature) {
                    self.props
                        .entry_mut(spec.prop)
                        .map(|e| e.value = PropertyValue::Integer(value));
                }
                let mut min = 0i64;
                let mut max = 0i64;
                if unsafe { ffi::andor3_get_int_limits(self.ctx, f.as_ptr(), &mut min, &mut max) }
                    == 0
                {
                    self.props
                        .set_property_limits(spec.prop, min as f64, max as f64)
                        .ok();
                }
            }
            FeatureKind::Bool => {
                if let Some(value) = self.read_bool_feature(feature) {
                    self.props
                        .entry_mut(spec.prop)
                        .map(|e| e.value = bool_property_value(value));
                }
            }
        }
    }

    fn frame_rate_limits_text(&self) -> String {
        if self.ctx.is_null() {
            return self
                .props
                .get("FrameRateLimits")
                .ok()
                .map(|v| v.to_string())
                .unwrap_or_default();
        }
        let feature = self.sdk_feature_for_property("FrameRate");
        let f = cstr(feature);
        let mut min = 0.0;
        let mut max = 0.0;
        if unsafe { ffi::andor3_get_float_limits(self.ctx, f.as_ptr(), &mut min, &mut max) } != 0 {
            return String::new();
        }
        if let Some(max_sustain) = self.read_float_feature("MaxInterfaceTransferRate") {
            format!(
                "Min: {:.5}  Max: {}  Max Sustain: {}",
                min, max, max_sustain
            )
        } else {
            format!("Min: {:.5}  Max: {}", min, max)
        }
    }

    fn read_simple_feature_property(&self, spec: FeatureSpec) -> Option<PropertyValue> {
        let feature = self.sdk_feature_for_spec(spec);
        match spec.kind {
            FeatureKind::Enum => read_enum(self.ctx, feature).map(PropertyValue::String),
            FeatureKind::Float => self.read_float_feature(feature).map(PropertyValue::Float),
            FeatureKind::Integer => self.read_int_feature(feature).map(PropertyValue::Integer),
            FeatureKind::Bool => self.read_bool_feature(feature).map(bool_property_value),
        }
    }

    fn write_simple_feature_property(
        &self,
        spec: FeatureSpec,
        val: &PropertyValue,
    ) -> MmResult<PropertyValue> {
        let feature = self.sdk_feature_for_spec(spec);
        match spec.kind {
            FeatureKind::Enum => {
                let value = val.as_str().to_string();
                if !value.is_empty() {
                    self.write_enum_feature(feature, &value)?;
                }
                Ok(PropertyValue::String(value))
            }
            FeatureKind::Float => {
                let mut value = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !value.is_finite() {
                    return Err(MmError::InvalidPropertyValue);
                }
                if !self.ctx.is_null() {
                    let f = cstr(feature);
                    let mut min = 0.0;
                    let mut max = 0.0;
                    if unsafe {
                        ffi::andor3_get_float_limits(self.ctx, f.as_ptr(), &mut min, &mut max)
                    } == 0
                    {
                        value = value.clamp(min, max);
                    }
                }
                self.write_float_feature(feature, value)?;
                Ok(PropertyValue::Float(value))
            }
            FeatureKind::Integer => {
                let mut value = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !self.ctx.is_null() {
                    let f = cstr(feature);
                    let mut min = 0i64;
                    let mut max = 0i64;
                    if unsafe {
                        ffi::andor3_get_int_limits(self.ctx, f.as_ptr(), &mut min, &mut max)
                    } == 0
                    {
                        value = value.clamp(min, max);
                    }
                }
                self.write_int_feature(feature, value)?;
                Ok(PropertyValue::Integer(value))
            }
            FeatureKind::Bool => {
                let value = property_value_to_bool(val)?;
                self.write_bool_feature(feature, value)?;
                Ok(bool_property_value(value))
            }
        }
    }
}

impl Default for Andor3Camera {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Andor3Camera {
    fn drop(&mut self) {
        let _ = self.stop_sequence_acquisition();
        if !self.ctx.is_null() {
            unsafe { ffi::andor3_close(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
        self.close_sdk();
    }
}

// ── Device trait ───────────────────────────────────────────────────────────────

impl Device for Andor3Camera {
    fn name(&self) -> &str {
        "Andor sCMOS Camera"
    }
    fn description(&self) -> &str {
        "SDK3 Device Adapter for sCMOS cameras"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.ctx.is_null() {
            return Ok(());
        }

        if unsafe { ffi::andor3_sdk_open() } != 0 {
            return Err(MmError::LocallyDefined(
                "Andor SDK3: library initialisation failed".into(),
            ));
        }
        self.sdk_open = true;

        let count = unsafe { ffi::andor3_get_device_count() };
        if count <= 0 {
            self.close_sdk();
            return Err(MmError::LocallyDefined(
                "Andor SDK3: no cameras found".into(),
            ));
        }
        if self.camera_index >= count {
            self.close_sdk();
            return Err(MmError::LocallyDefined(format!(
                "Andor SDK3: camera index {} out of range (found {})",
                self.camera_index, count
            )));
        }

        let ctx = unsafe { ffi::andor3_open(self.camera_index) };
        if ctx.is_null() {
            self.close_sdk();
            return Err(MmError::LocallyDefined(format!(
                "Andor SDK3: failed to open camera {}",
                self.camera_index
            )));
        }
        self.ctx = ctx;

        // Match upstream defaults where available; unsupported features are ignored.
        self.set_default_bool_feature("SensorCooling", true);
        self.set_default_bool_feature("Overlap", true);
        self.set_default_bool_feature("MetadataEnable", true);
        self.set_default_bool_feature("MetadataTimestamp", true);
        self.fpga_ts_clock_frequency = self
            .read_int_feature("TimestampClockFrequency")
            .unwrap_or(0);
        self.props
            .entry_mut("TimestampClockFrequency")
            .map(|e| e.value = PropertyValue::Integer(self.fpga_ts_clock_frequency));

        // Read static properties.
        let sw = unsafe { ffi::andor3_get_sensor_width(ctx) } as i64;
        let sh = unsafe { ffi::andor3_get_sensor_height(ctx) } as i64;
        self.props
            .entry_mut("SensorWidth")
            .map(|e| e.value = PropertyValue::Integer(sw));
        self.props
            .entry_mut("SensorHeight")
            .map(|e| e.value = PropertyValue::Integer(sh));

        for (feat, prop) in &[
            ("SerialNumber", "SerialNumber"),
            ("CameraModel", "CameraModel"),
            ("FirmwareVersion", "FirmwareVersion"),
        ] {
            if let Some(s) = read_str(|b, l| {
                let f = cstr(feat);
                unsafe { ffi::andor3_get_string(ctx, f.as_ptr(), b, l) }
            }) {
                self.props
                    .entry_mut(prop)
                    .map(|e| e.value = PropertyValue::String(s));
            }
        }
        if let Ok(PropertyValue::String(serial)) = self.props.get("SerialNumber").cloned() {
            self.props
                .entry_mut("CameraID")
                .map(|e| e.value = PropertyValue::String(serial));
        }
        if let Ok(PropertyValue::String(firmware)) = self.props.get("FirmwareVersion").cloned() {
            self.props
                .entry_mut("CameraFirmware")
                .map(|e| e.value = PropertyValue::String(firmware));
        }
        if let Some(software) = read_str(|b, l| {
            let f = cstr("SoftwareVersion");
            unsafe { ffi::andor3_get_string(ctx, f.as_ptr(), b, l) }
        }) {
            self.props
                .entry_mut("CurrentSoftware")
                .map(|e| e.value = PropertyValue::String(software));
        }

        // Populate allowed values for enum properties.
        for prop in &[
            "PixelEncoding",
            "Binning",
            "TriggerMode",
            "PixelReadoutRate",
            "ElectronicShutteringMode",
            "Sensitivity/DynamicRange",
            "TemperatureControl",
            "TemperatureStatus",
            "FanSpeed",
        ] {
            let mut vals = enum_values(ctx, self.sdk_feature_for_property(prop));
            if *prop == "Binning" {
                let mut aliases = Vec::new();
                for value in &vals {
                    if let Ok((mm_value, _)) = normalize_binning_value(value) {
                        aliases.push(mm_value);
                    }
                }
                vals.extend(aliases);
                vals.extend(["1".to_string(), "1x1".to_string()]);
                vals.sort();
                vals.dedup();
            }
            if *prop == "TriggerMode" {
                vals.extend([
                    "Internal (Recommended for fast acquisitions)".to_string(),
                    "Software (Recommended for Live Mode)".to_string(),
                ]);
            }
            if !vals.is_empty() {
                let refs: Vec<&str> = vals.iter().map(|s| s.as_str()).collect();
                self.props.set_allowed_values(prop, &refs).ok();
                if self
                    .props
                    .get(prop)
                    .ok()
                    .map(|v| v.as_str())
                    .unwrap_or("")
                    .is_empty()
                {
                    self.props
                        .set(prop, PropertyValue::String(vals[0].clone()))
                        .ok();
                    match *prop {
                        "PixelReadoutRate" => self.pixel_readout_rate = vals[0].clone(),
                        "ElectronicShutteringMode" => {
                            self.electronic_shuttering_mode = vals[0].clone()
                        }
                        _ => {}
                    }
                }
            }
        }

        // Apply pre-init settings.
        self.exposure_ms = self.write_exposure_ms(self.exposure_ms)?;
        self.write_enum_feature("PixelEncoding", &self.pixel_enc)?;
        self.write_enum_feature("AOIBinning", &self.binning)?;
        self.write_enum_feature("TriggerMode", &self.trigger_mode)?;

        if !self.pixel_readout_rate.is_empty() {
            let f = cstr(self.sdk_feature_for_property("PixelReadoutRate"));
            let v = cstr(&self.pixel_readout_rate);
            if unsafe { ffi::andor3_set_enum(ctx, f.as_ptr(), v.as_ptr()) } != 0 {
                return Err(MmError::InvalidPropertyValue);
            }
        }

        if !self.electronic_shuttering_mode.is_empty() {
            let f = cstr(self.sdk_feature_for_property("ElectronicShutteringMode"));
            let v = cstr(&self.electronic_shuttering_mode);
            if unsafe { ffi::andor3_set_enum(ctx, f.as_ptr(), v.as_ptr()) } != 0 {
                return Err(MmError::InvalidPropertyValue);
            }
        }

        for prop in self.pending_simple_features.iter().copied() {
            if let Some(spec) = simple_feature_spec(prop) {
                if spec.read_only {
                    continue;
                }
                let value = self.props.get(prop)?.clone();
                self.write_simple_feature_property(spec, &value)?;
            }
        }
        self.pending_simple_features.clear();

        for spec in SIMPLE_FEATURES {
            self.refresh_simple_feature_property(*spec);
        }
        let limits = self.frame_rate_limits_text();
        self.props
            .entry_mut("FrameRateLimits")
            .map(|e| e.value = PropertyValue::String(limits));

        self.sync_dims();
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        let _ = self.stop_sequence_acquisition();
        if !self.ctx.is_null() {
            unsafe { ffi::andor3_close(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
        self.close_sdk();
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "CameraIndex" => Ok(PropertyValue::Integer(self.camera_index as i64)),
            "Exposure" => Ok(PropertyValue::Float(self.exposure_ms)),
            "PixelEncoding" => Ok(PropertyValue::String(self.pixel_enc.clone())),
            "Binning" => Ok(PropertyValue::String(self.binning.clone())),
            "TriggerMode" => Ok(PropertyValue::String(self.trigger_mode.clone())),
            "PixelReadoutRate" => {
                if !self.ctx.is_null() {
                    if let Some(v) = read_enum(self.ctx, self.sdk_feature_for_property(name)) {
                        return Ok(PropertyValue::String(v));
                    }
                }
                Ok(PropertyValue::String(self.pixel_readout_rate.clone()))
            }
            "ElectronicShutteringMode" => {
                if !self.ctx.is_null() {
                    if let Some(v) = read_enum(self.ctx, self.sdk_feature_for_property(name)) {
                        return Ok(PropertyValue::String(v));
                    }
                }
                Ok(PropertyValue::String(
                    self.electronic_shuttering_mode.clone(),
                ))
            }
            "FrameRateLimits" => Ok(PropertyValue::String(self.frame_rate_limits_text())),
            "Temperature" => {
                let t = if self.ctx.is_null() {
                    0.0
                } else {
                    unsafe { ffi::andor3_get_temperature(self.ctx) }
                };
                Ok(PropertyValue::Float(t))
            }
            _ if simple_feature_spec(name).is_some() && !self.ctx.is_null() => {
                let spec = simple_feature_spec(name).unwrap();
                self.read_simple_feature_property(spec)
                    .or_else(|| self.props.get(name).cloned().ok())
                    .ok_or_else(|| MmError::UnknownLabel(name.to_string()))
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "SRRF | Enable" => {
                self.require_not_capturing()?;
                let value = val.as_str();
                if value == "Enabled" {
                    return Err(MmError::NotYetImplemented);
                }
                self.props
                    .set(name, PropertyValue::String("Disabled".into()))
            }
            "CameraIndex" => {
                if !self.ctx.is_null() {
                    return Err(MmError::LocallyDefined(
                        "CameraIndex cannot be changed after initialize()".into(),
                    ));
                }
                let camera_index = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if camera_index < 0 || camera_index > i32::MAX as i64 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.camera_index = camera_index as i32;
                self.props
                    .set(name, PropertyValue::Integer(self.camera_index as i64))
            }
            "Exposure" => {
                self.require_not_capturing()?;
                let exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.exposure_ms = self.write_exposure_ms(exposure_ms)?;
                self.props
                    .set(name, PropertyValue::Float(self.exposure_ms))?;
                Ok(())
            }
            "PixelEncoding" => {
                self.require_not_capturing()?;
                let pixel_enc = val.as_str().to_string();
                self.write_enum_feature("PixelEncoding", &pixel_enc)?;
                self.props.set(name, val.clone())?;
                self.pixel_enc = pixel_enc;
                if !self.ctx.is_null() {
                    self.sync_dims();
                }
                Ok(())
            }
            "Binning" => {
                self.require_not_capturing()?;
                let (_mm_value, sdk_value) = normalize_binning_value(&val.to_string())?;
                self.write_enum_feature("AOIBinning", &sdk_value)?;
                self.props
                    .set(name, PropertyValue::String(sdk_value.clone()))?;
                self.binning = sdk_value;
                if !self.ctx.is_null() {
                    self.sync_dims();
                }
                Ok(())
            }
            "TriggerMode" => {
                self.require_not_capturing()?;
                let trigger_mode = sdk_trigger_mode(val.as_str()).to_string();
                self.write_enum_feature("TriggerMode", &trigger_mode)?;
                self.props
                    .set(name, PropertyValue::String(trigger_mode.clone()))?;
                self.trigger_mode = trigger_mode;
                Ok(())
            }
            "PixelReadoutRate" => {
                self.require_not_capturing()?;
                let pixel_readout_rate = val.as_str().to_string();
                if !pixel_readout_rate.is_empty() {
                    self.write_enum_feature(
                        self.sdk_feature_for_property(name),
                        &pixel_readout_rate,
                    )?;
                }
                self.props.set(name, val.clone())?;
                self.pixel_readout_rate = pixel_readout_rate;
                Ok(())
            }
            "ElectronicShutteringMode" => {
                self.require_not_capturing()?;
                let electronic_shuttering_mode = val.as_str().to_string();
                if !electronic_shuttering_mode.is_empty() {
                    self.write_enum_feature(
                        self.sdk_feature_for_property(name),
                        &electronic_shuttering_mode,
                    )?;
                }
                self.props.set(name, val.clone())?;
                self.electronic_shuttering_mode = electronic_shuttering_mode;
                Ok(())
            }
            "Ext (Exp) Trigger Timeout[ms]" => {
                let timeout = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0..=i32::MAX as i64).contains(&timeout) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.props.set(name, PropertyValue::Integer(timeout))
            }
            _ if simple_feature_spec(name).is_some() => {
                self.require_not_capturing()?;
                if self.is_property_read_only(name) {
                    return self.props.set(name, val);
                }
                let spec = simple_feature_spec(name).unwrap();
                let value = self.write_simple_feature_property(spec, &val)?;
                self.props.set(name, value)?;
                if self.ctx.is_null() {
                    self.pending_simple_features.insert(spec.prop);
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

fn define_srrf_stub_properties(props: &mut PropertyMap) {
    props
        .define_property(
            "SRRF Status",
            PropertyValue::String(SRRF_UNAVAILABLE_STATUS.into()),
            true,
        )
        .unwrap();
    props
        .define_property(
            "SRRF | Version Information",
            PropertyValue::String(SRRF_UNAVAILABLE_STATUS.into()),
            true,
        )
        .unwrap();
    props
        .define_property(
            "SRRF | Number of Frames per Time point",
            PropertyValue::Integer(100),
            false,
        )
        .unwrap();
    props
        .define_property(
            "SRRF | Radiality Magnification",
            PropertyValue::Integer(4),
            false,
        )
        .unwrap();
    props
        .set_property_limits("SRRF | Radiality Magnification", 1.0, 6.0)
        .unwrap();
    props
        .define_property("SRRF | Ring Radius", PropertyValue::Float(2.0), false)
        .unwrap();
    props
        .set_property_limits("SRRF | Ring Radius", 1.0, 3.0)
        .unwrap();
    props
        .define_property(
            "SRRF | Radiality Temporal Analysis",
            PropertyValue::String("Mean".into()),
            false,
        )
        .unwrap();
    props
        .set_allowed_values("SRRF | Radiality Temporal Analysis", &["Mean", "MIP"])
        .unwrap();
    props
        .define_property(
            "SRRF | Interpolation Type",
            PropertyValue::String("Catmull-Rom".into()),
            false,
        )
        .unwrap();
    props
        .set_allowed_values(
            "SRRF | Interpolation Type",
            &["Catmull-Rom", "Fast B-spline"],
        )
        .unwrap();
    props
        .define_property(
            "SRRF | Enable",
            PropertyValue::String("Disabled".into()),
            false,
        )
        .unwrap();
    props
        .set_allowed_values("SRRF | Enable", &["Disabled", "Enabled"])
        .unwrap();
    props
        .define_property(
            "SRRF | Save Original Data | Option",
            PropertyValue::String("None".into()),
            false,
        )
        .unwrap();
    props
        .set_allowed_values(
            "SRRF | Save Original Data | Option",
            &["None", "All", "Averaged"],
        )
        .unwrap();
    props
        .define_property(
            "SRRF | Save Original Data | Path",
            PropertyValue::String(String::new()),
            false,
        )
        .unwrap();
}

// ── Camera trait ───────────────────────────────────────────────────────────────

impl Camera for Andor3Camera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_open()?;
        if self.capturing {
            let timeout = self.snap_timeout_ms();
            let rc = unsafe { ffi::andor3_get_next_frame(self.ctx, timeout) };
            if rc != 0 {
                let err = self.frame_wait_error();
                let _ = self.stop_sequence_acquisition();
                return Err(err);
            }
            self.copy_frame_from_shim()?;
            if let Some(sink) = &self.sequence_image_sink {
                if sink.insert_sequence_image(ImageFrame::new(
                    self.img_buf.clone(),
                    self.img_width,
                    self.img_height,
                    self.bytes_per_pixel.max(1),
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
        let timeout = self.snap_timeout_ms();
        self.sequence_start_timestamp = None;
        let rc = unsafe { ffi::andor3_snap(self.ctx, timeout) };
        if rc != 0 {
            return Err(self.frame_wait_error());
        }
        self.sync_dims();
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
        self.bytes_per_pixel.max(1)
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
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        let exp_ms = self.write_exposure_ms(exp_ms)?;
        self.exposure_ms = exp_ms;
        self.props.set("Exposure", PropertyValue::Float(exp_ms))?;
        Ok(())
    }

    fn get_binning(&self) -> i32 {
        // Parse "NxN" → N
        self.binning
            .split('x')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1)
    }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        self.set_property("Binning", PropertyValue::String(bin.to_string()))
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        if self.ctx.is_null() {
            return Ok(ImageRoi::new(0, 0, self.img_width, self.img_height));
        }
        let (mut l, mut t, mut w, mut h) = (0i32, 0i32, 0i32, 0i32);
        if unsafe { ffi::andor3_get_aoi(self.ctx, &mut l, &mut t, &mut w, &mut h) } != 0 {
            return Err(MmError::Err);
        }
        let binning = self.get_binning();
        Ok(ImageRoi::new(
            sdk_zero_based_to_mm_roi_origin(l, binning),
            sdk_zero_based_to_mm_roi_origin(t, binning),
            w as u32,
            h as u32,
        ))
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        self.require_not_capturing()?;
        self.check_open()?;
        if roi.width == 0 && roi.height == 0 {
            return self.clear_roi();
        }
        let binning = self.get_binning();
        let left = checked_mm_roi_origin_to_sdk_zero_based(roi.x, binning)
            .ok_or(MmError::InvalidPropertyValue)?;
        let top = checked_mm_roi_origin_to_sdk_zero_based(roi.y, binning)
            .ok_or(MmError::InvalidPropertyValue)?;
        let width = i32::try_from(roi.width).map_err(|_| MmError::InvalidPropertyValue)?;
        let height = i32::try_from(roi.height).map_err(|_| MmError::InvalidPropertyValue)?;
        let rc = unsafe { ffi::andor3_set_aoi(self.ctx, left, top, width, height) };
        if rc != 0 {
            return Err(MmError::Err);
        }
        self.sync_dims();
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        self.require_not_capturing()?;
        self.check_open()?;
        if unsafe { ffi::andor3_clear_aoi(self.ctx) } != 0 {
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
        if count > 0 {
            self.write_enum_feature("CycleMode", "Fixed")?;
            self.write_int_feature("FrameCount", count)?;
        } else {
            self.write_enum_feature("CycleMode", "Continuous")?;
        }
        let rc = unsafe { ffi::andor3_start_cont(self.ctx) };
        if rc != 0 {
            return Err(MmError::LocallyDefined(
                "Andor SDK3: failed to start continuous acquisition".into(),
            ));
        }
        self.sequence_start_timestamp = None;
        self.capturing = true;
        self.sequence_remaining = if count > 0 { Some(count) } else { None };
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        if !self.capturing {
            return Ok(());
        }
        if !self.ctx.is_null() {
            unsafe { ffi::andor3_stop_cont(self.ctx) };
        }
        self.capturing = false;
        self.sequence_remaining = None;
        self.sequence_start_timestamp = None;
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

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{c_int, c_uint};
    use std::sync::{Mutex, MutexGuard};

    static STUB_SDK_LOCK: Mutex<()> = Mutex::new(());
    const AT_HANDLE_SYSTEM: c_int = 1;
    const STUB_HANDLE: c_int = 100;
    const INVALID_HANDLE: c_int = 999;
    const STUB_IMAGE_BYTES: usize = 64 * 48 * 2;

    extern "C" {
        fn AT_IsWritable(hndl: c_int, feature: *const c_int, writable: *mut c_int) -> c_int;
        fn AT_GetEnumCount(hndl: c_int, feature: *const c_int, count: *mut c_int) -> c_int;
        fn AT_GetEnumStringByIndex(
            hndl: c_int,
            feature: *const c_int,
            index: c_int,
            string: *mut c_int,
            string_length: c_int,
        ) -> c_int;
        fn AT_GetStringMaxLength(
            hndl: c_int,
            feature: *const c_int,
            max_string_length: *mut c_int,
        ) -> c_int;
        fn AT_QueueBuffer(hndl: c_int, ptr: *mut u8, ptr_size: c_int) -> c_int;
        fn AT_WaitBuffer(
            hndl: c_int,
            ptr: *mut *mut u8,
            ptr_size: *mut c_int,
            timeout: c_uint,
        ) -> c_int;
        fn AT_Flush(hndl: c_int) -> c_int;
        fn AT_ConvertBuffer(
            input_buffer: *mut u8,
            output_buffer: *mut u8,
            width: i64,
            height: i64,
            stride: i64,
            input_pixel_encoding: *const c_int,
            output_pixel_encoding: *const c_int,
        ) -> c_int;
    }

    fn stub_sdk_lock() -> MutexGuard<'static, ()> {
        STUB_SDK_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn stub_sdk_image_bytes(d: &Andor3Camera) -> usize {
        d.read_int_feature("ImageSizeBytes").unwrap() as usize
    }

    fn wide(s: &str) -> Vec<c_int> {
        s.bytes()
            .map(c_int::from)
            .chain(std::iter::once(0))
            .collect()
    }

    #[test]
    fn default_properties() {
        let d = Andor3Camera::new();
        assert_eq!(d.device_type(), DeviceType::Camera);
        assert_eq!(d.name(), "Andor sCMOS Camera");
        assert_eq!(d.description(), "SDK3 Device Adapter for sCMOS cameras");
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(d.get_binning(), 1);
        assert!(!d.is_capturing());
        assert_eq!(d.get_number_of_components(), 1);
    }

    #[test]
    fn upstream_metadata_property_aliases_are_present() {
        let d = Andor3Camera::new();
        for prop in [
            "Description",
            "CameraID",
            "CameraModel",
            "CameraFirmware",
            "CurrentSoftware",
            "Ext (Exp) Trigger Timeout[ms]",
            "SensorTemperature",
            "FrameRate",
            "FrameRateLimits",
            "Sensitivity/DynamicRange",
            "TemperatureControl",
            "TemperatureStatus",
            "FanSpeed",
            "FanSpeedRPM",
            "SensorCooling",
            "RollingShutterGlobalClear",
            "Overlap",
            "SpuriousNoiseFilter",
            "StaticBlemishCorrection",
            "ExternalTriggerDelay",
            "AuxOut1 (TTL I/O)",
            "AuxOut2 (TTL I/O)",
            "AuxOut3 (TTL I/O)",
            "ShutterOutputMode",
            "ShutterTransferTime [s]",
            "LightScanPlus-SensorReadoutMode",
            "LightScanPlus-AlternatingReadoutDirection",
            "LightScanPlus-ExposedPixelHeight",
            "LightScanPlus-ScanSpeedControlEnable",
            "LightScanPlus-LineScanSpeed [lines/sec]",
            "RowReadTime",
            "PreTriggerEnable",
            "iStar-PIV",
            "iStar-GateMode",
            "iStar-MCPIntelligate",
            "iStar-MCPGain",
            "iStar-MCPVoltage",
            "iStar-InsertionDelay",
            "iStar-DDGIOCEnable",
            "iStar-DDGIOCNumberOfPulses",
            "iStar-DDGIOCPeriod",
            "iStar-DDGOutputDelay",
            "iStar-DDGOutputEnable",
            "iStar-DDGOutputStepEnable",
            "iStar-DDGStepEnabled",
            "iStar-DDGOpticalWidthEnable",
            "iStar-DDGOutputPolarity",
            "iStar-DDGOutputSelector",
            "iStar-DDGOutputWidth",
            "iStar-DDGStepCount",
            "iStar-DDGStepDelayCoefficientA",
            "iStar-DDGStepDelayCoefficientB",
            "iStar-DDGStepWidthMode",
            "LowDarkCurrentEnable",
        ] {
            assert!(d.has_property(prop), "missing property {prop}");
        }
        assert!(d.is_property_read_only("Description"));
        assert!(d.is_property_read_only("CameraID"));
        assert!(d.is_property_read_only("CameraFirmware"));
        assert!(d.is_property_read_only("FrameRateLimits"));
        assert!(d.is_property_read_only("TemperatureStatus"));
        assert!(!d.is_property_read_only("SensorTemperature"));
        assert!(!d.is_property_read_only("Ext (Exp) Trigger Timeout[ms]"));
    }

    #[test]
    fn set_camera_index_pre_init() {
        let mut d = Andor3Camera::new();
        d.set_property("CameraIndex", PropertyValue::Integer(1))
            .unwrap();
        assert_eq!(d.camera_index, 1);
    }

    #[test]
    fn invalid_camera_index_values_do_not_mutate_cached_state() {
        let mut d = Andor3Camera::new();
        assert_eq!(
            d.set_property("CameraIndex", PropertyValue::Integer(-1))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(d.camera_index, 0);
        assert_eq!(
            d.get_property("CameraIndex").unwrap(),
            PropertyValue::Integer(0)
        );

        assert_eq!(
            d.set_property(
                "CameraIndex",
                PropertyValue::Integer(i64::from(i32::MAX) + 1),
            )
            .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(d.camera_index, 0);
        assert_eq!(
            d.get_property("CameraIndex").unwrap(),
            PropertyValue::Integer(0)
        );
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = Andor3Camera::new();
        d.set_property("Exposure", PropertyValue::Float(50.0))
            .unwrap();
        assert_eq!(d.exposure_ms, 50.0);
        assert_eq!(d.get_exposure(), 50.0);
    }

    #[test]
    fn invalid_exposure_values_do_not_mutate_cached_state() {
        let mut d = Andor3Camera::new();
        assert_eq!(
            d.set_property("Exposure", PropertyValue::Float(-1.0))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(d.get_exposure(), 10.0);

        d.set_exposure(f64::NAN);
        assert_eq!(d.get_exposure(), 10.0);
    }

    #[test]
    fn set_pixel_encoding_pre_init() {
        let mut d = Andor3Camera::new();
        d.set_property("PixelEncoding", PropertyValue::String("Mono12".into()))
            .unwrap();
        assert_eq!(d.pixel_enc, "Mono12");
    }

    #[test]
    fn set_trigger_mode_pre_init() {
        let mut d = Andor3Camera::new();
        d.set_property("TriggerMode", PropertyValue::String("Software".into()))
            .unwrap();
        assert_eq!(d.trigger_mode, "Software");
    }

    #[test]
    fn trigger_mode_accepts_upstream_display_aliases() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();

        d.initialize().unwrap();
        d.set_property(
            "TriggerMode",
            PropertyValue::String("Software (Recommended for Live Mode)".into()),
        )
        .unwrap();

        assert_eq!(d.trigger_mode, "Software");
        assert_eq!(read_enum(d.ctx, "TriggerMode"), Some("Software".into()));

        d.set_property(
            "TriggerMode",
            PropertyValue::String("Internal (Recommended for fast acquisitions)".into()),
        )
        .unwrap();

        assert_eq!(d.trigger_mode, "Internal");
        assert_eq!(read_enum(d.ctx, "TriggerMode"), Some("Internal".into()));
    }

    #[test]
    fn invalid_allowed_values_do_not_mutate_cached_state() {
        let mut d = Andor3Camera::new();
        assert!(d
            .set_property("PixelEncoding", PropertyValue::String("Bogus".into()))
            .is_err());
        assert_eq!(d.pixel_enc, "Mono16");
        assert!(d
            .set_property("Binning", PropertyValue::String("3x3".into()))
            .is_err());
        assert_eq!(d.binning, "1x1");
        assert!(d
            .set_property("TriggerMode", PropertyValue::String("Line1".into()))
            .is_err());
        assert_eq!(d.trigger_mode, "Internal");
    }

    #[test]
    fn readout_properties_are_present() {
        let d = Andor3Camera::new();
        assert!(d.has_property("PixelReadoutRate"));
        assert!(d.has_property("ElectronicShutteringMode"));
    }

    #[test]
    fn srrf_stub_properties_are_present_but_cannot_be_enabled() {
        let mut d = Andor3Camera::new();
        for prop in [
            "SRRF Status",
            "SRRF | Version Information",
            "SRRF | Number of Frames per Time point",
            "SRRF | Radiality Magnification",
            "SRRF | Ring Radius",
            "SRRF | Radiality Temporal Analysis",
            "SRRF | Interpolation Type",
            "SRRF | Enable",
            "SRRF | Save Original Data | Option",
            "SRRF | Save Original Data | Path",
        ] {
            assert!(d.has_property(prop), "{prop}");
        }
        assert!(d.is_property_read_only("SRRF Status"));
        assert!(d.is_property_read_only("SRRF | Version Information"));
        assert_eq!(
            d.get_property("SRRF | Enable").unwrap(),
            PropertyValue::String("Disabled".into())
        );
        assert_eq!(
            d.get_property("SRRF | Radiality Magnification").unwrap(),
            PropertyValue::Integer(4)
        );
        assert_eq!(
            d.get_property("SRRF | Ring Radius").unwrap(),
            PropertyValue::Float(2.0)
        );

        d.set_property(
            "SRRF | Radiality Temporal Analysis",
            PropertyValue::String("MIP".into()),
        )
        .unwrap();
        d.set_property(
            "SRRF | Interpolation Type",
            PropertyValue::String("Fast B-spline".into()),
        )
        .unwrap();
        assert_eq!(
            d.set_property("SRRF | Enable", PropertyValue::String("Enabled".into()))
                .unwrap_err(),
            MmError::NotYetImplemented
        );
        assert_eq!(
            d.get_property("SRRF | Enable").unwrap(),
            PropertyValue::String("Disabled".into())
        );
    }

    #[test]
    fn bool_feature_properties_use_upstream_on_off_strings() {
        let mut d = Andor3Camera::new();
        assert_eq!(
            d.get_property("SensorCooling").unwrap(),
            PropertyValue::String("On".into())
        );
        d.set_property("SensorCooling", PropertyValue::String("Off".into()))
            .unwrap();
        assert_eq!(
            d.get_property("SensorCooling").unwrap(),
            PropertyValue::String("Off".into())
        );
        assert!(d
            .set_property("SensorCooling", PropertyValue::String("maybe".into()))
            .is_err());
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
    fn binning_accepts_mm_integer_and_sdk_enum_forms() {
        let mut d = Andor3Camera::new();
        d.set_binning(2).unwrap();
        assert_eq!(d.get_binning(), 2);
        assert_eq!(d.binning, "2x2");
        assert_eq!(
            d.get_property("Binning").unwrap(),
            PropertyValue::String("2x2".into())
        );

        d.set_property("Binning", PropertyValue::String("4x4".into()))
            .unwrap();
        assert_eq!(d.get_binning(), 4);
        assert_eq!(d.binning, "4x4");
        assert_eq!(
            d.get_property("Binning").unwrap(),
            PropertyValue::String("4x4".into())
        );

        assert_eq!(
            d.set_property("Binning", PropertyValue::String("2x4".into()))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn roi_origin_uses_upstream_binning_coordinate_mapping() {
        assert_eq!(mm_roi_origin_to_sdk_zero_based(0, 1), 0);
        assert_eq!(mm_roi_origin_to_sdk_zero_based(10, 1), 10);
        assert_eq!(mm_roi_origin_to_sdk_zero_based(0, 2), 1);
        assert_eq!(mm_roi_origin_to_sdk_zero_based(10, 2), 21);

        assert_eq!(sdk_zero_based_to_mm_roi_origin(0, 1), 0);
        assert_eq!(sdk_zero_based_to_mm_roi_origin(10, 1), 10);
        assert_eq!(sdk_zero_based_to_mm_roi_origin(1, 2), 0);
        assert_eq!(sdk_zero_based_to_mm_roi_origin(21, 2), 10);
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
    fn acquisition_rejects_setting_changes_before_state_mutation() {
        let mut d = Andor3Camera::new();
        d.capturing = true;

        assert_eq!(
            d.set_property("Exposure", PropertyValue::Float(50.0))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(
            d.set_property("Binning", PropertyValue::String("2x2".into()))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(d.get_binning(), 1);
        assert_eq!(
            d.set_property("TriggerMode", PropertyValue::String("Software".into()))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(d.trigger_mode, "Internal");
    }

    #[test]
    fn initialize_stub_camera_applies_pre_init_settings_and_defaults() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.set_property("PixelEncoding", PropertyValue::String("Mono32".into()))
            .unwrap();
        d.set_property("TriggerMode", PropertyValue::String("Software".into()))
            .unwrap();
        d.set_binning(2).unwrap();

        d.initialize().unwrap();

        assert!(d.sdk_open);
        assert_eq!(
            d.get_property("PixelEncoding").unwrap(),
            PropertyValue::String("Mono32".into())
        );
        assert_eq!(
            d.get_property("TriggerMode").unwrap(),
            PropertyValue::String("Software".into())
        );
        assert_eq!(
            d.get_property("Binning").unwrap(),
            PropertyValue::String("2x2".into())
        );
        assert_eq!(d.get_binning(), 2);
        assert_eq!(d.get_image_width(), 32);
        assert_eq!(d.get_image_height(), 24);
        assert_eq!(d.get_image_bytes_per_pixel(), 4);
        assert_eq!(d.get_bit_depth(), 32);
        assert_eq!(
            d.get_property("SensorCooling").unwrap(),
            PropertyValue::String("On".into())
        );
        assert_eq!(
            d.get_property("Overlap").unwrap(),
            PropertyValue::String("On".into())
        );
        assert_eq!(
            d.get_property("CameraID").unwrap(),
            PropertyValue::String("SDK3-STUB".into())
        );
        assert_eq!(
            d.get_property("CurrentSoftware").unwrap(),
            PropertyValue::String("SW-STUB".into())
        );

        d.snap_image().unwrap();
        assert_eq!(
            d.get_image_buffer().unwrap().len(),
            (d.get_image_width() * d.get_image_height() * d.get_image_bytes_per_pixel()) as usize
        );
    }

    #[test]
    fn stub_binning_updates_image_geometry_and_full_aoi() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();

        d.initialize().unwrap();
        assert_eq!((d.get_image_width(), d.get_image_height()), (64, 48));

        d.set_binning(2).unwrap();
        assert_eq!(
            d.get_property("Binning").unwrap(),
            PropertyValue::String("2x2".into())
        );
        assert_eq!((d.get_image_width(), d.get_image_height()), (32, 24));

        d.set_roi(ImageRoi::new(0, 0, 16, 12)).unwrap();
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(0, 0, 16, 12));
        assert_eq!((d.get_image_width(), d.get_image_height()), (16, 12));

        d.clear_roi().unwrap();
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(0, 0, 32, 24));
        assert_eq!((d.get_image_width(), d.get_image_height()), (32, 24));
        d.snap_image().unwrap();
        assert_eq!(d.get_image_buffer().unwrap().len(), 32 * 24 * 2);
    }

    #[test]
    fn initialize_stub_camera_enables_upstream_metadata_defaults() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();

        d.initialize().unwrap();

        for feature in ["MetadataEnable", "MetadataTimestamp"] {
            let f = cstr(feature);
            let mut value = 0;
            assert_eq!(
                unsafe { ffi::andor3_get_bool(d.ctx, f.as_ptr(), &mut value) },
                0,
                "stub should expose {feature}"
            );
            assert_eq!(value, 1, "initialize should enable {feature}");
        }
    }

    #[test]
    fn stub_reports_static_sdk_features_read_only_and_rejects_writes() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();

        d.initialize().unwrap();

        let sensor_width = cstr("SensorWidth");
        assert_eq!(
            unsafe { ffi::andor3_is_read_only(d.ctx, sensor_width.as_ptr()) },
            1
        );
        assert_ne!(
            unsafe { ffi::andor3_set_int(d.ctx, sensor_width.as_ptr(), 12) },
            0
        );
        assert_eq!(unsafe { ffi::andor3_get_sensor_width(d.ctx) }, 64);

        let exposure = cstr("ExposureTime");
        assert_eq!(
            unsafe { ffi::andor3_is_read_only(d.ctx, exposure.as_ptr()) },
            0
        );
        assert_eq!(
            unsafe { ffi::andor3_set_float(d.ctx, exposure.as_ptr(), 0.025) },
            0
        );
    }

    #[test]
    fn stub_enum_index_writes_validate_and_update_state() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();

        d.initialize().unwrap();

        let pixel_encoding = cstr("PixelEncoding");
        assert_ne!(
            unsafe { ffi::andor3_set_enum_index(d.ctx, pixel_encoding.as_ptr(), 99) },
            0
        );
        assert_eq!(
            d.get_property("PixelEncoding").unwrap(),
            PropertyValue::String("Mono16".into())
        );

        assert_eq!(
            unsafe { ffi::andor3_set_enum_index(d.ctx, pixel_encoding.as_ptr(), 3) },
            0
        );
        assert_eq!(
            read_enum(d.ctx, "PixelEncoding"),
            Some("Mono32".to_string())
        );
        d.sync_dims();
        assert_eq!(d.get_image_bytes_per_pixel(), 4);
        assert_eq!(d.get_bit_depth(), 32);
    }

    #[test]
    fn stub_commands_only_accept_command_features() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();

        d.initialize().unwrap();

        let acquisition_start = cstr("AcquisitionStart");
        assert_eq!(
            unsafe { ffi::andor3_command(d.ctx, acquisition_start.as_ptr()) },
            0
        );
        let exposure_time = cstr("ExposureTime");
        assert_ne!(
            unsafe { ffi::andor3_command(d.ctx, exposure_time.as_ptr()) },
            0
        );
    }

    #[test]
    fn stub_queries_reject_invalid_handles() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();

        d.initialize().unwrap();

        let pixel_encoding = wide("PixelEncoding");
        let serial_number = wide("SerialNumber");
        let mut writable = 0;
        let mut count = 0;
        let mut enum_value = [0; 32];
        let mut max_string_length = 0;

        assert_eq!(
            unsafe { AT_IsWritable(STUB_HANDLE, pixel_encoding.as_ptr(), &mut writable) },
            0
        );
        assert_ne!(
            unsafe { AT_IsWritable(INVALID_HANDLE, pixel_encoding.as_ptr(), &mut writable) },
            0
        );
        assert_eq!(
            unsafe { AT_GetEnumCount(STUB_HANDLE, pixel_encoding.as_ptr(), &mut count) },
            0
        );
        assert_ne!(
            unsafe { AT_GetEnumCount(INVALID_HANDLE, pixel_encoding.as_ptr(), &mut count) },
            0
        );
        assert_eq!(
            unsafe {
                AT_GetEnumStringByIndex(
                    STUB_HANDLE,
                    pixel_encoding.as_ptr(),
                    0,
                    enum_value.as_mut_ptr(),
                    enum_value.len() as c_int,
                )
            },
            0
        );
        assert_ne!(
            unsafe {
                AT_GetEnumStringByIndex(
                    INVALID_HANDLE,
                    pixel_encoding.as_ptr(),
                    0,
                    enum_value.as_mut_ptr(),
                    enum_value.len() as c_int,
                )
            },
            0
        );
        assert_eq!(
            unsafe {
                AT_GetStringMaxLength(STUB_HANDLE, serial_number.as_ptr(), &mut max_string_length)
            },
            0
        );
        assert_ne!(
            unsafe {
                AT_GetStringMaxLength(
                    INVALID_HANDLE,
                    serial_number.as_ptr(),
                    &mut max_string_length,
                )
            },
            0
        );
        assert_eq!(unsafe { AT_Flush(STUB_HANDLE) }, 0);
        assert_ne!(unsafe { AT_Flush(INVALID_HANDLE) }, 0);
    }

    #[test]
    fn stub_system_software_version_supports_string_length_query() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();

        d.initialize().unwrap();

        let software_version = wide("SoftwareVersion");
        let mut max_string_length = 0;
        assert_eq!(
            unsafe {
                AT_GetStringMaxLength(
                    AT_HANDLE_SYSTEM,
                    software_version.as_ptr(),
                    &mut max_string_length,
                )
            },
            0
        );
        assert!(max_string_length >= "SW-STUB".len() as c_int + 1);
        assert_eq!(
            d.get_property("CurrentSoftware").unwrap(),
            PropertyValue::String("SW-STUB".into())
        );
    }

    #[test]
    fn initialize_opens_sdk3_utility_library_for_buffer_conversion() {
        let _guard = stub_sdk_lock();
        let mut input = vec![1u8; 8];
        let mut output = vec![0u8; 8];
        let mono16 = wide("Mono16");

        assert_ne!(
            unsafe {
                AT_ConvertBuffer(
                    input.as_mut_ptr(),
                    output.as_mut_ptr(),
                    2,
                    2,
                    4,
                    mono16.as_ptr(),
                    mono16.as_ptr(),
                )
            },
            0
        );

        let mut d = Andor3Camera::new();
        d.initialize().unwrap();

        assert_eq!(
            unsafe {
                AT_ConvertBuffer(
                    input.as_mut_ptr(),
                    output.as_mut_ptr(),
                    2,
                    2,
                    4,
                    mono16.as_ptr(),
                    mono16.as_ptr(),
                )
            },
            0
        );

        d.shutdown().unwrap();
        assert_ne!(
            unsafe {
                AT_ConvertBuffer(
                    input.as_mut_ptr(),
                    output.as_mut_ptr(),
                    2,
                    2,
                    4,
                    mono16.as_ptr(),
                    mono16.as_ptr(),
                )
            },
            0
        );
    }

    #[test]
    fn stub_wait_buffer_returns_queued_buffers_fifo() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();

        d.initialize().unwrap();

        let sdk_image_bytes = stub_sdk_image_bytes(&d);
        let mut first = vec![1u8; sdk_image_bytes];
        let mut second = vec![2u8; sdk_image_bytes];
        assert_eq!(
            unsafe { AT_QueueBuffer(STUB_HANDLE, first.as_mut_ptr(), first.len() as c_int) },
            0
        );
        assert_eq!(
            unsafe { AT_QueueBuffer(STUB_HANDLE, second.as_mut_ptr(), second.len() as c_int) },
            0
        );

        let mut returned = std::ptr::null_mut();
        let mut returned_size = 0;
        assert_eq!(
            unsafe { AT_WaitBuffer(STUB_HANDLE, &mut returned, &mut returned_size, 0) },
            0
        );
        assert_eq!(returned, first.as_mut_ptr());
        assert_eq!(returned_size, first.len() as c_int);

        assert_eq!(
            unsafe { AT_WaitBuffer(STUB_HANDLE, &mut returned, &mut returned_size, 0) },
            0
        );
        assert_eq!(returned, second.as_mut_ptr());
        assert_eq!(returned_size, second.len() as c_int);

        assert_ne!(
            unsafe { AT_WaitBuffer(STUB_HANDLE, &mut returned, &mut returned_size, 0) },
            0
        );
    }

    #[test]
    fn stub_queue_buffer_rejects_buffers_smaller_than_image_size() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();

        d.initialize().unwrap();

        let sdk_image_bytes = stub_sdk_image_bytes(&d);
        let mut too_small = vec![0u8; sdk_image_bytes - 1];
        assert_ne!(
            unsafe {
                AT_QueueBuffer(
                    STUB_HANDLE,
                    too_small.as_mut_ptr(),
                    too_small.len() as c_int,
                )
            },
            0
        );

        let mut returned = std::ptr::null_mut();
        let mut returned_size = 0;
        assert_ne!(
            unsafe { AT_WaitBuffer(STUB_HANDLE, &mut returned, &mut returned_size, 0) },
            0
        );
    }

    #[test]
    fn stub_queue_buffer_rejects_unaligned_buffers() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();

        d.initialize().unwrap();

        let sdk_image_bytes = stub_sdk_image_bytes(&d);
        let mut storage = vec![0u8; sdk_image_bytes + 8];
        let base = storage.as_mut_ptr() as usize;
        let offset = if base & 7 == 0 { 1 } else { 0 };
        let unaligned = unsafe { storage.as_mut_ptr().add(offset) };
        assert_ne!((unaligned as usize) & 7, 0);
        assert_ne!(
            unsafe { AT_QueueBuffer(STUB_HANDLE, unaligned, (storage.len() - offset) as c_int,) },
            0
        );

        let mut returned = std::ptr::null_mut();
        let mut returned_size = 0;
        assert_ne!(
            unsafe { AT_WaitBuffer(STUB_HANDLE, &mut returned, &mut returned_size, 0) },
            0
        );
    }

    #[test]
    fn initialize_applies_pre_init_simple_sdk_properties() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.set_property("SensorCooling", PropertyValue::String("Off".into()))
            .unwrap();
        d.set_property("Overlap", PropertyValue::String("Off".into()))
            .unwrap();
        d.set_property("SensorTemperature", PropertyValue::Float(17.5))
            .unwrap();

        d.initialize().unwrap();

        assert_eq!(
            d.get_property("SensorCooling").unwrap(),
            PropertyValue::String("Off".into())
        );
        assert_eq!(
            d.get_property("Overlap").unwrap(),
            PropertyValue::String("Off".into())
        );
        assert_eq!(
            d.get_property("SensorTemperature").unwrap(),
            PropertyValue::Float(17.5)
        );
    }

    #[test]
    fn simple_float_properties_clamp_to_sdk_limits() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.initialize().unwrap();

        d.set_property("SensorTemperature", PropertyValue::Float(99.0))
            .unwrap();

        assert_eq!(
            d.get_property("SensorTemperature").unwrap(),
            PropertyValue::Float(60.0)
        );
        assert_eq!(d.read_float_feature("SensorTemperature"), Some(60.0));
    }

    #[test]
    fn exposure_property_clamps_to_sdk_limits_after_initialize() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.initialize().unwrap();

        d.set_property("Exposure", PropertyValue::Float(70_000.0))
            .unwrap();

        assert_eq!(d.get_exposure(), 60_000.0);
        assert_eq!(
            d.get_property("Exposure").unwrap(),
            PropertyValue::Float(60_000.0)
        );
        assert_eq!(d.read_float_feature("ExposureTime"), Some(60.0));
    }

    #[test]
    fn initialize_uses_sdk_feature_fallbacks_when_primary_alias_is_missing() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();

        d.initialize().unwrap();

        assert_eq!(
            d.get_property("PixelReadoutRate").unwrap(),
            PropertyValue::String("100 MHz".into())
        );
        assert_eq!(
            d.get_property("Sensitivity/DynamicRange").unwrap(),
            PropertyValue::String("16-bit".into())
        );
    }

    #[test]
    fn unsupported_stub_features_do_not_get_placeholder_values_or_mutate_on_write() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();

        d.initialize().unwrap();

        assert_eq!(
            d.get_property("FrameRate").unwrap(),
            PropertyValue::Float(0.0)
        );
        assert_eq!(
            d.set_property("FrameRate", PropertyValue::Float(17.0))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            d.get_property("FrameRate").unwrap(),
            PropertyValue::Float(0.0)
        );
        assert_eq!(
            d.get_property("TemperatureControl").unwrap(),
            PropertyValue::String("".into())
        );
        assert_eq!(
            d.set_property("TemperatureControl", PropertyValue::String("Cool".into()))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            d.get_property("TemperatureControl").unwrap(),
            PropertyValue::String("".into())
        );
    }

    #[test]
    fn roi_rejects_out_of_sensor_writes_and_zero_size_clears() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.initialize().unwrap();
        let full = ImageRoi::new(0, 0, 64, 48);
        assert_eq!(d.get_roi().unwrap(), full);

        d.set_roi(ImageRoi::new(4, 5, 16, 12)).unwrap();
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(4, 5, 16, 12));

        assert!(d.set_roi(ImageRoi::new(60, 40, 16, 12)).is_err());
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(4, 5, 16, 12));

        d.set_roi(ImageRoi::new(0, 0, 0, 0)).unwrap();
        assert_eq!(d.get_roi().unwrap(), full);
    }

    #[test]
    fn roi_rejects_coordinates_that_overflow_sdk_origin_mapping() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.initialize().unwrap();

        d.set_roi(ImageRoi::new(4, 5, 16, 12)).unwrap();
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(4, 5, 16, 12));

        assert_eq!(
            d.set_roi(ImageRoi::new(u32::MAX, 0, 16, 12)).unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(4, 5, 16, 12));

        assert_eq!(
            d.set_roi(ImageRoi::new(0, 0, u32::MAX, 12)).unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(d.get_roi().unwrap(), ImageRoi::new(4, 5, 16, 12));
    }

    #[test]
    fn finite_sequence_acquisition_stops_after_requested_frames() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.initialize().unwrap();

        d.start_sequence_acquisition(2, 0.0).unwrap();
        assert!(d.is_capturing());
        assert_eq!(d.sequence_remaining, Some(2));

        d.snap_image().unwrap();
        assert!(d.is_capturing());
        assert_eq!(d.sequence_remaining, Some(1));

        d.snap_image().unwrap();
        assert!(!d.is_capturing());
        assert_eq!(d.sequence_remaining, None);
    }

    #[test]
    fn software_trigger_sequence_uses_exposure_end_event_for_next_trigger() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.initialize().unwrap();
        d.set_property("TriggerMode", PropertyValue::String("Software".into()))
            .unwrap();

        d.start_sequence_acquisition(2, 0.0).unwrap();
        d.snap_image().unwrap();
        assert!(d.is_capturing());

        d.snap_image().unwrap();
        assert!(!d.is_capturing());
        assert_eq!(
            d.get_property("LastWaitError").unwrap(),
            PropertyValue::Integer(0)
        );
    }

    #[test]
    fn stub_snap_parses_upstream_metadata_timestamp_trailer() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.initialize().unwrap();

        assert_eq!(
            d.get_property("TimestampClockFrequency").unwrap(),
            PropertyValue::Integer(1_000_000)
        );
        d.snap_image().unwrap();

        assert_eq!(d.get_image_buffer().unwrap().len(), STUB_IMAGE_BYTES);
        assert_eq!(
            d.get_property("LastFPGAFrameTimestamp").unwrap(),
            PropertyValue::Integer(1_000_000)
        );
        assert_eq!(
            d.get_property("ElapsedTime-ms").unwrap(),
            PropertyValue::Float(0.0)
        );
        assert_eq!(
            d.get_property("LastWaitError").unwrap(),
            PropertyValue::Integer(0)
        );
    }

    #[test]
    fn stub_sequence_elapsed_time_uses_fpga_timestamp_frequency() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.initialize().unwrap();

        d.start_sequence_acquisition(2, 0.0).unwrap();
        d.snap_image().unwrap();
        assert_eq!(
            d.get_property("LastFPGAFrameTimestamp").unwrap(),
            PropertyValue::Integer(1_000_000)
        );
        assert_eq!(
            d.get_property("ElapsedTime-ms").unwrap(),
            PropertyValue::Float(0.0)
        );

        d.snap_image().unwrap();
        assert_eq!(
            d.get_property("LastFPGAFrameTimestamp").unwrap(),
            PropertyValue::Integer(1_000_001)
        );
        assert_eq!(
            d.get_property("ElapsedTime-ms").unwrap(),
            PropertyValue::Float(0.001)
        );
        assert!(!d.is_capturing());
    }

    #[test]
    fn finite_sequence_programs_sdk_fixed_cycle_and_frame_count() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.initialize().unwrap();

        d.start_sequence_acquisition(3, 0.0).unwrap();

        assert_eq!(read_enum(d.ctx, "CycleMode"), Some("Fixed".to_string()));
        assert_eq!(d.read_int_feature("FrameCount"), Some(3));
        d.stop_sequence_acquisition().unwrap();

        d.start_sequence_acquisition(0, 0.0).unwrap();
        assert_eq!(
            read_enum(d.ctx, "CycleMode"),
            Some("Continuous".to_string())
        );
        assert_eq!(d.read_int_feature("FrameCount"), Some(3));
        d.stop_sequence_acquisition().unwrap();
    }

    #[test]
    fn stub_fixed_cycle_stops_waiting_after_frame_count() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.initialize().unwrap();

        d.write_enum_feature("CycleMode", "Fixed").unwrap();
        d.write_int_feature("FrameCount", 2).unwrap();

        let sdk_image_bytes = stub_sdk_image_bytes(&d);
        let mut buffers = [
            vec![0u8; sdk_image_bytes],
            vec![0u8; sdk_image_bytes],
            vec![0u8; sdk_image_bytes],
        ];
        for buffer in &mut buffers {
            assert_eq!(
                unsafe { AT_QueueBuffer(STUB_HANDLE, buffer.as_mut_ptr(), buffer.len() as c_int) },
                0
            );
        }

        let acquisition_start = cstr("AcquisitionStart");
        assert_eq!(
            unsafe { ffi::andor3_command(d.ctx, acquisition_start.as_ptr()) },
            0
        );

        let mut returned = std::ptr::null_mut();
        let mut returned_size = 0;
        assert_eq!(
            unsafe { AT_WaitBuffer(STUB_HANDLE, &mut returned, &mut returned_size, 0) },
            0
        );
        assert_eq!(
            unsafe { AT_WaitBuffer(STUB_HANDLE, &mut returned, &mut returned_size, 0) },
            0
        );
        assert_ne!(
            unsafe { AT_WaitBuffer(STUB_HANDLE, &mut returned, &mut returned_size, 0) },
            0
        );
    }

    #[test]
    fn buffer_overflow_event_maps_snap_wait_failure_to_buffer_overflow() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.initialize().unwrap();

        d.write_bool_feature("StubForceHardwareOverflow", true)
            .unwrap();

        assert_eq!(d.snap_image().unwrap_err(), MmError::BufferOverflow);
        assert_eq!(
            d.get_property("LastWaitError").unwrap(),
            PropertyValue::Integer(AT_ERR_HARDWARE_OVERFLOW)
        );
    }

    #[test]
    fn buffer_overflow_event_maps_sequence_wait_failure_to_buffer_overflow() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.initialize().unwrap();

        d.start_sequence_acquisition(0, 0.0).unwrap();
        d.write_bool_feature("StubForceHardwareOverflow", true)
            .unwrap();

        assert_eq!(d.snap_image().unwrap_err(), MmError::BufferOverflow);
        assert!(!d.is_capturing());
        assert_eq!(
            d.get_property("LastWaitError").unwrap(),
            PropertyValue::Integer(AT_ERR_HARDWARE_OVERFLOW)
        );
    }

    #[test]
    fn duplicate_sequence_start_is_busy_and_keeps_existing_count() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.initialize().unwrap();

        d.start_sequence_acquisition(2, 0.0).unwrap();
        assert_eq!(
            d.start_sequence_acquisition(5, 0.0).unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert!(d.is_capturing());
        assert_eq!(d.sequence_remaining, Some(2));

        d.snap_image().unwrap();
        assert_eq!(d.sequence_remaining, Some(1));
        d.stop_sequence_acquisition().unwrap();
    }

    #[test]
    fn invalid_sequence_count_is_rejected_before_starting() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.initialize().unwrap();

        assert_eq!(
            d.start_sequence_acquisition(-1, 0.0).unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert!(!d.is_capturing());
        assert_eq!(d.sequence_remaining, None);
    }

    #[test]
    fn failed_sequence_frame_clears_local_acquisition_state() {
        let _guard = stub_sdk_lock();
        let mut d = Andor3Camera::new();
        d.initialize().unwrap();

        d.start_sequence_acquisition(2, 0.0).unwrap();
        assert!(d.is_capturing());
        unsafe { ffi::andor3_stop_cont(d.ctx) };

        assert_eq!(d.snap_image().unwrap_err(), MmError::SnapImageFailed);
        assert!(!d.is_capturing());
        assert_eq!(d.sequence_remaining, None);
        d.set_property("Exposure", PropertyValue::Float(12.0))
            .unwrap();
        assert_eq!(d.get_exposure(), 12.0);
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
        assert!(!d.is_property_read_only("SensorTemperature"));
        assert!(d.is_property_read_only("SerialNumber"));
        assert!(d.is_property_read_only("CameraID"));
        assert!(d.is_property_read_only("CameraModel"));
        assert!(d.is_property_read_only("CameraFirmware"));
        assert!(!d.is_property_read_only("Exposure"));
        assert!(!d.is_property_read_only("Binning"));
        assert!(!d.is_property_read_only("TriggerMode"));
        assert!(!d.is_property_read_only("PixelEncoding"));
    }

    #[test]
    fn exposure_ms_to_s_conversion() {
        let ms = 33.3_f64;
        let s = ms / 1_000.0;
        assert!((s - 0.0333).abs() < 1e-6);
    }

    #[test]
    fn snap_timeout_at_least_5s() {
        let d = Andor3Camera::new();
        assert!(d.snap_timeout_ms() >= 5_000);
    }

    #[test]
    fn external_trigger_timeout_property_is_used_for_external_modes() {
        let mut d = Andor3Camera::new();
        d.set_property("Exposure", PropertyValue::Float(10.2))
            .unwrap();
        d.set_property(
            "Ext (Exp) Trigger Timeout[ms]",
            PropertyValue::Integer(1_234),
        )
        .unwrap();

        d.set_property("TriggerMode", PropertyValue::String("External".into()))
            .unwrap();
        assert_eq!(d.snap_timeout_ms(), 1_245);

        d.set_property("TriggerMode", PropertyValue::String("Software".into()))
            .unwrap();
        assert_eq!(d.snap_timeout_ms(), 5_011);
    }

    #[test]
    fn negative_external_trigger_timeout_is_rejected() {
        let mut d = Andor3Camera::new();
        assert_eq!(
            d.set_property("Ext (Exp) Trigger Timeout[ms]", PropertyValue::Integer(-1))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn oversized_timeout_inputs_do_not_overflow_shim_timeout() {
        let mut d = Andor3Camera::new();
        assert_eq!(
            d.set_property(
                "Ext (Exp) Trigger Timeout[ms]",
                PropertyValue::Integer(i64::from(i32::MAX) + 1),
            )
            .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            d.get_property("Ext (Exp) Trigger Timeout[ms]").unwrap(),
            PropertyValue::Integer(DEFAULT_SNAP_TIMEOUT_MS)
        );

        d.set_property("Exposure", PropertyValue::Float(f64::MAX))
            .unwrap();
        assert_eq!(d.snap_timeout_ms(), i32::MAX);
    }

    #[test]
    fn public_binning_property_maps_to_sdk_aoi_binning_feature() {
        assert_eq!(sdk_enum_feature_for_property("Binning"), "AOIBinning");
        assert_eq!(
            sdk_enum_feature_for_property("PixelEncoding"),
            "PixelEncoding"
        );
        assert_eq!(sdk_enum_feature_for_property("TriggerMode"), "TriggerMode");
        assert_eq!(
            simple_feature_spec("Sensitivity/DynamicRange")
                .unwrap()
                .fallback_feature,
            Some("SimplePreAmpGainControl")
        );
        assert_eq!(
            simple_feature_spec("AuxOut1 (TTL I/O)")
                .unwrap()
                .primary_feature,
            "AuxOut1"
        );
        assert_eq!(
            simple_feature_spec("AuxOut1 (TTL I/O)")
                .unwrap()
                .fallback_feature,
            Some("AuxiliaryOutSource")
        );
        assert_eq!(
            simple_feature_spec("LightScanPlus-LineScanSpeed [lines/sec]")
                .unwrap()
                .primary_feature,
            "LineScanSpeed"
        );
        assert_eq!(
            simple_feature_spec("iStar-MCPGain")
                .unwrap()
                .primary_feature,
            "MCPGain"
        );
        assert_eq!(
            simple_feature_spec("iStar-DDGOutputPolarity")
                .unwrap()
                .primary_feature,
            "DDGOutputPolarity"
        );
        assert_eq!(
            simple_feature_spec("iStar-DDGStepDelayCoefficientA")
                .unwrap()
                .primary_feature,
            "DDGStepDelayCoefficientA"
        );
    }
}
