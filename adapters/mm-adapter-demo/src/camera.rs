use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Camera, Device};
use micromanager::types::{DeviceType, ImageRoi, PropertyValue};

/// Demo camera — simulates a 512×512 grayscale camera.
pub struct DemoCamera {
    props: PropertyMap,
    initialized: bool,
    image_buf: Vec<u8>,
    width: u32,
    height: u32,
    bytes_per_pixel: u32,
    exposure_ms: f64,
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
        props.define_property("Exposure", PropertyValue::Float(10.0), false).unwrap();
        props.define_property("Binning", PropertyValue::Integer(1), false).unwrap();
        props.set_allowed_values("Binning", &["1", "2", "4", "8"]).unwrap();
        props.define_property("PixelType", PropertyValue::String("GRAY8".into()), false).unwrap();
        props.set_allowed_values("PixelType", &["GRAY8", "GRAY16"]).unwrap();
        props.define_property("ReadoutTime", PropertyValue::Float(0.0), true).unwrap();
        props.define_property("CameraName", PropertyValue::String("DemoCamera".into()), true).unwrap();

        Self {
            props,
            initialized: false,
            image_buf: vec![0u8; (width * height * bpp) as usize],
            width,
            height,
            bytes_per_pixel: bpp,
            exposure_ms: 10.0,
            binning: 1,
            roi: ImageRoi::new(0, 0, width, height),
            capturing: false,
        }
    }

    /// Generate a synthetic test pattern (sine wave gradient).
    fn generate_image(&mut self) {
        let w = self.roi.width as usize;
        let h = self.roi.height as usize;
        let buf = &mut self.image_buf;
        buf.resize(w * h * self.bytes_per_pixel as usize, 0);

        for y in 0..h {
            for x in 0..w {
                let val = ((x + y) % 256) as u8;
                let idx = y * w + x;
                buf[idx] = val;
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
        "DCamera"
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
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Exposure" => {
                self.exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, val)
            }
            "Binning" => {
                let b = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.binning = b;
                self.width = 512 / b as u32;
                self.height = 512 / b as u32;
                self.roi = ImageRoi::new(0, 0, self.width, self.height);
                self.props.set(name, PropertyValue::Integer(b as i64))
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
        self.generate_image();
        Ok(())
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
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
        8
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
    }

    fn get_binning(&self) -> i32 {
        self.binning
    }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        if ![1, 2, 4, 8].contains(&bin) {
            return Err(MmError::InvalidPropertyValue);
        }
        self.binning = bin;
        self.width = 512 / bin as u32;
        self.height = 512 / bin as u32;
        self.roi = ImageRoi::new(0, 0, self.width, self.height);
        Ok(())
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        Ok(self.roi)
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        self.roi = roi;
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        self.roi = ImageRoi::new(0, 0, 512 / self.binning as u32, 512 / self.binning as u32);
        Ok(())
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
        cam.set_property("Exposure", PropertyValue::Float(50.0)).unwrap();
        assert_eq!(cam.get_exposure(), 50.0);
    }
}
