use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Camera, Device};
use crate::types::{DeviceType, ImageRoi, PropertyValue};
use std::cell::Cell;
use std::time::{Duration, Instant};

/// Demo camera — simulates a 512×512 grayscale camera.
pub struct DemoCamera {
    props: PropertyMap,
    initialized: bool,
    image_buf: Vec<u8>,
    width: u32,
    height: u32,
    bytes_per_pixel: u32,
    bit_depth: u32,
    components: u32,
    exposure_ms: f64,
    readout_ms: f64,
    readout_start: Cell<Option<Instant>>,
    binning: i32,
    roi: ImageRoi,
    capturing: bool,
}

impl DemoCamera {
    pub fn new() -> Self {
        let width = 512u32;
        let height = 512u32;
        let bpp = 1u32;
        let mut props = PropertyMap::new();
        props
            .define_property("Exposure", PropertyValue::Float(10.0), false)
            .unwrap();
        props
            .define_property("Binning", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .set_allowed_values("Binning", &["1", "2", "4", "8"])
            .unwrap();
        props
            .define_property("PixelType", PropertyValue::String("8bit".into()), false)
            .unwrap();
        props
            .set_allowed_values(
                "PixelType",
                &["8bit", "16bit", "32bitRGB", "64bitRGB", "32bit"],
            )
            .unwrap();
        props
            .define_property("BitDepth", PropertyValue::Integer(8), false)
            .unwrap();
        props
            .set_allowed_values("BitDepth", &["8", "10", "11", "12", "14", "16", "32"])
            .unwrap();
        props
            .define_property("ReadoutTime", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property(
                "CameraName",
                PropertyValue::String("DemoCamera-MultiMode".into()),
                true,
            )
            .unwrap();

        Self {
            props,
            initialized: false,
            image_buf: vec![0u8; (width * height * bpp) as usize],
            width,
            height,
            bytes_per_pixel: bpp,
            bit_depth: 8,
            components: 1,
            exposure_ms: 10.0,
            readout_ms: 0.0,
            readout_start: Cell::new(None),
            binning: 1,
            roi: ImageRoi::new(0, 0, width, height),
            capturing: false,
        }
    }

    fn set_pixel_type(&mut self, pixel_type: &str) -> MmResult<()> {
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        let (bytes_per_pixel, bit_depth, components) = match pixel_type {
            "8bit" => (1, 8, 1),
            "16bit" => (2, 16, 1),
            "32bitRGB" => (4, 8, 4),
            "64bitRGB" => (8, 16, 4),
            "32bit" => (4, 32, 1),
            _ => {
                self.bytes_per_pixel = 1;
                self.bit_depth = 8;
                self.components = 1;
                self.props
                    .set("PixelType", PropertyValue::String("8bit".to_string()))?;
                self.props.set("BitDepth", PropertyValue::Integer(8))?;
                self.image_buf
                    .resize((self.roi.width * self.roi.height) as usize, 0);
                return Err(MmError::InvalidPropertyValue);
            }
        };

        self.bytes_per_pixel = bytes_per_pixel;
        self.bit_depth = bit_depth;
        self.components = components;
        self.props
            .set("PixelType", PropertyValue::String(pixel_type.to_string()))?;
        self.props
            .set("BitDepth", PropertyValue::Integer(bit_depth as i64))?;
        self.image_buf.resize(
            (self.roi.width * self.roi.height * self.bytes_per_pixel) as usize,
            0,
        );
        Ok(())
    }

    fn set_bit_depth(&mut self, bit_depth: u32) -> MmResult<()> {
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        let bytes_per_component = match bit_depth {
            8 => 1,
            10 | 11 | 12 | 14 | 16 => 2,
            32 => 4,
            _ => {
                self.bit_depth = 8;
                self.props.set("BitDepth", PropertyValue::Integer(8))?;
                return Err(MmError::InvalidPropertyValue);
            }
        };

        self.bit_depth = bit_depth;
        self.props
            .set("BitDepth", PropertyValue::Integer(bit_depth as i64))?;

        if self
            .props
            .get("PixelType")
            .map(|v| v.to_string())
            .as_deref()
            == Ok("8bit")
        {
            match bytes_per_component {
                2 => self.set_pixel_type("16bit")?,
                4 => self.set_pixel_type("32bit")?,
                _ => {}
            }
            self.bit_depth = bit_depth;
            self.props
                .set("BitDepth", PropertyValue::Integer(bit_depth as i64))?;
        } else {
            self.image_buf.resize(
                (self.roi.width * self.roi.height * self.bytes_per_pixel) as usize,
                0,
            );
        }
        Ok(())
    }

    fn set_binning_checked(&mut self, bin: i32) -> MmResult<()> {
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        if ![1, 2, 4, 8].contains(&bin) {
            return Err(MmError::InvalidPropertyValue);
        }
        let old_binning = self.binning;
        let old_roi = self.roi;
        self.props
            .set("Binning", PropertyValue::Integer(bin as i64))?;
        self.binning = bin;
        self.width = 512 / bin as u32;
        self.height = 512 / bin as u32;
        if old_roi.x == 0
            && old_roi.y == 0
            && old_roi.width == 512 / old_binning as u32
            && old_roi.height == 512 / old_binning as u32
        {
            self.roi = ImageRoi::new(0, 0, self.width, self.height);
        } else {
            let factor = bin as f64 / old_binning as f64;
            self.roi = ImageRoi::new(
                (old_roi.x as f64 / factor) as u32,
                (old_roi.y as f64 / factor) as u32,
                (old_roi.width as f64 / factor) as u32,
                (old_roi.height as f64 / factor) as u32,
            );
        }
        self.image_buf.resize(
            (self.roi.width * self.roi.height * self.bytes_per_pixel) as usize,
            0,
        );
        Ok(())
    }

    fn set_roi_checked(&mut self, roi: ImageRoi) -> MmResult<()> {
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        if roi.width == 0 && roi.height == 0 {
            self.roi = ImageRoi::new(0, 0, self.width, self.height);
        } else {
            if roi.width == 0
                || roi.height == 0
                || roi.x >= self.width
                || roi.y >= self.height
                || roi.width > self.width - roi.x
                || roi.height > self.height - roi.y
            {
                return Err(MmError::InvalidInputParam);
            }
            self.roi = roi;
        }
        self.image_buf.resize(
            (self.roi.width * self.roi.height * self.bytes_per_pixel) as usize,
            0,
        );
        Ok(())
    }

    /// Generate a synthetic test pattern (sine wave gradient).
    fn generate_image(&mut self) {
        let w = self.roi.width as usize;
        let h = self.roi.height as usize;
        let bytes_per_pixel = self.bytes_per_pixel as usize;
        let buf = &mut self.image_buf;
        buf.resize(w * h * bytes_per_pixel, 0);

        for y in 0..h {
            for x in 0..w {
                let val = ((x + y) % 256) as u8;
                let idx = (y * w + x) * bytes_per_pixel;
                for byte in &mut buf[idx..idx + bytes_per_pixel] {
                    *byte = val;
                }
            }
        }
    }
}

impl Default for DemoCamera {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for DemoCamera {
    fn name(&self) -> &str {
        "DCam"
    }

    fn description(&self) -> &str {
        "Demo camera — simulates a digital camera"
    }

    fn initialize(&mut self) -> MmResult<()> {
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        self.capturing = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Exposure" => Ok(PropertyValue::Float(self.exposure_ms)),
            "Binning" => Ok(PropertyValue::Integer(self.binning as i64)),
            "PixelType" => self.props.get(name).cloned(),
            "BitDepth" => Ok(PropertyValue::Integer(self.bit_depth as i64)),
            "ReadoutTime" => Ok(PropertyValue::Float(self.readout_ms)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Exposure" => {
                let exposure = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, val)?;
                self.exposure_ms = exposure;
                Ok(())
            }
            "Binning" => {
                let b = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.set_binning_checked(b)
            }
            "PixelType" => self.set_pixel_type(val.as_str()),
            "BitDepth" => {
                let bit_depth = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_bit_depth(bit_depth as u32)
            }
            "ReadoutTime" => {
                let readout = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, val)?;
                self.readout_ms = readout;
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

impl Camera for DemoCamera {
    fn snap_image(&mut self) -> MmResult<()> {
        if !self.initialized {
            return Err(MmError::NotConnected);
        }
        if self.exposure_ms > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(self.exposure_ms / 1000.0));
        }
        self.generate_image();
        self.readout_start.set(Some(Instant::now()));
        Ok(())
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if let Some(start) = self.readout_start.get() {
            let readout = Duration::from_secs_f64((self.readout_ms.max(0.0)) / 1000.0);
            let elapsed = start.elapsed();
            if readout > elapsed {
                std::thread::sleep(readout - elapsed);
            }
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
        self.exposure_ms
    }

    fn set_exposure(&mut self, exp_ms: f64) {
        self.exposure_ms = exp_ms;
        let _ = self.props.set("Exposure", PropertyValue::Float(exp_ms));
    }

    fn get_binning(&self) -> i32 {
        self.binning
    }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        self.set_binning_checked(bin)
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        Ok(self.roi)
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        self.set_roi_checked(roi)
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        self.set_roi_checked(ImageRoi::new(0, 0, self.width, self.height))
    }

    fn start_sequence_acquisition(&mut self, _count: i64, _interval_ms: f64) -> MmResult<()> {
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
    fn snap_and_check_size() {
        let mut cam = DemoCamera::new();
        cam.initialize().unwrap();
        cam.snap_image().unwrap();
        let buf = cam.get_image_buffer().unwrap();
        assert_eq!(buf.len(), (512 * 512) as usize);
    }

    #[test]
    fn binning_changes_size() {
        let mut cam = DemoCamera::new();
        cam.initialize().unwrap();
        cam.set_binning(2).unwrap();
        cam.snap_image().unwrap();
        let buf = cam.get_image_buffer().unwrap();
        assert_eq!(buf.len(), (256 * 256) as usize);
        assert_eq!(cam.get_image_width(), 256);
        assert_eq!(cam.get_image_height(), 256);
    }

    #[test]
    fn exposure_property() {
        let mut cam = DemoCamera::new();
        cam.set_property("Exposure", PropertyValue::Float(50.0))
            .unwrap();
        assert_eq!(cam.get_exposure(), 50.0);
    }

    #[test]
    fn upstream_pixel_type_names_update_image_shape() {
        let mut cam = DemoCamera::new();
        cam.initialize().unwrap();

        assert_eq!(
            cam.get_property("PixelType").unwrap(),
            PropertyValue::String("8bit".into())
        );
        assert_eq!(
            cam.get_property("CameraName").unwrap(),
            PropertyValue::String("DemoCamera-MultiMode".into())
        );
        assert!(!cam.is_property_read_only("ReadoutTime"));

        cam.set_property("PixelType", PropertyValue::String("16bit".into()))
            .unwrap();
        cam.snap_image().unwrap();
        assert_eq!(cam.get_image_bytes_per_pixel(), 2);
        assert_eq!(cam.get_bit_depth(), 16);
        assert_eq!(cam.get_number_of_components(), 1);
        assert_eq!(cam.get_image_buffer().unwrap().len(), 512 * 512 * 2);

        cam.set_property("PixelType", PropertyValue::String("32bitRGB".into()))
            .unwrap();
        assert_eq!(cam.get_image_bytes_per_pixel(), 4);
        assert_eq!(cam.get_bit_depth(), 8);
        assert_eq!(cam.get_number_of_components(), 4);
        assert!(cam
            .set_property("PixelType", PropertyValue::String("GRAY8".into()))
            .is_err());
        assert_eq!(
            cam.get_property("PixelType").unwrap(),
            PropertyValue::String("8bit".into())
        );
        assert_eq!(cam.get_image_bytes_per_pixel(), 1);
        assert_eq!(cam.get_bit_depth(), 8);
    }

    #[test]
    fn upstream_bit_depth_promotes_default_pixel_type() {
        let mut cam = DemoCamera::new();
        cam.set_property("BitDepth", PropertyValue::Integer(12))
            .unwrap();
        assert_eq!(cam.get_bit_depth(), 12);
        assert_eq!(cam.get_image_bytes_per_pixel(), 2);
        assert_eq!(
            cam.get_property("PixelType").unwrap(),
            PropertyValue::String("16bit".into())
        );

        cam.set_property("PixelType", PropertyValue::String("32bitRGB".into()))
            .unwrap();
        cam.set_property("BitDepth", PropertyValue::Integer(16))
            .unwrap();
        assert_eq!(cam.get_bit_depth(), 16);
        assert_eq!(cam.get_image_bytes_per_pixel(), 4);
        assert_eq!(
            cam.get_property("PixelType").unwrap(),
            PropertyValue::String("32bitRGB".into())
        );

        assert!(cam
            .set_property("BitDepth", PropertyValue::Integer(9))
            .is_err());
        assert_eq!(cam.get_bit_depth(), 8);
    }

    #[test]
    fn invalid_binning_does_not_change_geometry() {
        let mut cam = DemoCamera::new();
        cam.set_binning(2).unwrap();
        assert_eq!(cam.get_image_width(), 256);
        assert!(cam
            .set_property("Binning", PropertyValue::Integer(3))
            .is_err());
        assert_eq!(cam.get_binning(), 2);
        assert_eq!(cam.get_image_width(), 256);
        assert_eq!(
            cam.get_property("Binning").unwrap(),
            PropertyValue::Integer(2)
        );
    }

    #[test]
    fn acquisition_rejects_geometry_and_pixel_format_changes() {
        let mut cam = DemoCamera::new();
        cam.initialize().unwrap();
        cam.start_sequence_acquisition(10, 100.0).unwrap();

        assert_eq!(
            cam.set_binning(2).unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(
            cam.set_property("PixelType", PropertyValue::String("16bit".into()))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(
            cam.set_property("BitDepth", PropertyValue::Integer(16))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );

        cam.stop_sequence_acquisition().unwrap();
        cam.set_binning(2).unwrap();
        assert_eq!(cam.get_image_width(), 256);
    }

    #[test]
    fn binning_scales_existing_roi_like_upstream() {
        let mut cam = DemoCamera::new();
        cam.set_roi(ImageRoi::new(20, 40, 100, 80)).unwrap();
        cam.set_binning(2).unwrap();

        assert_eq!(cam.get_roi().unwrap(), ImageRoi::new(10, 20, 50, 40));
        assert_eq!(cam.get_image_width(), 50);
        assert_eq!(cam.get_image_height(), 40);
    }

    #[test]
    fn readout_time_property_is_writable() {
        let mut cam = DemoCamera::new();
        cam.set_property("ReadoutTime", PropertyValue::Float(1.5))
            .unwrap();

        assert_eq!(
            cam.get_property("ReadoutTime").unwrap(),
            PropertyValue::Float(1.5)
        );
        assert!(!cam.is_property_read_only("ReadoutTime"));
    }

    #[test]
    fn roi_is_rejected_outside_current_binned_image() {
        let mut cam = DemoCamera::new();
        cam.set_binning(2).unwrap();
        cam.set_roi(ImageRoi::new(10, 12, 40, 30)).unwrap();

        assert_eq!(
            cam.set_roi(ImageRoi::new(240, 0, 32, 32)).unwrap_err(),
            MmError::InvalidInputParam
        );
        assert_eq!(cam.get_roi().unwrap(), ImageRoi::new(10, 12, 40, 30));

        assert_eq!(
            cam.set_roi(ImageRoi::new(0, 0, 0, 10)).unwrap_err(),
            MmError::InvalidInputParam
        );
        assert_eq!(cam.get_roi().unwrap(), ImageRoi::new(10, 12, 40, 30));

        cam.set_roi(ImageRoi::new(0, 0, 0, 0)).unwrap();
        assert_eq!(cam.get_roi().unwrap(), ImageRoi::new(0, 0, 256, 256));
    }

    #[test]
    fn acquisition_rejects_roi_changes() {
        let mut cam = DemoCamera::new();
        cam.initialize().unwrap();
        cam.start_sequence_acquisition(10, 100.0).unwrap();

        assert_eq!(
            cam.set_roi(ImageRoi::new(0, 0, 128, 128)).unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(cam.get_roi().unwrap(), ImageRoi::new(0, 0, 512, 512));
    }
}
