/// GenICam camera via the Aravis library.
///
/// Aravis provides a uniform C API for USB3 Vision and GigE Vision cameras.
/// This adapter wraps the safe `aravis` Rust crate.
///
/// Exposure is stored in milliseconds (MicroManager convention) and converted
/// to microseconds for the Aravis API.
use aravis::prelude::*;

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Camera, Device};
use crate::types::{DeviceType, ImageRoi, PropertyValue};

// ─── Pixel format helpers ────────────────────────────────────────────────────

const SUPPORTED_PIXEL_FORMATS: &[&str] = &[
    "Mono8",
    "Mono10",
    "Mono12",
    "Mono14",
    "Mono16",
    "BayerRG8",
    "BayerRG10",
    "BayerRG12",
    "BayerRG16",
    "RGB8",
    "BGR8",
];

fn is_supported_pixel_format(fmt: &str) -> bool {
    SUPPORTED_PIXEL_FORMATS.contains(&fmt)
}

fn pixel_format_bpp(fmt: &str) -> u32 {
    match fmt {
        "Mono8" | "BayerRG8" => 1,
        "Mono10" | "Mono12" | "Mono14" | "Mono16" | "BayerRG10" | "BayerRG12" | "BayerRG16" => 2,
        "RGB8" | "BGR8" => 4, // expanded to RGBA/BGRA
        _ => 1,
    }
}

fn pixel_format_depth(fmt: &str) -> u32 {
    match fmt {
        "Mono10" | "BayerRG10" => 10,
        "Mono12" => 12,
        "BayerRG12" => 12,
        "Mono14" => 14,
        "Mono16" | "BayerRG16" => 16,
        _ => 8,
    }
}

fn pixel_format_components(fmt: &str) -> u32 {
    match fmt {
        "RGB8" | "BGR8" => 4, // RGBA
        _ => 1,
    }
}

/// Convert RGB8 (3 bytes/pixel) to RGBA (4 bytes/pixel).
fn rgb_to_rgba(src: &[u8], width: u32, height: u32) -> Vec<u8> {
    let npix = (width * height) as usize;
    let mut dst = vec![0u8; npix * 4];
    for i in 0..npix {
        dst[i * 4] = src[i * 3];
        dst[i * 4 + 1] = src[i * 3 + 1];
        dst[i * 4 + 2] = src[i * 3 + 2];
        dst[i * 4 + 3] = 255;
    }
    dst
}

// ─── Camera struct ───────────────────────────────────────────────────────────

unsafe impl Send for AravisCamera {}

const NUM_STREAM_BUFFERS: usize = 20;

pub struct AravisCamera {
    props: PropertyMap,
    camera: Option<aravis::Camera>,
    stream: Option<aravis::Stream>,
    img_buf: Vec<u8>,
    width: u32,
    height: u32,
    bytes_per_pixel: u32,
    bit_depth: u32,
    num_components: u32,
    exposure_ms: f64,
    binning: i32,
    pixel_format: String,
    pixel_format_configured: bool,
    capturing: bool,
    device_id: String,
}

