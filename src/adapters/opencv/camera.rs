use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Camera, Device};
use crate::types::{DeviceType, ImageRoi, PropertyValue};
use opencv::core::{self, Mat};
use opencv::imgproc;
use opencv::prelude::*;
use opencv::videoio::{self, VideoCapture, VideoCaptureTrait, VideoCaptureTraitConst};

const RESOLUTIONS: &[&str] = &[
    "320x200",
    "320x240",
    "340x256",
    "480x320",
    "640x480",
    "680x512",
    "720x480",
    "720x576",
    "768x512",
    "768x576",
    "800x480",
    "800x600",
    "854x480",
    "800x480",
    "1024x600",
    "1024x768",
    "1136x768",
    "1280x720",
    "1280x800",
    "1280x960",
    "1280x1024",
    "1360x1024",
    "1400x1050",
    "1440x900",
    "1440x960",
    "1600x1200",
    "1680x1050",
    "1920x1080",
    "1920x1200",
    "2048x1080",
    "2048x1536",
    "2560x1600",
    "2560x2048",
    "2592x1944",
];

/// Pixel format selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PixelFormat {
    Gray8,
    Rgb32,
}

impl PixelFormat {
    fn as_str(self) -> &'static str {
        match self {
            PixelFormat::Gray8 => "8bit",
            PixelFormat::Rgb32 => "32bitRGB",
        }
    }

    fn bytes_per_pixel(self) -> u32 {
        match self {
            PixelFormat::Gray8 => 1,
            PixelFormat::Rgb32 => 4,
        }
    }

    fn channels(self) -> u32 {
        match self {
            PixelFormat::Gray8 => 1,
            PixelFormat::Rgb32 => 4,
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
    readout_ms: f64,
    offset: i64,
    bit_depth: u32,
    binning: i32,
    roi: ImageRoi,
    pixel_format: PixelFormat,
    flip_x: bool,
    flip_y: bool,
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
        props
            .define_property("CameraIndex", PropertyValue::Integer(index as i64), true)
            .unwrap();
        props
            .define_property(
                "CameraName",
                PropertyValue::String("OpenCVgrabber video input".into()),
                true,
            )
            .unwrap();
        props
            .define_pre_init_property("Camera Number", PropertyValue::Integer(index as i64))
            .unwrap();
        props
            .set_allowed_values("Camera Number", &["0", "1", "2", "3"])
            .unwrap();
        props
            .define_property("FrameWidth", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("FrameHeight", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("FPS", PropertyValue::Float(30.0), false)
            .unwrap();
        props
            .define_property("Resolution", PropertyValue::String("320x200".into()), false)
            .unwrap();
        props.set_allowed_values("Resolution", RESOLUTIONS).unwrap();
        props
            .define_property("BitDepth", PropertyValue::Integer(8), false)
            .unwrap();
        props.set_allowed_values("BitDepth", &["8"]).unwrap();
        props
            .define_property("Binning", PropertyValue::Integer(1), false)
            .unwrap();
        props.set_allowed_values("Binning", &["1"]).unwrap();
        props
            .define_property("PixelType", PropertyValue::String("32bitRGB".into()), false)
            .unwrap();
        props
            .set_allowed_values("PixelType", &["8bit", "32bitRGB"])
            .unwrap();
        props
            .define_property(
                "PixelFormat",
                PropertyValue::String("32bitRGB".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("PixelFormat", &["8bit", "32bitRGB", "GRAY8", "BGR8"])
            .unwrap();
        props
            .define_property("Exposure", PropertyValue::Float(10.0), false)
            .unwrap();
        props.set_property_limits("Exposure", 0.0, 10000.0).unwrap();
        props
            .define_property("Offset", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("ReadoutTime", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("OnCameraCCDXSize", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("OnCameraCCDYSize", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("Flip X", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_property_limits("Flip X", 0.0, 1.0).unwrap();
        props
            .define_property("Flip Y", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_property_limits("Flip Y", 0.0, 1.0).unwrap();

        Self {
            props,
            device_index: index,
            cap: None,
            image_buf: Vec::new(),
            width: 0,
            height: 0,
            exposure_ms: 10.0,
            readout_ms: 0.0,
            offset: 0,
            bit_depth: 8,
            binning: 1,
            roi: ImageRoi::new(0, 0, 0, 0),
            pixel_format: PixelFormat::Rgb32,
            flip_x: false,
            flip_y: false,
            capturing: false,
        }
    }

    /// Read one frame from the capture device and write into `self.image_buf`,
    /// cropping to `self.roi` and converting to the selected pixel format.
    fn grab_into_buf(&mut self) -> MmResult<()> {
        let mut raw = Mat::default();
        {
            let cap = self.cap.as_mut().ok_or(MmError::NotConnected)?;
            let ok = cap
                .read(&mut raw)
                .map_err(|e| MmError::LocallyDefined(e.to_string()))?;
            if !ok || raw.empty() {
                return Err(MmError::LocallyDefined(
                    "OpenCV read() returned empty frame".into(),
                ));
            }
        }

        // Crop to ROI if not full-frame
        let roi_rect = opencv::core::Rect::new(
            self.roi.x as i32,
            self.roi.y as i32,
            self.roi.width as i32,
            self.roi.height as i32,
        );
        let cropped =
            Mat::roi(&raw, roi_rect).map_err(|e| MmError::LocallyDefined(e.to_string()))?;

        // Convert to target pixel format
        let converted = match self.pixel_format {
            PixelFormat::Gray8 => {
                let mut gray = Mat::default();
                imgproc::cvt_color(&cropped, &mut gray, imgproc::COLOR_BGR2GRAY, 0)
                    .map_err(|e| MmError::LocallyDefined(e.to_string()))?;
                gray
            }
            PixelFormat::Rgb32 => {
                let mut rgba = Mat::default();
                imgproc::cvt_color(&cropped, &mut rgba, imgproc::COLOR_BGR2BGRA, 0)
                    .map_err(|e| MmError::LocallyDefined(e.to_string()))?;
                rgba
            }
        };

        let final_mat = if self.flip_x || self.flip_y {
            let mut flipped = Mat::default();
            let flip_code = match (self.flip_x, self.flip_y) {
                (true, true) => -1,
                (true, false) => 1,
                (false, true) => 0,
                (false, false) => unreachable!(),
            };
            core::flip(&converted, &mut flipped, flip_code)
                .map_err(|e| MmError::LocallyDefined(e.to_string()))?;
            flipped
        } else {
            converted
        };

        // Copy pixel data into our buffer
        let data: &[u8] = final_mat
            .data_bytes()
            .map_err(|e| MmError::LocallyDefined(e.to_string()))?;
        self.image_buf.resize(data.len(), 0);
        self.image_buf.copy_from_slice(data);

        Ok(())
    }

    fn parse_resolution(resolution: &str) -> MmResult<(u32, u32)> {
        if !RESOLUTIONS.contains(&resolution) {
            return Err(MmError::InvalidPropertyValue);
        }
        let (w, h) = resolution
            .split_once('x')
            .ok_or(MmError::InvalidPropertyValue)?;
        let w = w
            .parse::<u32>()
            .map_err(|_| MmError::InvalidPropertyValue)?;
        let h = h
            .parse::<u32>()
            .map_err(|_| MmError::InvalidPropertyValue)?;
        Ok((w, h))
    }

    fn set_cached_size(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.roi = ImageRoi::new(0, 0, width, height);
        self.props
            .entry_mut("FrameWidth")
            .map(|e| e.value = PropertyValue::Integer(width as i64));
        self.props
            .entry_mut("FrameHeight")
            .map(|e| e.value = PropertyValue::Integer(height as i64));
        self.props
            .entry_mut("OnCameraCCDXSize")
            .map(|e| e.value = PropertyValue::Integer(width as i64));
        self.props
            .entry_mut("OnCameraCCDYSize")
            .map(|e| e.value = PropertyValue::Integer(height as i64));
        self.props
            .entry_mut("Resolution")
            .map(|e| e.value = PropertyValue::String(format!("{}x{}", width, height)));
    }

    fn update_size_from_cap(&mut self) -> MmResult<()> {
        let cap = self.cap.as_ref().ok_or(MmError::NotConnected)?;
        let w = cap
            .get(videoio::CAP_PROP_FRAME_WIDTH)
            .map_err(|e| MmError::LocallyDefined(e.to_string()))? as u32;
        let h = cap
            .get(videoio::CAP_PROP_FRAME_HEIGHT)
            .map_err(|e| MmError::LocallyDefined(e.to_string()))? as u32;
        self.set_cached_size(w, h);
        Ok(())
    }
}

impl Default for OpenCvCamera {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Device for OpenCvCamera {
    fn name(&self) -> &str {
        "OpenCVgrabber"
    }
    fn description(&self) -> &str {
        "OpenCVgrabber Device Adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        let mut cap = VideoCapture::new(self.device_index, videoio::CAP_ANY)
            .map_err(|e| MmError::LocallyDefined(format!("OpenCV VideoCapture::new: {}", e)))?;

        if !cap
            .is_opened()
            .map_err(|e| MmError::LocallyDefined(e.to_string()))?
        {
            return Err(MmError::LocallyDefined(format!(
                "OpenCV: failed to open device index {}",
                self.device_index
            )));
        }

        // Apply requested FPS
        let fps = self
            .props
            .get("FPS")
            .ok()
            .and_then(|v| v.as_f64())
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
            "Exposure" => Ok(PropertyValue::Float(self.exposure_ms)),
            "ReadoutTime" => Ok(PropertyValue::Float(self.readout_ms)),
            "Offset" => Ok(PropertyValue::Integer(self.offset)),
            "BitDepth" => Ok(PropertyValue::Integer(self.bit_depth as i64)),
            "Flip X" => Ok(PropertyValue::Integer(self.flip_x as i64)),
            "Flip Y" => Ok(PropertyValue::Integer(self.flip_y as i64)),
            "Camera Number" | "CameraIndex" => Ok(PropertyValue::Integer(self.device_index as i64)),
            "Resolution" => Ok(PropertyValue::String(format!(
                "{}x{}",
                self.width, self.height
            ))),
            "PixelType" | "PixelFormat" => {
                Ok(PropertyValue::String(self.pixel_format.as_str().into()))
            }
            "Binning" => Ok(PropertyValue::Integer(self.binning as i64)),
            _ => self.props.get(name).cloned(),
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
            "ReadoutTime" => {
                let ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.readout_ms = ms;
                self.props.set(name, PropertyValue::Float(ms))
            }
            "Offset" => {
                let offset = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.offset = offset;
                self.props.set(name, PropertyValue::Integer(offset))
            }
            "BitDepth" => {
                if self.capturing {
                    return Err(MmError::CameraBusyAcquiring);
                }
                let depth = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(depth))?;
                self.bit_depth = depth as u32;
                Ok(())
            }
            "Flip X" => {
                let flip = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(flip))?;
                self.flip_x = flip != 0;
                Ok(())
            }
            "Flip Y" => {
                let flip = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(flip))?;
                self.flip_y = flip != 0;
                Ok(())
            }
            "FPS" => {
                let fps = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if let Some(cap) = self.cap.as_mut() {
                    let _ = cap.set(videoio::CAP_PROP_FPS, fps);
                }
                self.props.set(name, PropertyValue::Float(fps))
            }
            "Camera Number" => {
                let index = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.device_index = index;
                if let Some(entry) = self.props.entry_mut("CameraIndex") {
                    entry.value = PropertyValue::Integer(index as i64);
                }
                self.props
                    .set("Camera Number", PropertyValue::Integer(index as i64))
            }
            "Resolution" => {
                if self.capturing {
                    return Err(MmError::CameraBusyAcquiring);
                }
                let resolution = val.as_str().to_string();
                let (requested_w, requested_h) = Self::parse_resolution(&resolution)?;
                if let Some(cap) = self.cap.as_mut() {
                    let _ = cap.set(videoio::CAP_PROP_FRAME_WIDTH, requested_w as f64);
                    let _ = cap.set(videoio::CAP_PROP_FRAME_HEIGHT, requested_h as f64);
                    let actual_w = cap
                        .get(videoio::CAP_PROP_FRAME_WIDTH)
                        .map_err(|e| MmError::LocallyDefined(e.to_string()))?
                        as u32;
                    let actual_h = cap
                        .get(videoio::CAP_PROP_FRAME_HEIGHT)
                        .map_err(|e| MmError::LocallyDefined(e.to_string()))?
                        as u32;
                    if actual_w == 0 || actual_h == 0 {
                        return Err(MmError::LocallyDefined(
                            "OpenCV returned invalid frame size".into(),
                        ));
                    }
                    self.set_cached_size(actual_w, actual_h);
                    Ok(())
                } else {
                    self.set_cached_size(requested_w, requested_h);
                    self.props
                        .set("Resolution", PropertyValue::String(resolution))
                }
            }
            "PixelType" | "PixelFormat" => {
                if self.capturing {
                    return Err(MmError::CameraBusyAcquiring);
                }
                let s = val.as_str().to_string();
                self.pixel_format = match s.as_str() {
                    "8bit" | "GRAY8" => PixelFormat::Gray8,
                    "32bitRGB" | "BGR8" => PixelFormat::Rgb32,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                let upstream = self.pixel_format.as_str().to_string();
                self.props
                    .set("PixelType", PropertyValue::String(upstream.clone()))?;
                self.props
                    .set("PixelFormat", PropertyValue::String(upstream.clone()))?;
                Ok(())
            }
            "Binning" => {
                let bin = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.set_binning(bin)
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

impl Camera for OpenCvCamera {
    fn snap_image(&mut self) -> MmResult<()> {
        if self.cap.is_none() {
            return Err(MmError::NotConnected);
        }
        self.grab_into_buf()
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if self.image_buf.is_empty() {
            return Err(MmError::LocallyDefined(
                "No image captured yet; call snap_image first".into(),
            ));
        }
        Ok(&self.image_buf)
    }

    fn get_image_width(&self) -> u32 {
        self.roi.width
    }
    fn get_image_height(&self) -> u32 {
        self.roi.height
    }
    fn get_image_bytes_per_pixel(&self) -> u32 {
        self.pixel_format.bytes_per_pixel()
    }
    fn get_bit_depth(&self) -> u32 {
        self.bit_depth
    }
    fn get_number_of_components(&self) -> u32 {
        self.pixel_format.channels()
    }
    fn get_number_of_channels(&self) -> u32 {
        1
    } // single optical channel

    fn get_exposure(&self) -> f64 {
        self.exposure_ms
    }
    fn set_exposure(&mut self, exp_ms: f64) -> MmResult<()> {
        self.exposure_ms = exp_ms;
        if let Some(cap) = self.cap.as_mut() {
            let _ = cap.set(videoio::CAP_PROP_EXPOSURE, exp_ms);
        }
        Ok(())
    }

    fn get_binning(&self) -> i32 {
        self.binning
    }
    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        if bin != 1 {
            // VideoCapture does not support hardware binning in general;
            // only 1×1 is supported unless the specific backend does.
            return Err(MmError::LocallyDefined(
                "OpenCV VideoCapture does not support hardware binning; use binning=1".into(),
            ));
        }
        self.binning = bin;
        self.props
            .set("Binning", PropertyValue::Integer(bin as i64))?;
        Ok(())
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        Ok(self.roi)
    }
    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        if roi.width == 0 && roi.height == 0 {
            return self.clear_roi();
        }
        // Validate against sensor size
        if roi.x + roi.width > self.width || roi.y + roi.height > self.height {
            return Err(MmError::LocallyDefined(format!(
                "ROI ({},{} {}x{}) exceeds sensor size {}x{}",
                roi.x, roi.y, roi.width, roi.height, self.width, self.height
            )));
        }
        self.roi = roi;
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        self.roi = ImageRoi::new(0, 0, self.width, self.height);
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, _count: i64, _interval_ms: f64) -> MmResult<()> {
        if self.cap.is_none() {
            return Err(MmError::NotConnected);
        }
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        self.capturing = true;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        self.capturing = false;
        Ok(())
    }

    fn is_capturing(&self) -> bool {
        self.capturing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_sized_roi_clears_to_full_frame() {
        let mut cam = OpenCvCamera::new(0);
        cam.width = 640;
        cam.height = 480;
        cam.roi = ImageRoi::new(10, 20, 100, 80);

        cam.set_roi(ImageRoi::new(0, 0, 0, 0)).unwrap();

        assert_eq!(cam.get_roi().unwrap(), ImageRoi::new(0, 0, 640, 480));
    }

    #[test]
    fn upstream_pixel_type_defaults_and_metadata() {
        let mut cam = OpenCvCamera::new(0);

        assert_eq!(
            cam.get_property("PixelType").unwrap(),
            PropertyValue::String("32bitRGB".into())
        );
        assert_eq!(cam.get_image_bytes_per_pixel(), 4);
        assert_eq!(cam.get_number_of_components(), 4);

        cam.set_property("PixelType", PropertyValue::String("8bit".into()))
            .unwrap();

        assert_eq!(cam.get_image_bytes_per_pixel(), 1);
        assert_eq!(cam.get_number_of_components(), 1);
    }

    #[test]
    fn upstream_binning_property_only_accepts_one() {
        let mut cam = OpenCvCamera::new(0);

        cam.set_property("Binning", PropertyValue::Integer(1))
            .unwrap();
        assert_eq!(cam.get_binning(), 1);
        assert!(cam
            .set_property("Binning", PropertyValue::Integer(2))
            .is_err());
        assert_eq!(cam.get_binning(), 1);
    }

    #[test]
    fn linux_camera_number_alias_updates_device_index() {
        let mut cam = OpenCvCamera::new(0);

        assert_eq!(
            cam.get_property("CameraName").unwrap(),
            PropertyValue::String("OpenCVgrabber video input".into())
        );
        assert!(cam.is_property_read_only("CameraName"));
        assert_eq!(
            cam.get_property("Camera Number").unwrap(),
            PropertyValue::Integer(0)
        );

        cam.set_property("Camera Number", PropertyValue::Integer(2))
            .unwrap();

        assert_eq!(
            cam.get_property("Camera Number").unwrap(),
            PropertyValue::Integer(2)
        );
        assert_eq!(
            cam.get_property("CameraIndex").unwrap(),
            PropertyValue::Integer(2)
        );
    }

    #[test]
    fn pixel_type_rejected_while_sequence_active() {
        let mut cam = OpenCvCamera::new(0);
        cam.capturing = true;

        assert_eq!(
            cam.set_property("PixelType", PropertyValue::String("8bit".into())),
            Err(MmError::CameraBusyAcquiring)
        );
        assert_eq!(
            cam.get_property("PixelType").unwrap(),
            PropertyValue::String("32bitRGB".into())
        );
    }

    #[test]
    fn upstream_noop_properties_are_cached() {
        let mut cam = OpenCvCamera::new(0);

        assert_eq!(
            cam.get_property("BitDepth").unwrap(),
            PropertyValue::Integer(8)
        );
        assert_eq!(
            cam.get_property("ReadoutTime").unwrap(),
            PropertyValue::Float(0.0)
        );
        assert_eq!(
            cam.get_property("Offset").unwrap(),
            PropertyValue::Integer(0)
        );
        assert_eq!(
            cam.get_property("OnCameraCCDXSize").unwrap(),
            PropertyValue::Integer(0)
        );
        assert_eq!(
            cam.get_property("OnCameraCCDYSize").unwrap(),
            PropertyValue::Integer(0)
        );
        assert!(cam.is_property_read_only("OnCameraCCDXSize"));
        assert!(cam.is_property_read_only("OnCameraCCDYSize"));

        cam.set_property("ReadoutTime", PropertyValue::Float(2.5))
            .unwrap();
        cam.set_property("Offset", PropertyValue::Integer(3))
            .unwrap();
        cam.set_property("Flip X", PropertyValue::Integer(1))
            .unwrap();
        cam.set_property("Flip Y", PropertyValue::Integer(1))
            .unwrap();

        assert_eq!(
            cam.get_property("ReadoutTime").unwrap(),
            PropertyValue::Float(2.5)
        );
        assert_eq!(
            cam.get_property("Offset").unwrap(),
            PropertyValue::Integer(3)
        );
        assert_eq!(
            cam.get_property("Flip X").unwrap(),
            PropertyValue::Integer(1)
        );
        assert_eq!(
            cam.get_property("Flip Y").unwrap(),
            PropertyValue::Integer(1)
        );
        assert!(cam.flip_x);
        assert!(cam.flip_y);
    }

    #[test]
    fn bit_depth_only_accepts_upstream_allowed_value() {
        let mut cam = OpenCvCamera::new(0);

        cam.set_property("BitDepth", PropertyValue::Integer(8))
            .unwrap();
        assert_eq!(cam.get_bit_depth(), 8);
        assert_eq!(
            cam.set_property("BitDepth", PropertyValue::Integer(16)),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(cam.get_bit_depth(), 8);

        cam.capturing = true;
        assert_eq!(
            cam.set_property("BitDepth", PropertyValue::Integer(8)),
            Err(MmError::CameraBusyAcquiring)
        );
    }

    #[test]
    fn upstream_resolution_property_updates_cached_size_before_init() {
        let mut cam = OpenCvCamera::new(0);

        assert_eq!(
            cam.get_property("Resolution").unwrap(),
            PropertyValue::String("0x0".into())
        );
        cam.set_property("Resolution", PropertyValue::String("640x480".into()))
            .unwrap();
        assert_eq!(
            cam.get_property("Resolution").unwrap(),
            PropertyValue::String("640x480".into())
        );
        assert_eq!(
            cam.set_property("Resolution", PropertyValue::String("123x456".into())),
            Err(MmError::InvalidPropertyValue)
        );

        cam.capturing = true;
        assert_eq!(
            cam.set_property("Resolution", PropertyValue::String("320x240".into())),
            Err(MmError::CameraBusyAcquiring)
        );
    }
}
