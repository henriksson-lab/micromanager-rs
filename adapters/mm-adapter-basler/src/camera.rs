use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Camera, Device};
use micromanager::types::{DeviceType, ImageRoi, PropertyValue};
use pylon_cxx::{GrabOptions, GrabResult, HasProperties, InstantCamera, Pylon, TlFactory};

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

fn pixel_format_bpp(fmt: &str) -> u32 {
    match fmt {
        "Mono8" | "BayerRG8" | "BayerBG8" | "BayerGB8" | "BayerGR8" => 1,
        "Mono10" | "Mono10p" | "Mono12" | "Mono12p" | "Mono16" => 2,
        "RGB8" | "BGR8" => 3,
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
        "RGB8" | "BGR8" | "RGB16" | "BGR16" => 3,
        _ => 1,
    }
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
    serial_number: String,
    exposure_ms: f64,
    gain: f64,
    pixel_format: String,
    binning: i32,
}

impl BaslerCamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("SerialNumber", PropertyValue::String("".into()), false).unwrap();
        props.define_property("Exposure",     PropertyValue::Float(10.0), false).unwrap();
        props.define_property("Gain",         PropertyValue::Float(0.0),  false).unwrap();
        props.define_property("PixelFormat",  PropertyValue::String("Mono8".into()), false).unwrap();
        props.define_property("Binning",      PropertyValue::Integer(1),  false).unwrap();
        props.define_property("Width",        PropertyValue::Integer(0),  true).unwrap();
        props.define_property("Height",       PropertyValue::Integer(0),  true).unwrap();
        props.define_property("Temperature",  PropertyValue::Float(0.0),  true).unwrap();

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
            serial_number: String::new(),
            exposure_ms: 10.0,
            gain: 0.0,
            pixel_format: "Mono8".into(),
            binning: 1,
        }
    }

    fn check_open(&self) -> MmResult<()> {
        if self.camera.is_none() { Err(MmError::NotConnected) } else { Ok(()) }
    }

    fn pylon_err(e: pylon_cxx::PylonError) -> MmError {
        MmError::LocallyDefined(format!("Pylon: {}", e))
    }

    // ── Write helpers (take shared ref to avoid borrow conflicts) ───────────

    fn write_exposure(camera: &InstantCamera<'_>, ms: f64) {
        let us = ms * 1000.0;
        if let Ok(nm) = camera.node_map() {
            if let Ok(mut p) = nm.float_node("ExposureTime") {
                let _ = p.set_value(us);
            } else if let Ok(mut p) = nm.float_node("ExposureTimeAbs") {
                let _ = p.set_value(us);
            }
        }
    }

    fn write_gain(camera: &InstantCamera<'_>, gain: f64) {
        if let Ok(nm) = camera.node_map() {
            if let Ok(mut p) = nm.float_node("Gain") {
                let _ = p.set_value(gain);
            } else if let Ok(mut p) = nm.integer_node("GainRaw") {
                let _ = p.set_value(gain as i64);
            }
        }
    }

    fn write_binning(camera: &InstantCamera<'_>, bin: i32) {
        if let Ok(nm) = camera.node_map() {
            if let Ok(mut p) = nm.integer_node("BinningHorizontal") { let _ = p.set_value(bin as i64); }
            if let Ok(mut p) = nm.integer_node("BinningVertical")   { let _ = p.set_value(bin as i64); }
        }
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
        let Some(camera) = self.camera.as_ref() else { return };
        let Ok(nm) = camera.node_map() else { return };

        if let Ok(p) = nm.integer_node("Width")  { if let Ok(v) = p.value() { self.width  = v as u32; } }
        if let Ok(p) = nm.integer_node("Height") { if let Ok(v) = p.value() { self.height = v as u32; } }
        if let Ok(p) = nm.enum_node("PixelFormat") {
            if let Ok(fmt) = p.value() {
                self.bytes_per_pixel = pixel_format_bpp(&fmt);
                self.bit_depth       = pixel_format_depth(&fmt);
                self.num_components  = pixel_format_components(&fmt);
                self.pixel_format    = fmt;
            }
        }
        self.props.entry_mut("Width") .map(|e| e.value = PropertyValue::Integer(self.width  as i64));
        self.props.entry_mut("Height").map(|e| e.value = PropertyValue::Integer(self.height as i64));
    }

    /// Retrieve one grabbed frame and copy into `self.image_buf`.
    fn fetch_frame(&mut self) -> MmResult<()> {
        let camera = self.camera.as_ref().ok_or(MmError::NotConnected)?;
        let mut result = GrabResult::new().map_err(Self::pylon_err)?;
        camera.retrieve_result(5000, &mut result, pylon_cxx::TimeoutHandling::ThrowException)
            .map_err(Self::pylon_err)?;
        if !result.grab_succeeded().map_err(Self::pylon_err)? {
            return Err(MmError::SnapImageFailed);
        }
        let buf = result.buffer().map_err(Self::pylon_err)?;
        self.image_buf = buf.to_vec();
        if let Ok(w) = result.width()  { self.width  = w; }
        if let Ok(h) = result.height() { self.height = h; }
        Ok(())
    }
}