impl AravisCamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("DeviceId", PropertyValue::String("".into()), false)
            .unwrap();
        props
            .define_property("Exposure", PropertyValue::Float(10.0), false)
            .unwrap();
        props
            .define_property("Gain", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("PixelType", PropertyValue::String("Mono8".into()), false)
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
            camera: None,
            stream: None,
            img_buf: Vec::new(),
            width: 0,
            height: 0,
            bytes_per_pixel: 1,
            bit_depth: 8,
            num_components: 1,
            exposure_ms: 10.0,
            binning: 1,
            pixel_format: "Mono8".into(),
            pixel_format_configured: false,
            capturing: false,
            device_id: String::new(),
        }
    }

    fn check_open(&self) -> MmResult<()> {
        if self.camera.is_none() {
            Err(MmError::NotConnected)
        } else {
            Ok(())
        }
    }

    fn arv_err(e: glib::Error) -> MmError {
        MmError::LocallyDefined(format!("Aravis: {}", e))
    }

    // ── GenICam node write helpers ───────────────────────────────────────────

    fn write_exposure(camera: &aravis::Camera, ms: f64) {
        let us = ms * 1000.0;
        // Clamp to hardware bounds
        if let Ok((min, max)) = camera.exposure_time_bounds() {
            let clamped = us.clamp(min, max);
            let _ = camera.set_exposure_time(clamped);
        } else {
            let _ = camera.set_exposure_time(us);
        }
        // Disable frame rate limit so exposure is not constrained
        let _ = camera.set_frame_rate(-1.0);
    }

    fn write_gain(camera: &aravis::Camera, gain: f64) {
        let _ = camera.set_gain(gain);
    }

    fn write_binning(camera: &aravis::Camera, bin: i32) {
        let _ = camera.set_binning(bin, bin);
    }

    fn write_pixel_format(camera: &aravis::Camera, fmt: &str) {
        let _ = camera.set_pixel_format_from_string(fmt);
    }

    /// Sync cached dimensions and pixel format metadata from the camera.
    fn sync_dimensions(&mut self) {
        let Some(camera) = self.camera.as_ref() else {
            return;
        };
        if let Ok((_, _, w, h)) = camera.region() {
            self.width = w as u32;
            self.height = h as u32;
        }
        if let Ok(fmt_str) = camera.pixel_format_as_string() {
            self.bytes_per_pixel = pixel_format_bpp(&fmt_str);
            self.bit_depth = pixel_format_depth(&fmt_str);
            self.num_components = pixel_format_components(&fmt_str);
            self.pixel_format = fmt_str;
        }
        self.props
            .entry_mut("PixelType")
            .map(|e| e.value = PropertyValue::String(self.pixel_format.clone()));
        self.props
            .entry_mut("Width")
            .map(|e| e.value = PropertyValue::Integer(self.width as i64));
        self.props
            .entry_mut("Height")
            .map(|e| e.value = PropertyValue::Integer(self.height as i64));
    }

    /// Fetch one frame from a buffer and copy into `self.img_buf`.
    fn process_buffer(&mut self, buffer: &aravis::Buffer) -> MmResult<()> {
        let status = buffer.status();
        if status != aravis::BufferStatus::Success {
            return Err(MmError::SnapImageFailed);
        }

        let data = buffer.data().ok_or(MmError::SnapImageFailed)?;
        let w = buffer.image_width() as u32;
        let h = buffer.image_height() as u32;

        // Check if this is RGB8/BGR8 that needs RGBA expansion
        let fmt = buffer.image_pixel_format();
        let is_rgb =
            fmt == aravis::PixelFormat::RGB_8_PACKED || fmt == aravis::PixelFormat::BGR_8_PACKED;

        if is_rgb {
            self.img_buf = rgb_to_rgba(data, w, h);
        } else {
            self.img_buf = data.to_vec();
        }

        self.width = w;
        self.height = h;
        // Update format metadata from the buffer's pixel format
        if let Ok(fmt_str) = self
            .camera
            .as_ref()
            .map(|c| c.pixel_format_as_string())
            .transpose()
        {
            if let Some(fmt_str) = fmt_str {
                self.bytes_per_pixel = pixel_format_bpp(&fmt_str);
                self.bit_depth = pixel_format_depth(&fmt_str);
                self.num_components = pixel_format_components(&fmt_str);
            }
        }

        Ok(())
    }
}

