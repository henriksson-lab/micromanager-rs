use crate::circular_buffer::ImageFrame;
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Camera, Device, SequenceImageSink};
use crate::types::{DeviceType, ImageRoi, PropertyValue};
#[cfg(feature = "basler-stub")]
#[path = "pylon_cxx_stub.rs"]
mod pylon_cxx;
#[cfg(not(feature = "basler-stub"))]
use ::pylon_cxx;
use pylon_cxx::{GrabOptions, GrabResult, HasProperties, InstantCamera, Pylon, TlFactory};
use std::sync::Arc;

// ─── Safety note ────────────────────────────────────────────────────────────
//
// `InstantCamera<'a>` borrows from `TlFactory<'a>`, which borrows from
// `Pylon`.  Storing all three in one struct creates a self-referential type
// that Rust cannot express with safe lifetimes.
//
// We resolve this with the standard boxed-anchor pattern:
//   1. `Pylon` is heap-allocated behind `Box<Pylon>`.
//   2. We obtain a raw `*const Pylon` and transmute it to `&'static Pylon`
//      so the camera can carry a `'static` lifetime.
//   3. The camera (`Option<InstantCamera<'static>>`) is dropped BEFORE the
//      `Box<Pylon>` in the `Drop` impl — matching the actual borrow order.
//
// This is safe as long as the `Pylon` box is never moved or dropped while the
// camera is alive (both invariants we maintain below).
unsafe impl Send for BaslerCamera {}

// ─── Pixel format helpers ────────────────────────────────────────────────────

const UPSTREAM_PIXEL_TYPES: &[&str] = &[
    "Mono10", "Mono12", "Mono16", "Mono8", "BGR8", "RGB8", "BayerRG8", "BayerBG8", "BayerGR8",
];

fn upstream_supported_pixel_types<'a>(available: &'a [String]) -> Vec<&'a str> {
    UPSTREAM_PIXEL_TYPES
        .iter()
        .filter_map(|candidate| {
            available
                .iter()
                .find(|value| value.as_str() == *candidate)
                .map(|value| value.as_str())
        })
        .collect()
}

fn pixel_format_bpp(fmt: &str) -> u32 {
    match fmt {
        "Mono8" => 1,
        "Mono10" | "Mono10p" | "Mono12" | "Mono12p" | "Mono16" => 2,
        "BayerRG8" | "BayerBG8" | "BayerGB8" | "BayerGR8" => 4,
        "RGB8" | "BGR8" => 4,
        "RGB16" | "BGR16" => 6,
        _ => 1,
    }
}

fn pixel_format_depth(fmt: &str) -> u32 {
    match fmt {
        "Mono10" | "Mono10p" => 10,
        "Mono12" | "Mono12p" => 12,
        "Mono16" | "RGB16" | "BGR16" => 16,
        _ => 8,
    }
}

fn pixel_format_components(fmt: &str) -> u32 {
    match fmt {
        "RGB8" | "BGR8" | "BayerRG8" | "BayerBG8" | "BayerGB8" | "BayerGR8" => 4,
        "RGB16" | "BGR16" => 3,
        _ => 1,
    }
}

fn packed_rgb_to_rgba(src: &[u8], width: u32, height: u32, bgr: bool) -> Vec<u8> {
    let npix = (width as usize).saturating_mul(height as usize);
    let mut dst = vec![0u8; npix * 4];
    for (i, px) in src.chunks_exact(3).take(npix).enumerate() {
        let out = &mut dst[i * 4..i * 4 + 4];
        if bgr {
            out[0] = px[2];
            out[1] = px[1];
            out[2] = px[0];
        } else {
            out[0] = px[0];
            out[1] = px[1];
            out[2] = px[2];
        }
        out[3] = 255;
    }
    dst
}

fn is_bayer8(fmt: &str) -> bool {
    matches!(fmt, "BayerRG8" | "BayerBG8" | "BayerGB8" | "BayerGR8")
}

fn bayer_channel(fmt: &str, x: usize, y: usize) -> usize {
    let even_x = x % 2 == 0;
    let even_y = y % 2 == 0;
    match fmt {
        "BayerRG8" => match (even_x, even_y) {
            (true, true) => 0,
            (false, false) => 2,
            _ => 1,
        },
        "BayerBG8" => match (even_x, even_y) {
            (true, true) => 2,
            (false, false) => 0,
            _ => 1,
        },
        "BayerGB8" => match (even_x, even_y) {
            (false, true) => 2,
            (true, false) => 0,
            _ => 1,
        },
        "BayerGR8" => match (even_x, even_y) {
            (false, true) => 0,
            (true, false) => 2,
            _ => 1,
        },
        _ => 1,
    }
}

fn bayer8_to_rgba(src: &[u8], width: u32, height: u32, fmt: &str) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
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
                    let Some(&v) = src.get(yy * w + xx) else {
                        continue;
                    };
                    let ch = bayer_channel(fmt, xx, yy);
                    sums[ch] += v as u32;
                    counts[ch] += 1;
                }
            }
            let out = &mut dst[(y * w + x) * 4..(y * w + x) * 4 + 4];
            for ch in 0..3 {
                out[ch] = if counts[ch] == 0 {
                    src.get(y * w + x).copied().unwrap_or_default()
                } else {
                    (sums[ch] / counts[ch]) as u8
                };
            }
            out[3] = 255;
        }
    }
    dst
}