impl Default for BaslerCamera {
    fn default() -> Self { Self::new() }
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
    fn name(&self) -> &str { "BaslerCamera" }
    fn description(&self) -> &str { "Basler camera (Pylon SDK)" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.camera.is_some() { return Ok(()); }

        // Box the Pylon runtime so its address is stable.
        let pylon = Box::new(Pylon::new());
        // SAFETY: pylon is heap-allocated and lives inside `self.pylon` for the
        // entire lifetime of `camera`.  We drop camera before pylon in Drop.
        let pylon_ref: &'static Pylon = unsafe { &*(pylon.as_ref() as *const Pylon) };
        let tl_factory = TlFactory::instance(pylon_ref);

        let camera: InstantCamera<'static> = if self.serial_number.is_empty() {
            let dev = tl_factory.create_first_device().map_err(Self::pylon_err)?;
            InstantCamera::new(dev).map_err(Self::pylon_err)?
        } else {
            let devices = tl_factory.enumerate_devices().map_err(Self::pylon_err)?;
            let sn = &self.serial_number;
            let info = devices.iter().find(|d| {
                d.property_value("SerialNumber").ok().as_deref() == Some(sn)
            }).ok_or_else(|| {
                MmError::LocallyDefined(format!("Basler camera '{}' not found", sn))
            })?;
            let dev = tl_factory.create_device(info).map_err(Self::pylon_err)?;
            InstantCamera::new(dev).map_err(Self::pylon_err)?
        };

        camera.open().map_err(Self::pylon_err)?;

        // Populate allowed PixelFormat values from the camera.
        if let Ok(nm) = camera.node_map() {
            if let Ok(p) = nm.enum_node("PixelFormat") {
                if let Ok(vals) = p.settable_values() {
                    let refs: Vec<&str> = vals.iter().map(|s| s.as_str()).collect();
                    self.props.set_allowed_values("PixelFormat", &refs).ok();
                }
            }
        }

        self.pylon  = Some(pylon);
        self.camera = Some(camera);