impl Default for AravisCamera {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Device trait ────────────────────────────────────────────────────────────

impl Device for AravisCamera {
    fn name(&self) -> &str {
        "AravisCamera"
    }
    fn description(&self) -> &str {
        "GenICam camera (Aravis)"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.camera.is_some() {
            return Ok(());
        }

        let camera = if self.device_id.is_empty() {
            aravis::Camera::new(None).map_err(Self::arv_err)?
        } else {
            aravis::Camera::new(Some(&self.device_id)).map_err(Self::arv_err)?
        };

        // Disable auto-exposure
        let _ = camera.set_exposure_time_auto(aravis::Auto::Off);

        // Populate allowed pixel formats
        if let Ok(formats) = camera.dup_available_pixel_formats_as_strings() {
            let supported: Vec<&str> = formats
                .iter()
                .map(|s| s.as_str())
                .filter(|fmt| is_supported_pixel_format(fmt))
                .collect();
            let refs = if supported.is_empty() {
                SUPPORTED_PIXEL_FORMATS.to_vec()
            } else {
                supported
            };
            self.props.set_allowed_values("PixelType", &refs).ok();
        }

        self.camera = Some(camera);

        // Apply pre-init settings
        let cam = self.camera.as_ref().unwrap();
        Self::write_exposure(cam, self.exposure_ms);
        Self::write_gain(cam, self.gain());
        Self::write_binning(cam, self.binning);
        if self.pixel_format_configured {
            let fmt = self.pixel_format.clone();
            Self::write_pixel_format(cam, &fmt);
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
        self.stream = None;
        self.camera = None;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Exposure" => Ok(PropertyValue::Float(self.exposure_ms)),
            "Gain" => {
                if let Some(cam) = self.camera.as_ref() {
                    if let Ok(g) = cam.gain() {
                        return Ok(PropertyValue::Float(g));
                    }
                }
                self.props.get(name).cloned()
            }
            "PixelType" => Ok(PropertyValue::String(self.pixel_format.clone())),
            "Binning" => Ok(PropertyValue::Integer(self.binning as i64)),
            "DeviceId" => Ok(PropertyValue::String(self.device_id.clone())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "DeviceId" => {
                if self.camera.is_some() {
                    return Err(MmError::LocallyDefined(
                        "DeviceId cannot be changed after initialize()".into(),
                    ));
                }
                self.device_id = val.as_str().to_string();
                self.props.set(name, val)
            }
            "Exposure" => {
                self.exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.exposure_ms))?;
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_exposure(cam, self.exposure_ms);
                }
                Ok(())
            }
            "Gain" => {
                let g = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(g))?;
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_gain(cam, g);
                }
                Ok(())
            }
            "PixelType" => {
                let fmt = val.as_str();
                if !is_supported_pixel_format(fmt) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.pixel_format = fmt.to_string();
                self.pixel_format_configured = true;
                self.props.set(name, val)?;
                let fmt = self.pixel_format.clone();
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_pixel_format(cam, &fmt);
                }
                self.sync_dimensions();
                Ok(())
            }
            "Binning" => {
                self.binning = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props
                    .set(name, PropertyValue::Integer(self.binning as i64))?;
                if let Some(cam) = self.camera.as_ref() {
                    Self::write_binning(cam, self.binning);
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

impl Camera for AravisCamera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_open()?;
        let camera = self.camera.as_ref().unwrap();

        if self.capturing {
            // During sequence: pop next buffer from stream
            let stream = self.stream.as_ref().ok_or(MmError::SnapImageFailed)?;
            let buffer = stream.pop_buffer().ok_or(MmError::SnapImageFailed)?;
            let result = self.process_buffer(&buffer);
            // Recycle buffer
            if let Some(stream) = self.stream.as_ref() {
                stream.push_buffer(buffer);
            }
            return result;
        }

        // Single-shot: use blocking acquisition
        let buffer = camera.acquisition(0).map_err(Self::arv_err)?;
        self.process_buffer(&buffer)
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
        self.num_components
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
        if let Some(cam) = self.camera.as_ref() {
            Self::write_exposure(cam, exp_ms);
        }
    }

    fn get_binning(&self) -> i32 {
        self.binning
    }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        self.binning = bin;
        self.props
            .set("Binning", PropertyValue::Integer(bin as i64))?;
        if let Some(cam) = self.camera.as_ref() {
            Self::write_binning(cam, bin);
        }
        self.clear_roi().ok();
        self.sync_dimensions();
        Ok(())
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        if let Some(cam) = self.camera.as_ref() {
            if let Ok((x, y, w, h)) = cam.region() {
                return Ok(ImageRoi::new(x as u32, y as u32, w as u32, h as u32));
            }
        }
        Ok(ImageRoi::new(0, 0, self.width, self.height))
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        if roi.width == 0 && roi.height == 0 {
            return self.clear_roi();
        }
        let cam = self.camera.as_ref().ok_or(MmError::NotConnected)?;
        cam.set_region(
            roi.x as i32,
            roi.y as i32,
            roi.width as i32,
            roi.height as i32,
        )
        .map_err(Self::arv_err)?;
        self.sync_dimensions();
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        let cam = self.camera.as_ref().ok_or(MmError::NotConnected)?;
        // Reset offsets first, then set max dimensions
        let _ = cam.set_region(0, 0, 64, 64);
        if let Ok((_, max_w)) = cam.width_bounds() {
            if let Ok((_, max_h)) = cam.height_bounds() {
                let _ = cam.set_region(0, 0, max_w, max_h);
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
        let camera = self.camera.as_ref().unwrap();

        // Set continuous acquisition mode
        camera
            .set_acquisition_mode(aravis::AcquisitionMode::Continuous)
            .map_err(Self::arv_err)?;

        // Create stream (polling mode, no callback)
        let stream = camera.create_stream(None, None).map_err(Self::arv_err)?;

        // Allocate and push buffers
        let payload = camera.payload().map_err(Self::arv_err)? as usize;
        for _ in 0..NUM_STREAM_BUFFERS {
            let buf = aravis::Buffer::new(payload, None);
            stream.push_buffer(buf);
        }

        // Start
        camera.start_acquisition().map_err(Self::arv_err)?;
        self.stream = Some(stream);
        self.capturing = true;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        if !self.capturing {
            return Ok(());
        }
        if let Some(cam) = self.camera.as_ref() {
            let _ = cam.stop_acquisition();
        }
        self.stream = None;
        self.capturing = false;
        Ok(())
    }

    fn is_capturing(&self) -> bool {
        self.capturing
    }
}

// ─── Helper for gain (avoiding borrow conflict in initialize) ────────────────

impl AravisCamera {
    fn gain(&self) -> f64 {
        self.props
            .get("Gain")
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_format_helpers() {
        assert_eq!(pixel_format_bpp("Mono8"), 1);
        assert_eq!(pixel_format_bpp("Mono16"), 2);
        assert_eq!(pixel_format_bpp("RGB8"), 4);
        assert_eq!(pixel_format_bpp("BayerRG12"), 2);
        assert_eq!(pixel_format_bpp("Mono12p"), 1);

        assert_eq!(pixel_format_depth("Mono8"), 8);
        assert_eq!(pixel_format_depth("Mono12"), 12);
        assert_eq!(pixel_format_depth("Mono16"), 16);
        assert_eq!(pixel_format_depth("BayerRG10"), 10);
        assert_eq!(pixel_format_depth("BayerRG12"), 12);
        assert_eq!(pixel_format_depth("BayerBG10"), 8);

        assert_eq!(pixel_format_components("Mono8"), 1);
        assert_eq!(pixel_format_components("RGB8"), 4);
        assert_eq!(pixel_format_components("BayerRG8"), 1);
    }

    #[test]
    fn unsupported_pixel_formats_are_rejected() {
        let mut d = AravisCamera::new();
        assert!(d
            .set_property("PixelType", PropertyValue::String("Mono12p".into()))
            .is_err());
        assert!(d
            .set_property("PixelType", PropertyValue::String("BayerBG8".into()))
            .is_err());
    }

    #[test]
    fn rgb_to_rgba_conversion() {
        let rgb = vec![255, 0, 0, 0, 255, 0]; // 2 pixels: red, green
        let rgba = rgb_to_rgba(&rgb, 2, 1);
        assert_eq!(rgba, vec![255, 0, 0, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn zero_sized_roi_requires_connected_clear_path() {
        let mut d = AravisCamera::new();
        assert_eq!(
            d.set_roi(ImageRoi::new(0, 0, 0, 0)).unwrap_err(),
            MmError::NotConnected
        );
    }

    #[test]
    fn default_properties() {
        let d = AravisCamera::new();
        assert_eq!(d.device_type(), DeviceType::Camera);
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(d.get_binning(), 1);
        assert!(!d.is_capturing());
    }

    #[test]
    fn set_device_id_pre_init() {
        let mut d = AravisCamera::new();
        d.set_property("DeviceId", PropertyValue::String("Daheng-12345".into()))
            .unwrap();
        assert_eq!(d.device_id, "Daheng-12345");
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = AravisCamera::new();
        d.set_property("Exposure", PropertyValue::Float(25.0))
            .unwrap();
        assert_eq!(d.exposure_ms, 25.0);
    }

    #[test]
    fn no_image_before_snap() {
        let d = AravisCamera::new();
        assert!(d.get_image_buffer().is_err());
    }

    #[test]
    fn snap_without_init_errors() {
        let mut d = AravisCamera::new();
        assert!(d.snap_image().is_err());
    }
}