// ─── Camera struct ───────────────────────────────────────────────────────────

pub struct BaslerCamera {
    props: PropertyMap,
    /// Stable heap allocation for Pylon runtime. Must outlive `camera`.
    pylon: Option<Box<Pylon>>,
    /// Open camera handle (lifetime faked to 'static; actually borrows pylon).
    camera: Option<InstantCamera<'static>>,
    image_buf: Vec<u8>,
    width: u32,
    height: u32,
    bytes_per_pixel: u32,
    bit_depth: u32,
    num_components: u32,
    capturing: bool,
    sequence_image_sink: Option<Arc<dyn SequenceImageSink>>,
    serial_number: String,
    exposure_ms: f64,
    gain: f64,
    offset: f64,
    pixel_format: String,
    pixel_format_configured: bool,
    pixel_format_selected: bool,
    binning: i32,
    binning_mode: String,
    sensor_readout_mode: String,
    light_source_preset: String,
    trigger_mode: String,
    trigger_source: String,
    shutter_mode: String,
    gain_auto: String,
    exposure_auto: String,
    reverse_x: String,
    reverse_y: String,
    acquisition_framerate_enable: String,
    acquisition_framerate: f64,
    inter_packet_delay: i64,
    device_link_throughput_limit: i64,
}

impl BaslerCamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property(
                "SerialNumber",
                PropertyValue::String("Undefined".into()),
                false,
            )
            .unwrap();
        props
            .define_property("CameraID", PropertyValue::String("Undefined".into()), true)
            .unwrap();
        props
            .define_property("Exposure", PropertyValue::Float(10.0), false)
            .unwrap();
        props
            .define_property("Gain", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("Offset", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("PixelType", PropertyValue::String("Mono8".into()), false)
            .unwrap();
        props
            .define_property("Binning", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .define_property(
                "BinningMode",
                PropertyValue::String("Average".into()),
                false,
            )
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
            .define_property("Temperature", PropertyValue::String("N/A".into()), true)
            .unwrap();
        props
            .define_property(
                "TemperatureState",
                PropertyValue::String("N/A".into()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "SensorReadoutMode",
                PropertyValue::String("Normal".into()),
                false,
            )
            .unwrap();
        props
            .define_property(
                "LightSourcePreset",
                PropertyValue::String("Off".into()),
                false,
            )
            .unwrap();
        props
            .define_property("TriggerMode", PropertyValue::String("Off".into()), false)
            .unwrap();
        props
            .define_property(
                "TriggerSource",
                PropertyValue::String("Software".into()),
                false,
            )
            .unwrap();
        props
            .define_property(
                "ShutterMode",
                PropertyValue::String("Rolling".into()),
                false,
            )
            .unwrap();
        props
            .define_property("GainAuto", PropertyValue::String("Off".into()), false)
            .unwrap();
        props
            .define_property("ExposureAuto", PropertyValue::String("Off".into()), false)
            .unwrap();
        props
            .define_property("ReverseX", PropertyValue::String("0".into()), false)
            .unwrap();
        props
            .define_property("ReverseY", PropertyValue::String("0".into()), false)
            .unwrap();
        props
            .define_property(
                "ResultingFrameRate",
                PropertyValue::String("0".into()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "AcquisitionFramerateEnable",
                PropertyValue::String("0".into()),
                false,
            )
            .unwrap();
        props
            .define_property(
                "AcquisitionFramerate",
                PropertyValue::String("100".into()),
                false,
            )
            .unwrap();
        props
            .define_property(
                "DeviceLinkThroughputLimit",
                PropertyValue::Integer(0),
                false,
            )
            .unwrap();
        props
            .define_property("InterPacketDelay", PropertyValue::Integer(0), false)
            .unwrap();

        let _ = props.set_allowed_values("BinningMode", &["Average", "Sum"]);
        let _ = props.set_allowed_values("TriggerMode", &["Off", "On"]);
        let _ = props.set_allowed_values(
            "TriggerSource",
            &["Software", "Line1", "Line2", "Line3", "Line4"],
        );
        let _ = props.set_allowed_values("GainAuto", &["Off", "Once", "Continuous"]);
        let _ = props.set_allowed_values("ExposureAuto", &["Off", "Once", "Continuous"]);
        let _ = props.set_allowed_values("ReverseX", &["0", "1"]);
        let _ = props.set_allowed_values("ReverseY", &["0", "1"]);
        let _ = props.set_allowed_values("AcquisitionFramerateEnable", &["0", "1"]);

        Self {
            props,
            pylon: None,
            camera: None,
            image_buf: Vec::new(),
            width: 0,
            height: 0,
            bytes_per_pixel: 1,
            bit_depth: 8,
            num_components: 1,
            capturing: false,
            sequence_image_sink: None,
            serial_number: "Undefined".into(),
            exposure_ms: 10.0,
            gain: 0.0,
            offset: 0.0,
            pixel_format: "Mono8".into(),
            pixel_format_configured: false,
            pixel_format_selected: false,
            binning: 1,
            binning_mode: "Average".into(),
            sensor_readout_mode: "Normal".into(),
            light_source_preset: "Off".into(),
            trigger_mode: "Off".into(),
            trigger_source: "Software".into(),
            shutter_mode: "Rolling".into(),
            gain_auto: "Off".into(),
            exposure_auto: "Off".into(),
            reverse_x: "0".into(),
            reverse_y: "0".into(),
            acquisition_framerate_enable: "0".into(),
            acquisition_framerate: 100.0,
            inter_packet_delay: 0,
            device_link_throughput_limit: 0,
        }
    }

    fn check_open(&self) -> MmResult<()> {
        if self.camera.is_none() {
            Err(MmError::NotConnected)
        } else {
            Ok(())
        }
    }

    fn pylon_err(e: pylon_cxx::PylonError) -> MmError {
        MmError::LocallyDefined(format!("Pylon: {}", e))
    }

    fn require_not_capturing(&self) -> MmResult<()> {
        if self.capturing {
            Err(MmError::CameraBusyAcquiring)
        } else {
            Ok(())
        }
    }

    // ── Write helpers (take shared ref to avoid borrow conflicts) ───────────

    fn write_exposure(camera: &InstantCamera<'_>, ms: f64) -> f64 {
        let mut us = ms * 1000.0;
        if let Ok(nm) = camera.node_map() {
            if let Ok(mut p) = nm.float_node("ExposureTime") {
                if let (Ok(min), Ok(max)) = (p.min(), p.max()) {
                    us = us.clamp(min, max);
                }
                let _ = p.set_value(us);
                return us / 1000.0;
            } else if let Ok(mut p) = nm.float_node("ExposureTimeAbs") {
                if let (Ok(min), Ok(max)) = (p.min(), p.max()) {
                    us = us.clamp(min, max);
                }
                let _ = p.set_value(us);
                return us / 1000.0;
            }
        }
        ms
    }

    fn write_gain(camera: &InstantCamera<'_>, gain: f64) -> f64 {
        let mut actual = gain;
        if let Ok(nm) = camera.node_map() {
            if let Ok(mut p) = nm.float_node("Gain") {
                if let (Ok(min), Ok(max)) = (p.min(), p.max()) {
                    actual = actual.clamp(min, max);
                }
                let _ = p.set_value(actual);
                return actual;
            } else if let Ok(mut p) = nm.integer_node("GainRaw") {
                let mut raw = gain.round() as i64;
                if let (Ok(min), Ok(max)) = (p.min(), p.max()) {
                    raw = raw.clamp(min, max);
                }
                let _ = p.set_value(raw);
                return raw as f64;
            }
        }
        gain
    }

    fn write_float_node(camera: &InstantCamera<'_>, node_name: &str, value: f64) -> f64 {
        let mut actual = value;
        if let Ok(nm) = camera.node_map() {
            if let Ok(mut p) = nm.float_node(node_name) {
                if let (Ok(min), Ok(max)) = (p.min(), p.max()) {
                    actual = actual.clamp(min, max);
                }
                let _ = p.set_value(actual);
            }
        }
        actual
    }

    fn write_integer_node(camera: &InstantCamera<'_>, node_name: &str, value: i64) -> i64 {
        let mut actual = value;
        if let Ok(nm) = camera.node_map() {
            if let Ok(mut p) = nm.integer_node(node_name) {
                if let (Ok(min), Ok(max)) = (p.min(), p.max()) {
                    actual = actual.clamp(min, max);
                }
                let _ = p.set_value(actual);
            }
        }
        actual
    }

    fn write_enum_node(camera: &InstantCamera<'_>, node_name: &str, value: &str) {
        if let Ok(nm) = camera.node_map() {
            if let Ok(mut p) = nm.enum_node(node_name) {
                let _ = p.set_value(value);
            }
        }
    }

    fn write_boolish_enum_node(camera: &InstantCamera<'_>, node_name: &str, value: &str) {
        let symbolic = match value {
            "0" => "False",
            "1" => "True",
            other => other,
        };
        Self::write_enum_node(camera, node_name, symbolic);
    }

    fn write_binning(camera: &InstantCamera<'_>, bin: i32) -> i32 {
        let mut actual = bin.max(1) as i64;
        if let Ok(nm) = camera.node_map() {
            if let Ok(p) = nm.integer_node("BinningHorizontal") {
                if let (Ok(min), Ok(max)) = (p.min(), p.max()) {
                    actual = actual.clamp(min, max);
                }
            }
            if let Ok(mut p) = nm.integer_node("BinningHorizontal") {
                let _ = p.set_value(actual);
            }
            if let Ok(mut p) = nm.integer_node("BinningVertical") {
                let _ = p.set_value(actual);
            }
        }
        actual as i32
    }

    fn write_pixel_format(camera: &InstantCamera<'_>, fmt: &str) {
        if let Ok(nm) = camera.node_map() {
            if let Ok(mut p) = nm.enum_node("PixelFormat") {
                let _ = p.set_value(fmt);
            }
        }
    }

    /// Pull Width/Height/PixelFormat from the camera and update internal state.
    fn sync_dimensions(&mut self) {
        let Some(camera) = self.camera.as_ref() else {
            return;
        };
        let Ok(nm) = camera.node_map() else { return };

        if let Ok(p) = nm.integer_node("Width") {
            if let Ok(v) = p.value() {
                self.width = v as u32;
            }
        }
        if let Ok(p) = nm.integer_node("Height") {
            if let Ok(v) = p.value() {
                self.height = v as u32;
            }
        }
        if let Ok(p) = nm.enum_node("PixelFormat") {
            if let Ok(fmt) = p.value() {
                self.bytes_per_pixel = pixel_format_bpp(&fmt);
                self.bit_depth = pixel_format_depth(&fmt);
                self.num_components = pixel_format_components(&fmt);
                self.pixel_format = fmt;
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
    }

    fn apply_cached_genicam_properties(&self, camera: &InstantCamera<'_>) {
        Self::write_float_node(camera, "BlackLevel", self.offset);
        Self::write_enum_node(camera, "BinningModeHorizontal", &self.binning_mode);
        Self::write_enum_node(camera, "BinningModeVertical", &self.binning_mode);
        Self::write_enum_node(camera, "SensorReadoutMode", &self.sensor_readout_mode);
        Self::write_enum_node(camera, "LightSourcePreset", &self.light_source_preset);
        Self::write_enum_node(camera, "TriggerMode", &self.trigger_mode);
        Self::write_enum_node(camera, "TriggerSource", &self.trigger_source);
        Self::write_enum_node(camera, "ShutterMode", &self.shutter_mode);
        Self::write_enum_node(camera, "GainAuto", &self.gain_auto);
        Self::write_enum_node(camera, "ExposureAuto", &self.exposure_auto);
        Self::write_boolish_enum_node(camera, "ReverseX", &self.reverse_x);
        Self::write_boolish_enum_node(camera, "ReverseY", &self.reverse_y);
        Self::write_boolish_enum_node(
            camera,
            "AcquisitionFrameRateEnable",
            &self.acquisition_framerate_enable,
        );
        Self::write_float_node(camera, "AcquisitionFrameRate", self.acquisition_framerate);
        Self::write_float_node(
            camera,
            "AcquisitionFrameRateAbs",
            self.acquisition_framerate,
        );
        Self::write_integer_node(
            camera,
            "DeviceLinkThroughputLimit",
            self.device_link_throughput_limit,
        );
        Self::write_integer_node(camera, "GevSCPD", self.inter_packet_delay);
    }

    /// Retrieve one grabbed frame and copy into `self.image_buf`.
    fn fetch_frame(&mut self) -> MmResult<()> {
        let camera = self.camera.as_ref().ok_or(MmError::NotConnected)?;
        let mut result = GrabResult::new().map_err(Self::pylon_err)?;
        camera
            .retrieve_result(
                5000,
                &mut result,
                pylon_cxx::TimeoutHandling::ThrowException,
            )
            .map_err(Self::pylon_err)?;
        if !result.grab_succeeded().map_err(Self::pylon_err)? {
            return Err(MmError::SnapImageFailed);
        }
        let buf = result.buffer().map_err(Self::pylon_err)?;
        if let Ok(w) = result.width() {
            self.width = w;
        }
        if let Ok(h) = result.height() {
            self.height = h;
        }
        if is_bayer8(&self.pixel_format) {
            self.image_buf = bayer8_to_rgba(buf, self.width, self.height, &self.pixel_format);
        } else if self.pixel_format == "RGB8" {
            self.image_buf = packed_rgb_to_rgba(buf, self.width, self.height, false);
        } else if self.pixel_format == "BGR8" {
            self.image_buf = packed_rgb_to_rgba(buf, self.width, self.height, true);
        } else {
            self.image_buf = buf.to_vec();
        }
        if self.capturing {
            self.emit_sequence_frame_to_sink()?;
        }
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

impl Default for BaslerCamera {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BaslerCamera {
    fn drop(&mut self) {
        // Camera must be dropped before Pylon to respect the borrow order.
        if let Some(cam) = self.camera.take() {
            let _ = cam.close();
            drop(cam);
        }
        drop(self.pylon.take());
    }
}

// ─── Device trait ────────────────────────────────────────────────────────────

impl Device for BaslerCamera {
    fn name(&self) -> &str {
        "BaslerCamera"
    }
    fn description(&self) -> &str {
        "Basler Camera device adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.camera.is_some() {
            return Ok(());
        }

        if self.serial_number.is_empty() || self.serial_number == "Undefined" {
            return Err(MmError::LocallyDefined("Serial number is required".into()));
        }

        // Box the Pylon runtime so its address is stable.
        let pylon = Box::new(Pylon::new());
        // SAFETY: pylon is heap-allocated and lives inside `self.pylon` for the
        // entire lifetime of `camera`.  We drop camera before pylon in Drop.
        let pylon_ref: &'static Pylon = unsafe { &*(pylon.as_ref() as *const Pylon) };
        let tl_factory = TlFactory::instance(pylon_ref);

        let devices = tl_factory.enumerate_devices().map_err(Self::pylon_err)?;
        let sn = &self.serial_number;
        let info = devices
            .iter()
            .find(|d| d.property_value("SerialNumber").ok().as_deref() == Some(sn))
            .ok_or_else(|| MmError::LocallyDefined(format!("Basler camera '{}' not found", sn)))?;
        let dev = tl_factory.create_device(info).map_err(Self::pylon_err)?;
        let camera: InstantCamera<'static> = InstantCamera::new(dev).map_err(Self::pylon_err)?;

        camera.open().map_err(Self::pylon_err)?;
        self.props.entry_mut("CameraID").map(|e| {
            e.value = PropertyValue::String(self.serial_number.clone());
        });

        // Upstream exposes a fixed preferred subset of the PixelFormat node as PixelType.
        if let Ok(nm) = camera.node_map() {
            if let Ok(p) = nm.enum_node("PixelFormat") {
                if let Ok(vals) = p.settable_values() {
                    let refs = upstream_supported_pixel_types(&vals);
                    self.props.set_allowed_values("PixelType", &refs).ok();
                    if !self.pixel_format_configured {
                        if let Some(default_fmt) = refs.last() {
                            self.pixel_format = (*default_fmt).to_string();
                            self.pixel_format_selected = true;
                            self.props.entry_mut("PixelType").map(|e| {
                                e.value = PropertyValue::String(self.pixel_format.clone())
                            });
                        }
                    }
                }
            }
        }

        self.pylon = Some(pylon);
        self.camera = Some(camera);

        // Apply pre-init settings to hardware.
        let cam = self.camera.as_ref().unwrap();
        self.exposure_ms = Self::write_exposure(cam, self.exposure_ms);
        self.gain = Self::write_gain(cam, self.gain);
        self.binning = Self::write_binning(cam, self.binning);
        if self.pixel_format_selected {
            let fmt = self.pixel_format.clone();
            Self::write_pixel_format(cam, &fmt);
        }
        self.apply_cached_genicam_properties(cam);
        self.sync_dimensions();

        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.capturing {
            self.stop_sequence_acquisition()?;
        }
        // Drop impl handles camera → pylon order.
        if let Some(cam) = self.camera.take() {
            let _ = cam.close();
        }
        self.pylon = None;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Exposure" => Ok(PropertyValue::Float(self.exposure_ms)),
            "Gain" => Ok(PropertyValue::Float(self.gain)),
            "Offset" => Ok(PropertyValue::Float(self.offset)),
            "PixelType" => Ok(PropertyValue::String(self.pixel_format.clone())),
            "Binning" => Ok(PropertyValue::Integer(self.binning as i64)),
            "BinningMode" => Ok(PropertyValue::String(self.binning_mode.clone())),
            "SerialNumber" => Ok(PropertyValue::String(self.serial_number.clone())),
            "CameraID" => self.props.get("CameraID").cloned(),
            "SensorReadoutMode" => Ok(PropertyValue::String(self.sensor_readout_mode.clone())),
            "LightSourcePreset" => Ok(PropertyValue::String(self.light_source_preset.clone())),
            "TriggerMode" => Ok(PropertyValue::String(self.trigger_mode.clone())),
            "TriggerSource" => Ok(PropertyValue::String(self.trigger_source.clone())),
            "ShutterMode" => Ok(PropertyValue::String(self.shutter_mode.clone())),
            "GainAuto" => Ok(PropertyValue::String(self.gain_auto.clone())),
            "ExposureAuto" => Ok(PropertyValue::String(self.exposure_auto.clone())),
            "ReverseX" => Ok(PropertyValue::String(self.reverse_x.clone())),
            "ReverseY" => Ok(PropertyValue::String(self.reverse_y.clone())),
            "AcquisitionFramerateEnable" => Ok(PropertyValue::String(
                self.acquisition_framerate_enable.clone(),
            )),
            "AcquisitionFramerate" => Ok(PropertyValue::String(
                self.acquisition_framerate.to_string(),
            )),
            "DeviceLinkThroughputLimit" => {
                Ok(PropertyValue::Integer(self.device_link_throughput_limit))
            }
            "InterPacketDelay" => Ok(PropertyValue::Integer(self.inter_packet_delay)),
            "Temperature" => {
                if let Some(cam) = self.camera.as_ref() {
                    if let Ok(nm) = cam.node_map() {
                        if let Ok(p) = nm.float_node("DeviceTemperature") {
                            if let Ok(t) = p.value() {
                                return Ok(PropertyValue::String(t.to_string()));
                            }
                        }
                    }
                }
                self.props.get("Temperature").cloned()
            }
            "TemperatureState" => {
                if let Some(cam) = self.camera.as_ref() {
                    if let Ok(nm) = cam.node_map() {
                        if let Ok(p) = nm.enum_node("TemperatureState") {
                            if let Ok(t) = p.value() {
                                return Ok(PropertyValue::String(t));
                            }
                        }
                    }
                }
                self.props.get("TemperatureState").cloned()
            }
            "ResultingFrameRate" => {
                if let Some(cam) = self.camera.as_ref() {
                    if let Ok(nm) = cam.node_map() {
                        if let Ok(p) = nm.float_node("ResultingFrameRate") {
                            if let Ok(v) = p.value() {
                                return Ok(PropertyValue::String(v.to_string()));
                            }
                        }
                        if let Ok(p) = nm.float_node("ResultingFrameRateAbs") {
                            if let Ok(v) = p.value() {
                                return Ok(PropertyValue::String(v.to_string()));
                            }
                        }
                    }
                }
                self.props.get("ResultingFrameRate").cloned()
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "SerialNumber" => {
                if self.camera.is_some() {
                    return Err(MmError::LocallyDefined(
                        "SerialNumber cannot be changed after initialize()".into(),
                    ));
                }
                self.serial_number = val.as_str().to_string();
                self.props.set(name, val)
            }
            "Exposure" => {
                self.require_not_capturing()?;
                self.exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.exposure_ms))?;
                if let Some(cam) = self.camera.as_ref() {
                    self.exposure_ms = Self::write_exposure(cam, self.exposure_ms);
                    self.props
                        .set(name, PropertyValue::Float(self.exposure_ms))?;
                }
                Ok(())
            }
            "Gain" => {
                self.require_not_capturing()?;
                self.gain = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.gain))?;
                if let Some(cam) = self.camera.as_ref() {
                    self.gain = Self::write_gain(cam, self.gain);
                    self.props.set(name, PropertyValue::Float(self.gain))?;
                }
                Ok(())
            }
            "Offset" => {
                self.require_not_capturing()?;
                self.offset = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.offset))?;
                if let Some(cam) = self.camera.as_ref() {
                    self.offset = Self::write_float_node(cam, "BlackLevel", self.offset);
                    self.props.set(name, PropertyValue::Float(self.offset))?;
                }
                Ok(())
            }
            "PixelType" => {
                self.require_not_capturing()?;
                self.pixel_format = val.as_str().to_string();
                self.pixel_format_configured = true;
                self.pixel_format_selected = true;
                self.props.set(name, val)?;
                let fmt = self.pixel_format.clone();
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_pixel_format(cam, &fmt);
                }
                self.sync_dimensions();
                Ok(())
            }
            "Binning" => {
                self.require_not_capturing()?;
                self.binning = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props
                    .set(name, PropertyValue::Integer(self.binning as i64))?;
                if let Some(cam) = self.camera.as_ref() {
                    self.binning = Self::write_binning(cam, self.binning);
                    self.props
                        .set(name, PropertyValue::Integer(self.binning as i64))?;
                }
                self.sync_dimensions();
                Ok(())
            }
            "SensorWidth" => {
                self.require_not_capturing()?;
                let mut width = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(width))?;
                if let Some(cam) = self.camera.as_ref() {
                    width = Self::write_integer_node(cam, "Width", width);
                    self.props.set(name, PropertyValue::Integer(width))?;
                }
                self.sync_dimensions();
                Ok(())
            }
            "SensorHeight" => {
                self.require_not_capturing()?;
                let mut height = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(height))?;
                if let Some(cam) = self.camera.as_ref() {
                    height = Self::write_integer_node(cam, "Height", height);
                    self.props.set(name, PropertyValue::Integer(height))?;
                }
                self.sync_dimensions();
                Ok(())
            }
            "BinningMode" => {
                self.require_not_capturing()?;
                self.binning_mode = val.as_str().to_string();
                self.props.set(name, val)?;
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_enum_node(cam, "BinningModeHorizontal", &self.binning_mode);
                    Self::write_enum_node(cam, "BinningModeVertical", &self.binning_mode);
                }
                Ok(())
            }
            "SensorReadoutMode" => {
                self.require_not_capturing()?;
                self.sensor_readout_mode = val.as_str().to_string();
                self.props.set(name, val)?;
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_enum_node(cam, "SensorReadoutMode", &self.sensor_readout_mode);
                }
                Ok(())
            }
            "LightSourcePreset" => {
                self.require_not_capturing()?;
                self.light_source_preset = val.as_str().to_string();
                self.props.set(name, val)?;
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_enum_node(cam, "LightSourcePreset", &self.light_source_preset);
                }
                Ok(())
            }
            "TriggerMode" => {
                self.require_not_capturing()?;
                self.trigger_mode = val.as_str().to_string();
                self.props.set(name, val)?;
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_enum_node(cam, "TriggerMode", &self.trigger_mode);
                }
                Ok(())
            }
            "TriggerSource" => {
                self.require_not_capturing()?;
                self.trigger_source = val.as_str().to_string();
                self.props.set(name, val)?;
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_enum_node(cam, "TriggerSource", &self.trigger_source);
                }
                Ok(())
            }
            "ShutterMode" => {
                self.require_not_capturing()?;
                self.shutter_mode = val.as_str().to_string();
                self.props.set(name, val)?;
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_enum_node(cam, "ShutterMode", &self.shutter_mode);
                }
                Ok(())
            }
            "GainAuto" => {
                self.require_not_capturing()?;
                self.gain_auto = val.as_str().to_string();
                self.props.set(name, val)?;
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_enum_node(cam, "GainAuto", &self.gain_auto);
                }
                Ok(())
            }
            "ExposureAuto" => {
                self.require_not_capturing()?;
                self.exposure_auto = val.as_str().to_string();
                self.props.set(name, val)?;
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_enum_node(cam, "ExposureAuto", &self.exposure_auto);
                }
                Ok(())
            }
            "ReverseX" => {
                self.require_not_capturing()?;
                self.reverse_x = val.as_str().to_string();
                self.props.set(name, val)?;
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_boolish_enum_node(cam, "ReverseX", &self.reverse_x);
                }
                Ok(())
            }
            "ReverseY" => {
                self.require_not_capturing()?;
                self.reverse_y = val.as_str().to_string();
                self.props.set(name, val)?;
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_boolish_enum_node(cam, "ReverseY", &self.reverse_y);
                }
                Ok(())
            }
            "AcquisitionFramerateEnable" => {
                self.require_not_capturing()?;
                self.acquisition_framerate_enable = val.as_str().to_string();
                self.props.set(name, val)?;
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_boolish_enum_node(
                        cam,
                        "AcquisitionFrameRateEnable",
                        &self.acquisition_framerate_enable,
                    );
                }
                Ok(())
            }
            "AcquisitionFramerate" => {
                self.require_not_capturing()?;
                self.acquisition_framerate = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(
                    name,
                    PropertyValue::String(self.acquisition_framerate.to_string()),
                )?;
                if let Some(cam) = self.camera.as_ref() {
                    self.acquisition_framerate = Self::write_float_node(
                        cam,
                        "AcquisitionFrameRate",
                        self.acquisition_framerate,
                    );
                    self.acquisition_framerate = Self::write_float_node(
                        cam,
                        "AcquisitionFrameRateAbs",
                        self.acquisition_framerate,
                    );
                    self.props.set(
                        name,
                        PropertyValue::String(self.acquisition_framerate.to_string()),
                    )?;
                }
                Ok(())
            }
            "DeviceLinkThroughputLimit" => {
                self.require_not_capturing()?;
                self.device_link_throughput_limit =
                    val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(
                    name,
                    PropertyValue::Integer(self.device_link_throughput_limit),
                )?;
                if let Some(cam) = self.camera.as_ref() {
                    self.device_link_throughput_limit = Self::write_integer_node(
                        cam,
                        "DeviceLinkThroughputLimit",
                        self.device_link_throughput_limit,
                    );
                    self.props.set(
                        name,
                        PropertyValue::Integer(self.device_link_throughput_limit),
                    )?;
                }
                Ok(())
            }
            "InterPacketDelay" => {
                self.require_not_capturing()?;
                self.inter_packet_delay = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.inter_packet_delay))?;
                if let Some(cam) = self.camera.as_ref() {
                    self.inter_packet_delay =
                        Self::write_integer_node(cam, "GevSCPD", self.inter_packet_delay);
                    self.props
                        .set(name, PropertyValue::Integer(self.inter_packet_delay))?;
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

impl Camera for BaslerCamera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_open()?;
        if self.capturing {
            return self.fetch_frame();
        }
        let cam = self.camera.as_ref().unwrap();
        cam.start_grabbing(&GrabOptions::default().count(1))
            .map_err(Self::pylon_err)?;
        let result = self.fetch_frame();
        if let Some(cam) = self.camera.as_ref() {
            let _ = cam.stop_grabbing();
        }
        result
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
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        self.props.set("Exposure", PropertyValue::Float(exp_ms))?;
        self.exposure_ms = exp_ms;
        if let Some(cam) = self.camera.as_ref() {
            self.exposure_ms = Self::write_exposure(cam, exp_ms);
            self.props
                .set("Exposure", PropertyValue::Float(self.exposure_ms))
                .ok();
        }
        Ok(())
    }

    fn get_binning(&self) -> i32 {
        self.binning
    }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        self.require_not_capturing()?;
        self.binning = bin;
        self.props
            .set("Binning", PropertyValue::Integer(bin as i64))?;
        if let Some(cam) = self.camera.as_ref() {
            self.binning = Self::write_binning(cam, bin);
            self.props
                .set("Binning", PropertyValue::Integer(self.binning as i64))?;
        }
        self.sync_dimensions();
        Ok(())
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        Ok(ImageRoi::new(0, 0, self.width, self.height))
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        let cam = self.camera.as_ref().ok_or(MmError::NotConnected)?;
        let was_capturing = self.capturing;
        if was_capturing {
            let _ = cam.stop_grabbing();
        }
        let nm = cam.node_map().map_err(Self::pylon_err)?;
        let mut x = roi.x as i64;
        let mut y = roi.y as i64;
        let mut width = roi.width as i64;
        let mut height = roi.height as i64;
        if let Ok(p) = nm.integer_node("Width") {
            if let (Ok(min), Ok(max)) = (p.min(), p.max()) {
                width = width.clamp(min, max);
            }
        }
        if let Ok(p) = nm.integer_node("Height") {
            if let (Ok(min), Ok(max)) = (p.min(), p.max()) {
                height = height.clamp(min, max);
            }
        }
        if let Ok(p) = nm.integer_node("OffsetX") {
            if let (Ok(min), Ok(max)) = (p.min(), p.max()) {
                x = x.clamp(min, max);
            }
        }
        if let Ok(p) = nm.integer_node("OffsetY") {
            if let (Ok(min), Ok(max)) = (p.min(), p.max()) {
                y = y.clamp(min, max);
            }
        }
        // Width/Height before OffsetX/Y (Basler requirement).
        if let Ok(mut p) = nm.integer_node("Width") {
            let _ = p.set_value(width);
        }
        if let Ok(mut p) = nm.integer_node("Height") {
            let _ = p.set_value(height);
        }
        if let Ok(mut p) = nm.integer_node("OffsetX") {
            let _ = p.set_value(x);
        }
        if let Ok(mut p) = nm.integer_node("OffsetY") {
            let _ = p.set_value(y);
        }
        if was_capturing {
            cam.start_grabbing(&GrabOptions::default())
                .map_err(Self::pylon_err)?;
        }
        self.sync_dimensions();
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        let cam = self.camera.as_ref().ok_or(MmError::NotConnected)?;
        let nm = cam.node_map().map_err(Self::pylon_err)?;
        for name in &["OffsetX", "OffsetY"] {
            if let Ok(mut p) = nm.integer_node(name) {
                let _ = p.set_value(0);
            }
        }
        for name in &["Width", "Height"] {
            if let Ok(p) = nm.integer_node(name) {
                if let Ok(max) = p.max() {
                    if let Ok(mut q) = nm.integer_node(name) {
                        let _ = q.set_value(max);
                    }
                }
            }
        }
        self.sync_dimensions();
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, _count: i64, _interval_ms: f64) -> MmResult<()> {
        self.check_open()?;
        if self.capturing {
            return Ok(());
        }
        let cam = self.camera.as_ref().unwrap();
        cam.start_grabbing(&GrabOptions::default())
            .map_err(Self::pylon_err)?;
        self.capturing = true;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        if !self.capturing {
            return Ok(());
        }
        if let Some(cam) = self.camera.as_ref() {
            let _ = cam.stop_grabbing();
        }
        self.capturing = false;
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

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_properties() {
        let d = BaslerCamera::new();
        assert_eq!(d.device_type(), DeviceType::Camera);
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(d.get_binning(), 1);
        assert_eq!(
            d.get_property("CameraID").unwrap(),
            PropertyValue::String("Undefined".into())
        );
        assert!(d.is_property_read_only("CameraID"));
        assert!(!d.is_capturing());
    }

    #[test]
    fn set_serial_number_pre_init() {
        let mut d = BaslerCamera::new();
        d.set_property("SerialNumber", PropertyValue::String("12345678".into()))
            .unwrap();
        assert_eq!(d.serial_number, "12345678");
    }

    #[test]
    fn initialize_without_serial_number_fails_before_enumeration() {
        let mut d = BaslerCamera::new();
        let err = d.initialize().unwrap_err();
        assert_eq!(
            err,
            MmError::LocallyDefined("Serial number is required".into())
        );
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = BaslerCamera::new();
        d.set_property("Exposure", PropertyValue::Float(25.0))
            .unwrap();
        assert_eq!(d.exposure_ms, 25.0);
    }

    #[test]
    fn set_gain_pre_init() {
        let mut d = BaslerCamera::new();
        d.set_property("Gain", PropertyValue::Float(2.5)).unwrap();
        assert_eq!(d.gain, 2.5);
    }

    #[test]
    fn no_image_before_snap() {
        let d = BaslerCamera::new();
        assert!(d.get_image_buffer().is_err());
    }

    #[test]
    fn snap_without_init_errors() {
        let mut d = BaslerCamera::new();
        assert!(d.snap_image().is_err());
    }

    #[test]
    fn initialize_no_camera_fails_gracefully() {
        let mut d = BaslerCamera::new();
        d.set_property("SerialNumber", PropertyValue::String("12345678".into()))
            .unwrap();
        assert!(d.initialize().is_err());
    }

    #[test]
    fn upstream_pixel_type_subset_preserves_preference_order() {
        let available = vec![
            "Mono8".to_string(),
            "Coord3D_ABC32f".to_string(),
            "BayerGR8".to_string(),
            "Mono12".to_string(),
            "RGB8".to_string(),
        ];
        let supported = upstream_supported_pixel_types(&available);
        assert_eq!(supported, vec!["Mono12", "Mono8", "RGB8", "BayerGR8"]);
        assert_eq!(supported.last().copied(), Some("BayerGR8"));
    }

    #[test]
    fn rgb_and_bgr_pixel_types_report_rgba_buffers() {
        assert_eq!(pixel_format_bpp("RGB8"), 4);
        assert_eq!(pixel_format_bpp("BGR8"), 4);
        assert_eq!(pixel_format_components("RGB8"), 4);
        assert_eq!(pixel_format_components("BGR8"), 4);
        assert_eq!(pixel_format_bpp("BayerRG8"), 4);
        assert_eq!(pixel_format_components("BayerRG8"), 4);

        assert_eq!(
            packed_rgb_to_rgba(&[1, 2, 3], 1, 1, false),
            vec![1, 2, 3, 255]
        );
        assert_eq!(
            packed_rgb_to_rgba(&[1, 2, 3], 1, 1, true),
            vec![3, 2, 1, 255]
        );
    }

    #[test]
    fn bayer8_pixel_types_expand_to_rgba() {
        let rgba = bayer8_to_rgba(&[10, 20, 30, 40], 2, 2, "BayerRG8");
        assert_eq!(rgba.len(), 2 * 2 * 4);
        assert_eq!(&rgba[0..4], &[10, 25, 40, 255]);
        assert_eq!(pixel_format_depth("BayerRG8"), 8);
    }

    #[test]
    fn acquisition_rejects_mutating_settings_before_state_changes() {
        let mut d = BaslerCamera::new();
        d.capturing = true;

        assert_eq!(
            d.set_property("Exposure", PropertyValue::Float(25.0))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(
            d.set_property("Gain", PropertyValue::Float(2.0))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(d.gain, 0.0);
        assert_eq!(
            d.set_property("PixelType", PropertyValue::String("RGB8".into()))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(d.pixel_format, "Mono8");
        assert_eq!(d.set_binning(2).unwrap_err(), MmError::CameraBusyAcquiring);
        assert_eq!(d.get_binning(), 1);

        let _ = d.set_exposure(50.0);
        assert_eq!(d.get_exposure(), 10.0);
    }
}
