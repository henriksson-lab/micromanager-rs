use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Camera, Device};
use micromanager::types::{DeviceType, ImageRoi, PropertyValue};
use opencv::core::Mat;
use opencv::imgproc;
use opencv::prelude::*;
use opencv::videoio::{self, VideoCapture, VideoCaptureTrait, VideoCaptureTraitConst};

/// Pixel format selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PixelFormat {
    Gray8,
    Bgr8,
}

impl PixelFormat {
    fn as_str(self) -> &'static str {
        match self {
            PixelFormat::Gray8 => "GRAY8",
            PixelFormat::Bgr8 => "BGR8",
        }
    }

    fn bytes_per_pixel(self) -> u32 {
        match self {
            PixelFormat::Gray8 => 1,
            PixelFormat::Bgr8 => 3,
        }
    }

    fn channels(self) -> u32 {
        match self {
            PixelFormat::Gray8 => 1,
            PixelFormat::Bgr8 => 3,
        }
    }
}

/// OpenCV VideoCapture camera adapter.
pub struct OpenCvCamera {
    props: PropertyMap,
    device_index: i32,
    cap: Option<VideoCapture>,
    image_buf: Vec<u8>,
    width: u32,
    height: u32,
    exposure_ms: f64,
    binning: i32,
    roi: ImageRoi,
    pixel_format: PixelFormat,
    capturing: bool,
}

impl OpenCvCamera {
    /// Create a new adapter for the given OpenCV device index.
    ///
    /// `index` is passed directly to `VideoCapture::new()`:
    /// - `0` = default/first camera
    /// - `1`, `2`, … = additional cameras
    /// - Negative values or large indices select specific backends
    ///   (e.g. `cv::CAP_GSTREAMER`, `cv::CAP_V4L2`)
    pub fn new(index: i32) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("CameraIndex",  PropertyValue::Integer(index as i64), true).unwrap();
        props.define_property("FrameWidth",   PropertyValue::Integer(0), true).unwrap();
        props.define_property("FrameHeight",  PropertyValue::Integer(0), true).unwrap();
        props.define_property("FPS",          PropertyValue::Float(30.0), false).unwrap();
        props.define_property("PixelFormat",  PropertyValue::String("GRAY8".into()), false).unwrap();
        props.set_allowed_values("PixelFormat", &["GRAY8", "BGR8"]).unwrap();
        props.define_property("Exposure",     PropertyValue::Float(-1.0), false).unwrap();

        Self {
            props,
            device_index: index,
            cap: None,
            image_buf: Vec::new(),
            width: 0,
            height: 0,
            exposure_ms: -1.0,   // -1 = auto
            binning: 1,
            roi: ImageRoi::new(0, 0, 0, 0),
            pixel_format: PixelFormat::Gray8,
            capturing: false,
        }
    }

    /// Read one frame from the capture device and write into `self.image_buf`,
    /// cropping to `self.roi` and converting to the selected pixel format.
    fn grab_into_buf(&mut self) -> MmResult<()> {
        let mut raw = Mat::default();
        {
            let cap = self.cap.as_mut().ok_or(MmError::NotConnected)?;
            let ok = cap.read(&mut raw)
                .map_err(|e| MmError::LocallyDefined(e.to_string()))?;
            if !ok || raw.empty() {
                return Err(MmError::LocallyDefined("OpenCV read() returned empty frame".into()));
            }
        }

        // Crop to ROI if not full-frame
        let roi_rect = opencv::core::Rect::new(
            self.roi.x as i32,
            self.roi.y as i32,
            self.roi.width as i32,
            self.roi.height as i32,
        );
        let cropped = Mat::roi(&raw, roi_rect)
            .map_err(|e| MmError::LocallyDefined(e.to_string()))?;

        // Convert to target pixel format
        let converted = match self.pixel_format {
            PixelFormat::Gray8 => {
                let mut gray = Mat::default();
                imgproc::cvt_color(&cropped, &mut gray, imgproc::COLOR_BGR2GRAY, 0,
                    opencv::core::AlgorithmHint::ALGO_HINT_DEFAULT)
                    .map_err(|e| MmError::LocallyDefined(e.to_string()))?;
                gray
            }
            PixelFormat::Bgr8 => cropped.clone_pointee(),
        };

        // Copy pixel data into our buffer
        let data: &[u8] = converted.data_bytes()
            .map_err(|e| MmError::LocallyDefined(e.to_string()))?;
        self.image_buf.resize(data.len(), 0);
        self.image_buf.copy_from_slice(data);

        Ok(())
    }

    fn update_size_from_cap(&mut self) -> MmResult<()> {
        let cap = self.cap.as_ref().ok_or(MmError::NotConnected)?;
        let w = cap.get(videoio::CAP_PROP_FRAME_WIDTH)
            .map_err(|e| MmError::LocallyDefined(e.to_string()))? as u32;
        let h = cap.get(videoio::CAP_PROP_FRAME_HEIGHT)
            .map_err(|e| MmError::LocallyDefined(e.to_string()))? as u32;
        self.width = w;
        self.height = h;
        self.roi = ImageRoi::new(0, 0, w, h);
        self.props.entry_mut("FrameWidth").map(|e| e.value = PropertyValue::Integer(w as i64));
        self.props.entry_mut("FrameHeight").map(|e| e.value = PropertyValue::Integer(h as i64));
        Ok(())
    }
}