        // Apply pre-init settings to hardware.
        let cam = self.camera.as_ref().unwrap();
        Self::write_exposure(cam, self.exposure_ms);
        Self::write_gain(cam, self.gain);
        Self::write_binning(cam, self.binning);
        let fmt = self.pixel_format.clone();
        Self::write_pixel_format(cam, &fmt);
        self.sync_dimensions();

        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.capturing { self.stop_sequence_acquisition()?; }
        // Drop impl handles camera → pylon order.
        if let Some(cam) = self.camera.take() {
            let _ = cam.close();
        }
        self.pylon = None;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Exposure"     => Ok(PropertyValue::Float(self.exposure_ms)),
            "Gain"         => Ok(PropertyValue::Float(self.gain)),
            "PixelFormat"  => Ok(PropertyValue::String(self.pixel_format.clone())),
            "Binning"      => Ok(PropertyValue::Integer(self.binning as i64)),
            "SerialNumber" => Ok(PropertyValue::String(self.serial_number.clone())),
            "Temperature"  => {
                if let Some(cam) = self.camera.as_ref() {
                    if let Ok(nm) = cam.node_map() {
                        if let Ok(p) = nm.float_node("DeviceTemperature") {
                            if let Ok(t) = p.value() {
                                return Ok(PropertyValue::Float(t));
                            }
                        }
                    }
                }
                self.props.get("Temperature").cloned()
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
                self.exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.exposure_ms))?;
                if let Some(cam) = self.camera.as_ref() { Self::write_exposure(cam, self.exposure_ms); }
                Ok(())
            }
            "Gain" => {
                self.gain = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.gain))?;
                if let Some(cam) = self.camera.as_ref() { Self::write_gain(cam, self.gain); }
                Ok(())
            }
            "PixelFormat" => {
                self.pixel_format = val.as_str().to_string();
                self.props.set(name, val)?;
                let fmt = self.pixel_format.clone();
                if let Some(cam) = self.camera.as_ref() { Self::write_pixel_format(cam, &fmt); }
                self.sync_dimensions();
                Ok(())
            }
            "Binning" => {
                self.binning = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props.set(name, PropertyValue::Integer(self.binning as i64))?;
                if let Some(cam) = self.camera.as_ref() { Self::write_binning(cam, self.binning); }
                self.sync_dimensions();
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

// ─── Camera trait ────────────────────────────────────────────────────────────

impl Camera for BaslerCamera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_open()?;
        if self.capturing {
            return self.fetch_frame();
        }
        let cam = self.camera.as_ref().unwrap();
        cam.start_grabbing(&GrabOptions::default().count(1)).map_err(Self::pylon_err)?;
        let result = self.fetch_frame();
        if let Some(cam) = self.camera.as_ref() { let _ = cam.stop_grabbing(); }
        result
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if self.image_buf.is_empty() {
            Err(MmError::LocallyDefined("No image captured yet".into()))
        } else {
            Ok(&self.image_buf)
        }
    }

    fn get_image_width(&self) -> u32 { self.width }
    fn get_image_height(&self) -> u32 { self.height }
    fn get_image_bytes_per_pixel(&self) -> u32 { self.bytes_per_pixel }
    fn get_bit_depth(&self) -> u32 { self.bit_depth }
    fn get_number_of_components(&self) -> u32 { self.num_components }
    fn get_number_of_channels(&self) -> u32 { 1 }
    fn get_exposure(&self) -> f64 { self.exposure_ms }

    fn set_exposure(&mut self, exp_ms: f64) {
        self.exposure_ms = exp_ms;
        self.props.set("Exposure", PropertyValue::Float(exp_ms)).ok();
        if let Some(cam) = self.camera.as_ref() { Self::write_exposure(cam, exp_ms); }
    }

    fn get_binning(&self) -> i32 { self.binning }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        self.binning = bin;
        self.props.set("Binning", PropertyValue::Integer(bin as i64))?;
        if let Some(cam) = self.camera.as_ref() { Self::write_binning(cam, bin); }
        self.sync_dimensions();
        Ok(())
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        Ok(ImageRoi::new(0, 0, self.width, self.height))
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        let cam = self.camera.as_ref().ok_or(MmError::NotConnected)?;
        let nm = cam.node_map().map_err(Self::pylon_err)?;
        // Width/Height before OffsetX/Y (Basler requirement).
        if let Ok(mut p) = nm.integer_node("Width")   { let _ = p.set_value(roi.width  as i64); }
        if let Ok(mut p) = nm.integer_node("Height")  { let _ = p.set_value(roi.height as i64); }
        if let Ok(mut p) = nm.integer_node("OffsetX") { let _ = p.set_value(roi.x      as i64); }
        if let Ok(mut p) = nm.integer_node("OffsetY") { let _ = p.set_value(roi.y      as i64); }
        self.sync_dimensions();
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        let cam = self.camera.as_ref().ok_or(MmError::NotConnected)?;
        let nm = cam.node_map().map_err(Self::pylon_err)?;
        for name in &["OffsetX", "OffsetY"] {
            if let Ok(mut p) = nm.integer_node(name) { let _ = p.set_value(0); }
        }
        for name in &["Width", "Height"] {
            if let Ok(p) = nm.integer_node(name) {
                if let Ok(max) = p.max() {
                    if let Ok(mut q) = nm.integer_node(name) { let _ = q.set_value(max); }
                }
            }
        }
        self.sync_dimensions();
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, _count: i64, _interval_ms: f64) -> MmResult<()> {
        self.check_open()?;
        if self.capturing { return Ok(()); }
        let cam = self.camera.as_ref().unwrap();
        cam.start_grabbing(&GrabOptions::default()).map_err(Self::pylon_err)?;
        self.capturing = true;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        if !self.capturing { return Ok(()); }
        if let Some(cam) = self.camera.as_ref() { let _ = cam.stop_grabbing(); }
        self.capturing = false;
        Ok(())
    }

    fn is_capturing(&self) -> bool { self.capturing }
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
        assert!(!d.is_capturing());
    }

    #[test]
    fn set_serial_number_pre_init() {
        let mut d = BaslerCamera::new();
        d.set_property("SerialNumber", PropertyValue::String("12345678".into())).unwrap();
        assert_eq!(d.serial_number, "12345678");
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = BaslerCamera::new();
        d.set_property("Exposure", PropertyValue::Float(25.0)).unwrap();
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
        assert!(d.initialize().is_err());
    }
}
