use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Camera, Device};
use crate::types::{DeviceType, ImageRoi, PropertyValue};
use std::cell::Cell;
use std::time::{Duration, Instant};

const MODE_ARTIFICIAL_WAVES: &str = "Artificial Waves";
const MODE_NOISE: &str = "Noise";
const MODE_COLOR_TEST: &str = "Color Test Pattern";
const MODE_BEADS: &str = "Fluorescent Beads";

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
    exposure_maximum_ms: f64,
    scan_mode: i64,
    gain: i64,
    offset: i64,
    ccd_temperature: f64,
    camera_ccd_x_size: u32,
    camera_ccd_y_size: u32,
    trigger_device: String,
    drop_pixels: bool,
    saturate_pixels: bool,
    fast_image: bool,
    fraction_drop_or_saturate: f64,
    rotate_images: bool,
    display_image_number: bool,
    stripe_width: f64,
    allow_multi_roi: bool,
    multi_roi_fill_value: i64,
    photon_conversion_factor: f64,
    read_noise_electrons: f64,
    photon_flux: f64,
    bead_density: i64,
    bead_size: f64,
    bead_brightness: f64,
    bead_blur_rate: f64,
    async_property_leader: String,
    async_property_follower: String,
    async_property_delay_ms: i64,
    use_exposure_sequences: bool,
    exposure_sequence: Vec<f64>,
    exposure_sequence_running: bool,
    exposure_sequence_index: usize,
    sequence_remaining: Option<i64>,
    sequence_interval_ms: f64,
    sequence_last_frame: Option<Instant>,
    mode: String,
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
        props.set_property_limits("Exposure", 0.0, 1000.0).unwrap();
        props
            .define_property("MaximumExposureMs", PropertyValue::Float(1000.0), false)
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
            .define_property("ScanMode", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .set_allowed_values("ScanMode", &["1", "2", "3"])
            .unwrap();
        props
            .define_property("Gain", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_property_limits("Gain", -5.0, 8.0).unwrap();
        props
            .define_property("Offset", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("CCDTemperature", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("CCDTemperature", -100.0, 10.0)
            .unwrap();
        props
            .define_property("CCDTemperature RO", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property(
                "OnCameraCCDXSize",
                PropertyValue::Integer(width as i64),
                false,
            )
            .unwrap();
        props
            .define_property(
                "OnCameraCCDYSize",
                PropertyValue::Integer(height as i64),
                false,
            )
            .unwrap();
        props
            .define_property("TriggerDevice", PropertyValue::String(String::new()), false)
            .unwrap();
        for name in [
            "DropPixels",
            "SaturatePixels",
            "FastImage",
            "RotateImages",
            "DisplayImageNumber",
            "AllowMultiROI",
        ] {
            props
                .define_property(name, PropertyValue::Integer(0), false)
                .unwrap();
            props.set_allowed_values(name, &["0", "1"]).unwrap();
        }
        props
            .define_property(
                "FractionOfPixelsToDropOrSaturate",
                PropertyValue::Float(0.002),
                false,
            )
            .unwrap();
        props
            .set_property_limits("FractionOfPixelsToDropOrSaturate", 0.0, 0.1)
            .unwrap();
        props
            .define_property("StripeWidth", PropertyValue::Float(0.0), false)
            .unwrap();
        props.set_property_limits("StripeWidth", 0.0, 10.0).unwrap();
        props
            .define_property("MultiROIFillValue", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .set_property_limits("MultiROIFillValue", 0.0, 65536.0)
            .unwrap();
        props
            .define_property(
                "UseExposureSequences",
                PropertyValue::String("No".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("UseExposureSequences", &["Yes", "No"])
            .unwrap();
        props
            .define_property(
                "Mode",
                PropertyValue::String(MODE_ARTIFICIAL_WAVES.into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values(
                "Mode",
                &[
                    MODE_ARTIFICIAL_WAVES,
                    MODE_NOISE,
                    MODE_COLOR_TEST,
                    MODE_BEADS,
                ],
            )
            .unwrap();
        props
            .define_property("Photon Conversion Factor", PropertyValue::Float(1.0), false)
            .unwrap();
        props
            .set_property_limits("Photon Conversion Factor", 0.01, 10.0)
            .unwrap();
        props
            .define_property("ReadNoise (electrons)", PropertyValue::Float(2.5), false)
            .unwrap();
        props
            .set_property_limits("ReadNoise (electrons)", 0.25, 50.0)
            .unwrap();
        props
            .define_property("Photon Flux", PropertyValue::Float(50.0), false)
            .unwrap();
        props
            .set_property_limits("Photon Flux", 2.0, 5000.0)
            .unwrap();
        props
            .define_property("BeadDensity", PropertyValue::Integer(100), false)
            .unwrap();
        props
            .set_property_limits("BeadDensity", 10.0, 500.0)
            .unwrap();
        props
            .define_property("BeadSize", PropertyValue::Float(2.0), false)
            .unwrap();
        props.set_property_limits("BeadSize", 1.0, 10.0).unwrap();
        props
            .define_property("BeadBrightness", PropertyValue::Float(1.0), false)
            .unwrap();
        props
            .set_property_limits("BeadBrightness", 0.125, 8.0)
            .unwrap();
        props
            .define_property("BeadBlurRate", PropertyValue::Float(0.5), false)
            .unwrap();
        props.set_property_limits("BeadBlurRate", 0.1, 1.0).unwrap();
        props
            .define_property(
                "AsyncPropertyLeader",
                PropertyValue::String("init".into()),
                false,
            )
            .unwrap();
        props
            .define_property(
                "AsyncPropertyFollower",
                PropertyValue::String("init".into()),
                true,
            )
            .unwrap();
        props
            .define_property("AsyncPropertyDelayMS", PropertyValue::Integer(2000), false)
            .unwrap();
        props
            .define_property(
                "CameraName",
                PropertyValue::String("DemoCamera-MultiMode".into()),
                true,
            )
            .unwrap();
        props
            .define_property("CameraID", PropertyValue::String("V1.0".into()), true)
            .unwrap();
        props
            .define_property("SimulateCrash", PropertyValue::String(String::new()), false)
            .unwrap();
        props
            .set_allowed_values(
                "SimulateCrash",
                &["", "Dereference Null Pointer", "Divide by Zero"],
            )
            .unwrap();
        for index in 1..=6 {
            let name = format!("TestProperty{index}");
            props
                .define_property(&name, PropertyValue::Float(0.0), false)
                .unwrap();
            if index % 5 != 0 {
                let upper = (index as f64)
                    * 10_f64.powi(if index % 2 == 1 {
                        -(index as i32)
                    } else {
                        index as i32
                    });
                let lower = if index % 3 == 0 { 0.0 } else { -upper };
                props.set_property_limits(&name, lower, upper).unwrap();
            }
        }

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
            exposure_maximum_ms: 1000.0,
            scan_mode: 1,
            gain: 0,
            offset: 0,
            ccd_temperature: 0.0,
            camera_ccd_x_size: width,
            camera_ccd_y_size: height,
            trigger_device: String::new(),
            drop_pixels: false,
            saturate_pixels: false,
            fast_image: false,
            fraction_drop_or_saturate: 0.002,
            rotate_images: false,
            display_image_number: false,
            stripe_width: 0.0,
            allow_multi_roi: false,
            multi_roi_fill_value: 0,
            photon_conversion_factor: 1.0,
            read_noise_electrons: 2.5,
            photon_flux: 50.0,
            bead_density: 100,
            bead_size: 2.0,
            bead_brightness: 1.0,
            bead_blur_rate: 0.5,
            async_property_leader: "init".into(),
            async_property_follower: "init".into(),
            async_property_delay_ms: 2000,
            use_exposure_sequences: false,
            exposure_sequence: Vec::new(),
            exposure_sequence_running: false,
            exposure_sequence_index: 0,
            sequence_remaining: None,
            sequence_interval_ms: 0.0,
            sequence_last_frame: None,
            mode: MODE_ARTIFICIAL_WAVES.into(),
        }
    }

    fn set_allowed_binning_for_scan_mode(&mut self) -> MmResult<()> {
        let allowed: &[&str] = match self.scan_mode {
            1 => &["1", "2", "4", "8"],
            2 => &["1", "2", "4"],
            _ => &["1", "2"],
        };
        self.props.set_allowed_values("Binning", allowed)?;
        if self.scan_mode == 3 && [4, 8].contains(&self.binning) {
            self.set_binning_checked(2)?;
        } else if self.scan_mode == 2 && self.binning == 8 {
            self.set_binning_checked(4)?;
        }
        Ok(())
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
        let allowed = match self.scan_mode {
            1 => [1, 2, 4, 8].contains(&bin),
            2 => [1, 2, 4].contains(&bin),
            _ => [1, 2].contains(&bin),
        };
        if !allowed {
            return Err(MmError::InvalidPropertyValue);
        }
        let old_binning = self.binning;
        let old_roi = self.roi;
        self.props
            .set("Binning", PropertyValue::Integer(bin as i64))?;
        self.binning = bin;
        self.width = self.camera_ccd_x_size / bin as u32;
        self.height = self.camera_ccd_y_size / bin as u32;
        if old_roi.x == 0
            && old_roi.y == 0
            && old_roi.width == self.camera_ccd_x_size / old_binning as u32
            && old_roi.height == self.camera_ccd_y_size / old_binning as u32
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

    fn sequence_exposure(&mut self) -> f64 {
        if self.exposure_sequence_running && !self.exposure_sequence.is_empty() {
            let exposure = self.exposure_sequence[self.exposure_sequence_index];
            self.exposure_sequence_index =
                (self.exposure_sequence_index + 1) % self.exposure_sequence.len();
            exposure
        } else {
            self.exposure_ms
        }
    }

    fn wait_for_sequence_interval(&mut self) {
        if !self.capturing || self.sequence_interval_ms <= 0.0 {
            return;
        }
        if let Some(last_frame) = self.sequence_last_frame {
            let interval = Duration::from_secs_f64(self.sequence_interval_ms / 1000.0);
            let elapsed = last_frame.elapsed();
            if interval > elapsed {
                std::thread::sleep(interval - elapsed);
            }
        }
    }

    fn finish_sequence_frame(&mut self) {
        if !self.capturing {
            return;
        }
        self.sequence_last_frame = Some(Instant::now());
        if let Some(remaining) = self.sequence_remaining.as_mut() {
            *remaining -= 1;
            if *remaining <= 0 {
                let _ = self.stop_sequence_acquisition();
            }
        }
    }

    pub fn clear_exposure_sequence(&mut self) -> MmResult<()> {
        if !self.use_exposure_sequences {
            return Err(MmError::UnsupportedCommand);
        }
        self.exposure_sequence.clear();
        self.exposure_sequence_index = 0;
        Ok(())
    }

    pub fn add_to_exposure_sequence(&mut self, exposure_time_ms: f64) -> MmResult<()> {
        if !self.use_exposure_sequences {
            return Err(MmError::UnsupportedCommand);
        }
        if exposure_time_ms < 0.0 || exposure_time_ms > self.exposure_maximum_ms {
            return Err(MmError::InvalidPropertyValue);
        }
        self.exposure_sequence.push(exposure_time_ms);
        Ok(())
    }

    pub fn start_exposure_sequence(&mut self) -> MmResult<()> {
        if !self.use_exposure_sequences {
            return Err(MmError::UnsupportedCommand);
        }
        self.exposure_sequence_running = true;
        Ok(())
    }

    pub fn stop_exposure_sequence(&mut self) -> MmResult<()> {
        if !self.use_exposure_sequences {
            return Err(MmError::UnsupportedCommand);
        }
        self.exposure_sequence_running = false;
        self.exposure_sequence_index = 0;
        Ok(())
    }

    pub fn exposure_sequence_max_length(&self) -> MmResult<i64> {
        if !self.use_exposure_sequences {
            return Err(MmError::UnsupportedCommand);
        }
        Ok(1_000)
    }

    pub fn is_exposure_sequenceable(&self) -> bool {
        self.use_exposure_sequences
    }

    /// Generate a synthetic test pattern (sine wave gradient).
    fn generate_image(&mut self, exposure_ms: f64) {
        let w = self.roi.width as usize;
        let h = self.roi.height as usize;
        let bytes_per_pixel = self.bytes_per_pixel as usize;
        let buf = &mut self.image_buf;
        buf.resize(w * h * bytes_per_pixel, 0);
        let exposure_scale = (exposure_ms / self.exposure_maximum_ms.max(1.0)).clamp(0.0, 1.0);

        match self.mode.as_str() {
            MODE_NOISE => {
                for y in 0..h {
                    for x in 0..w {
                        let noise = ((x.wrapping_mul(73) ^ y.wrapping_mul(151)) & 0xff) as f64;
                        let val = (10.0 + noise * exposure_scale).min(255.0).round() as u8;
                        let idx = (y * w + x) * bytes_per_pixel;
                        for byte in &mut buf[idx..idx + bytes_per_pixel] {
                            *byte = val;
                        }
                    }
                }
            }
            MODE_COLOR_TEST => {
                for y in 0..h {
                    for x in 0..w {
                        let idx = (y * w + x) * bytes_per_pixel;
                        let band = if w == 0 { 0 } else { x * 6 / w };
                        let val = ((band * 42) as f64 * exposure_scale).round() as u8;
                        for byte in &mut buf[idx..idx + bytes_per_pixel] {
                            *byte = val;
                        }
                        if bytes_per_pixel >= 4 {
                            buf[idx] = if band == 0 || band == 3 { 255 } else { 0 };
                            buf[idx + 1] = if band == 1 || band == 3 { 255 } else { 0 };
                            buf[idx + 2] = if band == 2 || band == 3 { 255 } else { 0 };
                            buf[idx + 3] = 255;
                        }
                    }
                }
            }
            MODE_BEADS => {
                for y in 0..h {
                    for x in 0..w {
                        let tile_x = x / 32;
                        let tile_y = y / 32;
                        let center_x = tile_x * 32 + ((tile_y * 11 + 13) % 24) + 4;
                        let center_y = tile_y * 32 + ((tile_x * 7 + 9) % 24) + 4;
                        let dx = x as isize - center_x as isize;
                        let dy = y as isize - center_y as isize;
                        let dist2 = dx * dx + dy * dy;
                        let val = if dist2 <= 9 {
                            (220.0 * exposure_scale).round() as u8
                        } else {
                            (8.0 * exposure_scale).round() as u8
                        };
                        let idx = (y * w + x) * bytes_per_pixel;
                        for byte in &mut buf[idx..idx + bytes_per_pixel] {
                            *byte = val;
                        }
                    }
                }
            }
            _ => {
                for y in 0..h {
                    for x in 0..w {
                        let raw = ((x + y) % 256) as f64;
                        let val = (raw * exposure_scale).round() as u8;
                        let idx = (y * w + x) * bytes_per_pixel;
                        for byte in &mut buf[idx..idx + bytes_per_pixel] {
                            *byte = val;
                        }
                    }
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
        "Demo Camera Device Adapter"
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
            "MaximumExposureMs" => Ok(PropertyValue::Float(self.exposure_maximum_ms)),
            "ScanMode" => Ok(PropertyValue::Integer(self.scan_mode)),
            "Gain" => Ok(PropertyValue::Integer(self.gain)),
            "Offset" => Ok(PropertyValue::Integer(self.offset)),
            "CCDTemperature" | "CCDTemperature RO" => {
                Ok(PropertyValue::Float(self.ccd_temperature))
            }
            "OnCameraCCDXSize" => Ok(PropertyValue::Integer(self.camera_ccd_x_size as i64)),
            "OnCameraCCDYSize" => Ok(PropertyValue::Integer(self.camera_ccd_y_size as i64)),
            "TriggerDevice" => Ok(PropertyValue::String(self.trigger_device.clone())),
            "DropPixels" => Ok(PropertyValue::Integer(self.drop_pixels as i64)),
            "SaturatePixels" => Ok(PropertyValue::Integer(self.saturate_pixels as i64)),
            "FastImage" => Ok(PropertyValue::Integer(self.fast_image as i64)),
            "FractionOfPixelsToDropOrSaturate" => {
                Ok(PropertyValue::Float(self.fraction_drop_or_saturate))
            }
            "RotateImages" => Ok(PropertyValue::Integer(self.rotate_images as i64)),
            "DisplayImageNumber" => Ok(PropertyValue::Integer(self.display_image_number as i64)),
            "StripeWidth" => Ok(PropertyValue::Float(self.stripe_width)),
            "AllowMultiROI" => Ok(PropertyValue::Integer(self.allow_multi_roi as i64)),
            "MultiROIFillValue" => Ok(PropertyValue::Integer(self.multi_roi_fill_value)),
            "Photon Conversion Factor" => Ok(PropertyValue::Float(self.photon_conversion_factor)),
            "ReadNoise (electrons)" => Ok(PropertyValue::Float(self.read_noise_electrons)),
            "Photon Flux" => Ok(PropertyValue::Float(self.photon_flux)),
            "BeadDensity" => Ok(PropertyValue::Integer(self.bead_density)),
            "BeadSize" => Ok(PropertyValue::Float(self.bead_size)),
            "BeadBrightness" => Ok(PropertyValue::Float(self.bead_brightness)),
            "BeadBlurRate" => Ok(PropertyValue::Float(self.bead_blur_rate)),
            "AsyncPropertyLeader" => Ok(PropertyValue::String(self.async_property_leader.clone())),
            "AsyncPropertyFollower" => {
                Ok(PropertyValue::String(self.async_property_follower.clone()))
            }
            "AsyncPropertyDelayMS" => Ok(PropertyValue::Integer(self.async_property_delay_ms)),
            "UseExposureSequences" => Ok(PropertyValue::String(
                if self.use_exposure_sequences {
                    "Yes"
                } else {
                    "No"
                }
                .into(),
            )),
            "Mode" => Ok(PropertyValue::String(self.mode.clone())),
            "SimulateCrash" => Ok(PropertyValue::String(String::new())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Exposure" => {
                let exposure = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(exposure))?;
                self.exposure_ms = exposure;
                Ok(())
            }
            "MaximumExposureMs" => {
                let max = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if max < 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.exposure_maximum_ms = max;
                self.props.set(name, PropertyValue::Float(max))?;
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
            "ScanMode" => {
                let scan_mode = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !(1..=3).contains(&scan_mode) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.props.set(name, PropertyValue::Integer(scan_mode))?;
                self.scan_mode = scan_mode;
                self.set_allowed_binning_for_scan_mode()
            }
            "Gain" => {
                let gain = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(gain))?;
                self.gain = gain;
                Ok(())
            }
            "Offset" => {
                let offset = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(offset))?;
                self.offset = offset;
                Ok(())
            }
            "CCDTemperature" => {
                let temp = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(temp))?;
                self.ccd_temperature = temp;
                Ok(())
            }
            "OnCameraCCDXSize" | "OnCameraCCDYSize" => {
                let size = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !(16..=33_000).contains(&size) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.props.set(name, PropertyValue::Integer(size))?;
                if name == "OnCameraCCDXSize" {
                    self.camera_ccd_x_size = size as u32;
                } else {
                    self.camera_ccd_y_size = size as u32;
                }
                self.width = self.camera_ccd_x_size / self.binning as u32;
                self.height = self.camera_ccd_y_size / self.binning as u32;
                self.roi = ImageRoi::new(0, 0, self.width, self.height);
                self.image_buf.resize(
                    (self.roi.width * self.roi.height * self.bytes_per_pixel) as usize,
                    0,
                );
                Ok(())
            }
            "TriggerDevice" => {
                self.trigger_device = val.as_str().to_string();
                self.props.set(name, val)
            }
            "DropPixels" | "SaturatePixels" | "FastImage" | "RotateImages"
            | "DisplayImageNumber" | "AllowMultiROI" => {
                let enabled = val.as_i64().ok_or(MmError::InvalidPropertyValue)? != 0;
                self.props
                    .set(name, PropertyValue::Integer(enabled as i64))?;
                match name {
                    "DropPixels" => self.drop_pixels = enabled,
                    "SaturatePixels" => self.saturate_pixels = enabled,
                    "FastImage" => self.fast_image = enabled,
                    "RotateImages" => self.rotate_images = enabled,
                    "DisplayImageNumber" => self.display_image_number = enabled,
                    "AllowMultiROI" => self.allow_multi_roi = enabled,
                    _ => {}
                }
                Ok(())
            }
            "FractionOfPixelsToDropOrSaturate" => {
                let fraction = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(fraction))?;
                self.fraction_drop_or_saturate = fraction;
                Ok(())
            }
            "StripeWidth" => {
                let width = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(width))?;
                self.stripe_width = width;
                Ok(())
            }
            "MultiROIFillValue" => {
                let fill = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(fill))?;
                self.multi_roi_fill_value = fill;
                Ok(())
            }
            "Photon Conversion Factor" => {
                let factor = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(factor))?;
                self.photon_conversion_factor = factor;
                Ok(())
            }
            "ReadNoise (electrons)" => {
                let noise = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(noise))?;
                self.read_noise_electrons = noise;
                Ok(())
            }
            "Photon Flux" => {
                let flux = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(flux))?;
                self.photon_flux = flux;
                Ok(())
            }
            "BeadDensity" => {
                let density = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(density))?;
                self.bead_density = density;
                Ok(())
            }
            "BeadSize" => {
                let size = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(size))?;
                self.bead_size = size;
                Ok(())
            }
            "BeadBrightness" => {
                let brightness = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(brightness))?;
                self.bead_brightness = brightness;
                Ok(())
            }
            "BeadBlurRate" => {
                let rate = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(rate))?;
                self.bead_blur_rate = rate;
                Ok(())
            }
            "AsyncPropertyLeader" => {
                let leader = val.as_str().to_string();
                self.props
                    .set(name, PropertyValue::String(leader.clone()))?;
                self.async_property_leader = leader;
                Ok(())
            }
            "AsyncPropertyDelayMS" => {
                let delay = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(delay))?;
                self.async_property_delay_ms = delay;
                Ok(())
            }
            "UseExposureSequences" => {
                let setting = val.as_str();
                self.props
                    .set(name, PropertyValue::String(setting.to_string()))?;
                self.use_exposure_sequences = setting == "Yes";
                if !self.use_exposure_sequences {
                    self.exposure_sequence_running = false;
                    self.exposure_sequence_index = 0;
                }
                Ok(())
            }
            "Mode" => {
                let mode = val.as_str().to_string();
                self.props.set(name, PropertyValue::String(mode.clone()))?;
                self.mode = mode;
                Ok(())
            }
            "SimulateCrash" => {
                self.props.set(name, val)?;
                self.props.set(name, PropertyValue::String(String::new()))
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
        self.wait_for_sequence_interval();
        let exposure_ms = self.sequence_exposure();
        if exposure_ms > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(exposure_ms / 1000.0));
        }
        self.generate_image(exposure_ms);
        self.readout_start.set(Some(Instant::now()));
        self.finish_sequence_frame();
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

    fn set_exposure(&mut self, exp_ms: f64) -> MmResult<()> {
        self.props.set("Exposure", PropertyValue::Float(exp_ms))?;
        self.exposure_ms = exp_ms;
        Ok(())
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
        self.sequence_remaining = if _count > 0 { Some(_count) } else { None };
        self.sequence_interval_ms = _interval_ms.max(0.0);
        self.sequence_last_frame = None;
        self.capturing = true;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        self.capturing = false;
        self.sequence_remaining = None;
        self.sequence_interval_ms = 0.0;
        self.sequence_last_frame = None;
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

    #[test]
    fn upstream_simple_property_surface_is_stateful() {
        let mut cam = DemoCamera::new();

        for name in [
            "CameraID",
            "MaximumExposureMs",
            "TestProperty1",
            "TestProperty2",
            "TestProperty3",
            "TestProperty4",
            "TestProperty5",
            "TestProperty6",
            "ScanMode",
            "Gain",
            "Offset",
            "CCDTemperature",
            "CCDTemperature RO",
            "OnCameraCCDXSize",
            "OnCameraCCDYSize",
            "TriggerDevice",
            "DropPixels",
            "SaturatePixels",
            "FastImage",
            "FractionOfPixelsToDropOrSaturate",
            "RotateImages",
            "DisplayImageNumber",
            "StripeWidth",
            "AllowMultiROI",
            "MultiROIFillValue",
            "Photon Conversion Factor",
            "ReadNoise (electrons)",
            "Photon Flux",
            "BeadDensity",
            "BeadSize",
            "BeadBrightness",
            "BeadBlurRate",
            "AsyncPropertyLeader",
            "AsyncPropertyFollower",
            "AsyncPropertyDelayMS",
            "UseExposureSequences",
            "Mode",
            "SimulateCrash",
        ] {
            assert!(cam.has_property(name), "{name}");
        }

        cam.set_property("Gain", PropertyValue::Integer(8)).unwrap();
        cam.set_property("CCDTemperature", PropertyValue::Float(-20.0))
            .unwrap();
        cam.set_property("TriggerDevice", PropertyValue::String("DShutter".into()))
            .unwrap();
        cam.set_property("Mode", PropertyValue::String(MODE_BEADS.into()))
            .unwrap();
        cam.set_property("UseExposureSequences", PropertyValue::String("Yes".into()))
            .unwrap();

        assert_eq!(cam.get_property("Gain").unwrap(), PropertyValue::Integer(8));
        assert_eq!(
            cam.get_property("CCDTemperature RO").unwrap(),
            PropertyValue::Float(-20.0)
        );
        assert_eq!(
            cam.get_property("TriggerDevice").unwrap(),
            PropertyValue::String("DShutter".into())
        );
        assert_eq!(
            cam.get_property("Mode").unwrap(),
            PropertyValue::String(MODE_BEADS.into())
        );
        assert_eq!(
            cam.get_property("UseExposureSequences").unwrap(),
            PropertyValue::String("Yes".into())
        );
        assert_eq!(
            cam.get_property("StripeWidth").unwrap(),
            PropertyValue::Float(0.0)
        );
        assert_eq!(
            cam.get_property("CameraID").unwrap(),
            PropertyValue::String("V1.0".into())
        );
        assert!(cam.is_property_read_only("AsyncPropertyFollower"));
        assert_eq!(
            cam.get_property("AsyncPropertyFollower").unwrap(),
            PropertyValue::String("init".into())
        );
        cam.set_property(
            "AsyncPropertyLeader",
            PropertyValue::String("new leader".into()),
        )
        .unwrap();
        assert_eq!(
            cam.get_property("AsyncPropertyLeader").unwrap(),
            PropertyValue::String("new leader".into())
        );
        cam.set_property("AsyncPropertyDelayMS", PropertyValue::Integer(50))
            .unwrap();
        assert_eq!(
            cam.get_property("AsyncPropertyDelayMS").unwrap(),
            PropertyValue::Integer(50)
        );
        cam.set_property(
            "SimulateCrash",
            PropertyValue::String("Dereference Null Pointer".into()),
        )
        .unwrap();
        assert_eq!(
            cam.get_property("SimulateCrash").unwrap(),
            PropertyValue::String(String::new())
        );
        assert!(cam
            .set_property("SimulateCrash", PropertyValue::String("Other".into()))
            .is_err());

        cam.set_property("TestProperty1", PropertyValue::Float(0.05))
            .unwrap();
        assert_eq!(
            cam.get_property("TestProperty1").unwrap(),
            PropertyValue::Float(0.05)
        );
        assert!(cam
            .set_property("TestProperty1", PropertyValue::Float(0.2))
            .is_err());
        cam.set_property("TestProperty5", PropertyValue::Float(1.0e9))
            .unwrap();
    }

    #[test]
    fn upstream_numeric_tuning_property_limits_are_enforced() {
        let mut cam = DemoCamera::new();

        for (name, good, bad) in [
            (
                "Photon Conversion Factor",
                PropertyValue::Float(10.0),
                PropertyValue::Float(10.1),
            ),
            (
                "ReadNoise (electrons)",
                PropertyValue::Float(0.25),
                PropertyValue::Float(0.24),
            ),
            (
                "Photon Flux",
                PropertyValue::Float(5000.0),
                PropertyValue::Float(5000.1),
            ),
            (
                "BeadDensity",
                PropertyValue::Integer(500),
                PropertyValue::Integer(501),
            ),
            (
                "BeadSize",
                PropertyValue::Float(10.0),
                PropertyValue::Float(10.1),
            ),
            (
                "BeadBrightness",
                PropertyValue::Float(0.125),
                PropertyValue::Float(0.124),
            ),
            (
                "BeadBlurRate",
                PropertyValue::Float(1.0),
                PropertyValue::Float(1.1),
            ),
        ] {
            cam.set_property(name, good.clone()).unwrap();
            assert_eq!(cam.get_property(name).unwrap(), good);
            assert_eq!(
                cam.set_property(name, bad).unwrap_err(),
                MmError::InvalidPropertyValue,
                "{name}"
            );
            assert_eq!(cam.get_property(name).unwrap(), good, "{name}");
        }

        cam.set_property("Gain", PropertyValue::Integer(-5))
            .unwrap();
        assert_eq!(
            cam.set_property("Gain", PropertyValue::Integer(-6))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            cam.get_property("Gain").unwrap(),
            PropertyValue::Integer(-5)
        );

        cam.set_property("StripeWidth", PropertyValue::Float(10.0))
            .unwrap();
        assert_eq!(
            cam.set_property("StripeWidth", PropertyValue::Float(10.1))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            cam.get_property("StripeWidth").unwrap(),
            PropertyValue::Float(10.0)
        );
    }

    #[test]
    fn scan_mode_updates_allowed_binning_like_upstream() {
        let mut cam = DemoCamera::new();
        cam.set_binning(8).unwrap();
        cam.set_property("ScanMode", PropertyValue::Integer(2))
            .unwrap();
        assert_eq!(cam.get_binning(), 4);
        assert!(cam.set_binning(8).is_err());
        cam.set_property("ScanMode", PropertyValue::Integer(3))
            .unwrap();
        assert_eq!(cam.get_binning(), 2);
        assert!(cam.set_binning(4).is_err());
    }

    #[test]
    fn ccd_size_properties_resize_full_frame_and_validate_bounds() {
        let mut cam = DemoCamera::new();
        cam.set_property("OnCameraCCDXSize", PropertyValue::Integer(1024))
            .unwrap();
        cam.set_property("OnCameraCCDYSize", PropertyValue::Integer(256))
            .unwrap();
        assert_eq!(cam.get_roi().unwrap(), ImageRoi::new(0, 0, 1024, 256));
        cam.set_binning(2).unwrap();
        assert_eq!(cam.get_roi().unwrap(), ImageRoi::new(0, 0, 512, 128));
        assert!(cam
            .set_property("OnCameraCCDXSize", PropertyValue::Integer(15))
            .is_err());
    }

    #[test]
    fn ccd_size_properties_match_upstream_during_capture() {
        let mut cam = DemoCamera::new();
        cam.initialize().unwrap();
        cam.start_sequence_acquisition(0, 0.0).unwrap();

        cam.set_property("OnCameraCCDXSize", PropertyValue::Integer(1024))
            .unwrap();
        cam.set_property("OnCameraCCDYSize", PropertyValue::Integer(256))
            .unwrap();

        assert!(cam.is_capturing());
        assert_eq!(cam.get_roi().unwrap(), ImageRoi::new(0, 0, 1024, 256));
    }

    #[test]
    fn maximum_exposure_only_updates_cached_value_like_upstream() {
        let mut cam = DemoCamera::new();
        cam.set_property("Exposure", PropertyValue::Float(80.0))
            .unwrap();
        cam.set_property("MaximumExposureMs", PropertyValue::Float(50.0))
            .unwrap();
        assert_eq!(cam.get_exposure(), 80.0);
        cam.set_property("Exposure", PropertyValue::Float(60.0))
            .unwrap();
        assert_eq!(cam.get_exposure(), 60.0);
        cam.set_exposure(60.0).unwrap();
        assert_eq!(cam.get_exposure(), 60.0);
    }

    #[test]
    fn exposure_sequence_cycles_only_when_enabled() {
        let mut cam = DemoCamera::new();

        assert!(!cam.is_exposure_sequenceable());
        assert_eq!(
            cam.add_to_exposure_sequence(1.0).unwrap_err(),
            MmError::UnsupportedCommand
        );

        cam.set_property("UseExposureSequences", PropertyValue::String("Yes".into()))
            .unwrap();
        assert!(cam.is_exposure_sequenceable());
        assert_eq!(cam.exposure_sequence_max_length().unwrap(), 1_000);
        cam.add_to_exposure_sequence(5.0).unwrap();
        cam.add_to_exposure_sequence(15.0).unwrap();
        cam.start_exposure_sequence().unwrap();

        assert_eq!(cam.sequence_exposure(), 5.0);
        assert_eq!(cam.sequence_exposure(), 15.0);
        assert_eq!(cam.sequence_exposure(), 5.0);

        cam.stop_exposure_sequence().unwrap();
        assert_eq!(cam.sequence_exposure(), cam.get_exposure());
        cam.clear_exposure_sequence().unwrap();
        assert!(cam.exposure_sequence.is_empty());
    }

    #[test]
    fn finite_sequence_acquisition_stops_after_requested_snaps() {
        let mut cam = DemoCamera::new();
        cam.initialize().unwrap();
        cam.set_exposure(0.0).unwrap();
        cam.start_sequence_acquisition(2, 0.0).unwrap();

        cam.snap_image().unwrap();
        assert!(cam.is_capturing());
        cam.snap_image().unwrap();
        assert!(!cam.is_capturing());
        assert_eq!(cam.sequence_remaining, None);
    }

    #[test]
    fn generated_image_uses_selected_sequence_exposure() {
        let mut cam = DemoCamera::new();
        cam.generate_image(0.0);
        let dark = cam.image_buf[1];
        cam.generate_image(cam.exposure_maximum_ms);
        let bright = cam.image_buf[1];

        assert_eq!(dark, 0);
        assert!(bright > dark);
    }

    #[test]
    fn selected_modes_generate_distinct_bounded_patterns() {
        let mut cam = DemoCamera::new();
        cam.set_roi(ImageRoi::new(0, 0, 64, 64)).unwrap();
        cam.set_exposure(cam.exposure_maximum_ms).unwrap();

        cam.set_property("Mode", PropertyValue::String(MODE_ARTIFICIAL_WAVES.into()))
            .unwrap();
        cam.generate_image(cam.get_exposure());
        let waves = cam.image_buf.clone();

        cam.set_property("Mode", PropertyValue::String(MODE_NOISE.into()))
            .unwrap();
        cam.generate_image(cam.get_exposure());
        let noise = cam.image_buf.clone();

        cam.set_property("Mode", PropertyValue::String(MODE_BEADS.into()))
            .unwrap();
        cam.generate_image(cam.get_exposure());
        let beads = cam.image_buf.clone();

        assert_ne!(waves, noise);
        assert_ne!(waves, beads);
        assert!(beads.iter().any(|&v| v >= 200));
    }
}