impl Default for OpenCvCamera {
    fn default() -> Self { Self::new(0) }
}

impl Device for OpenCvCamera {
    fn name(&self) -> &str { "OpenCVgrabber" }
    fn description(&self) -> &str { "OpenCV VideoCapture camera adapter" }

    fn initialize(&mut self) -> MmResult<()> {
        let mut cap = VideoCapture::new(self.device_index, videoio::CAP_ANY)
            .map_err(|e| MmError::LocallyDefined(format!("OpenCV VideoCapture::new: {}", e)))?;

        if !cap.is_opened().map_err(|e| MmError::LocallyDefined(e.to_string()))? {
            return Err(MmError::LocallyDefined(
                format!("OpenCV: failed to open device index {}", self.device_index)
            ));
        }

        // Apply requested FPS
        let fps = self.props.get("FPS")
            .ok().and_then(|v| v.as_f64())
            .unwrap_or(30.0);
        let _ = cap.set(videoio::CAP_PROP_FPS, fps);

        // Apply exposure if not auto (-1)
        if self.exposure_ms >= 0.0 {
            // OpenCV CAP_PROP_EXPOSURE is camera-dependent; many backends expect
            // a log2 value or milliseconds depending on the backend. Pass ms directly.
            let _ = cap.set(videoio::CAP_PROP_EXPOSURE, self.exposure_ms);
        }

        self.cap = Some(cap);
        self.update_size_from_cap()?;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.capturing = false;
        if let Some(mut cap) = self.cap.take() {
            let _ = cap.release();
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Exposure"    => Ok(PropertyValue::Float(self.exposure_ms)),
            "PixelFormat" => Ok(PropertyValue::String(self.pixel_format.as_str().into())),
            _             => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Exposure" => {
                let ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.exposure_ms = ms;
                if let Some(cap) = self.cap.as_mut() {
                    let _ = cap.set(videoio::CAP_PROP_EXPOSURE, ms);
                }
                self.props.set(name, PropertyValue::Float(ms))
            }
            "FPS" => {
                let fps = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if let Some(cap) = self.cap.as_mut() {
                    let _ = cap.set(videoio::CAP_PROP_FPS, fps);
                }
                self.props.set(name, PropertyValue::Float(fps))
            }
            "PixelFormat" => {
                let s = val.as_str().to_string();
                self.pixel_format = match s.as_str() {
                    "GRAY8" => PixelFormat::Gray8,
                    "BGR8"  => PixelFormat::Bgr8,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.props.set(name, PropertyValue::String(s))
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

impl Camera for OpenCvCamera {
    fn snap_image(&mut self) -> MmResult<()> {
        if self.cap.is_none() {
            return Err(MmError::NotConnected);
        }
        self.grab_into_buf()
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if self.image_buf.is_empty() {
            return Err(MmError::LocallyDefined("No image captured yet; call snap_image first".into()));
        }
        Ok(&self.image_buf)
    }

    fn get_image_width(&self) -> u32 { self.roi.width }
    fn get_image_height(&self) -> u32 { self.roi.height }
    fn get_image_bytes_per_pixel(&self) -> u32 { self.pixel_format.bytes_per_pixel() }
    fn get_bit_depth(&self) -> u32 { 8 }
    fn get_number_of_components(&self) -> u32 { self.pixel_format.channels() }
    fn get_number_of_channels(&self) -> u32 { 1 }  // single optical channel

    fn get_exposure(&self) -> f64 { self.exposure_ms }
    fn set_exposure(&mut self, exp_ms: f64) {
        self.exposure_ms = exp_ms;
        if let Some(cap) = self.cap.as_mut() {
            let _ = cap.set(videoio::CAP_PROP_EXPOSURE, exp_ms);
        }
    }

    fn get_binning(&self) -> i32 { self.binning }
    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        if bin != 1 {
            // VideoCapture does not support hardware binning in general;
            // only 1×1 is supported unless the specific backend does.
            return Err(MmError::LocallyDefined(
                "OpenCV VideoCapture does not support hardware binning; use binning=1".into()
            ));
        }
        self.binning = bin;
        Ok(())
    }

    fn get_roi(&self) -> MmResult<ImageRoi> { Ok(self.roi) }
    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        // Validate against sensor size
        if roi.x + roi.width > self.width || roi.y + roi.height > self.height {
            return Err(MmError::LocallyDefined(
                format!("ROI ({},{} {}x{}) exceeds sensor size {}x{}",
                    roi.x, roi.y, roi.width, roi.height, self.width, self.height)
            ));
        }
        self.roi = roi;
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        self.roi = ImageRoi::new(0, 0, self.width, self.height);
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, _count: i64, _interval_ms: f64) -> MmResult<()> {
        if self.cap.is_none() { return Err(MmError::NotConnected); }
        if self.capturing { return Err(MmError::CameraBusyAcquiring); }
        self.capturing = true;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        self.capturing = false;
        Ok(())
    }

    fn is_capturing(&self) -> bool { self.capturing }
}
