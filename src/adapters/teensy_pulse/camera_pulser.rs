//! Camera wrapper for the Teensy pulse-generator adapter.
//!
//! The upstream `CameraPulser` is a camera device that looks up one loaded
//! physical camera through MMCore and uses Teensy pulses to trigger snaps and
//! sequences.  This Rust translation keeps the registered device and Teensy
//! property/serial behavior pointer-free; MiniCore owns the physical-camera
//! fan-out through dependency binding.

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Camera, Device};
use crate::transport::Transport;
use crate::types::{DeviceType, ImageRoi, PropertyValue};

use super::pulse::{CMD_NR_PULSES, CMD_PULSE_DUR, CMD_START, CMD_TRIGGER, CMD_VERSION, ENQUIRE};

const MAX_NUMBER_PHYSICAL_CAMERAS: usize = 1;
const MIN_MM_VERSION: u32 = 1;
const MAX_MM_VERSION: u32 = 1;

pub struct TeensyCameraPulser {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    version: u32,
    pulse_duration_ms: f64,
    interval_beyond_exposure_ms: f64,
    wait_for_input: bool,
    used_cameras: [String; MAX_NUMBER_PHYSICAL_CAMERAS],
    image: Vec<u8>,
}

impl TeensyCameraPulser {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property(
                "Name",
                PropertyValue::String("TeensySendsPulsesToCamera".into()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Description",
                PropertyValue::String(
                    "Use Camera in external trigger mode and provide triggers with Teensy".into(),
                ),
                true,
            )
            .unwrap();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            version: 0,
            pulse_duration_ms: 1.0,
            interval_beyond_exposure_ms: 5.0,
            wait_for_input: false,
            used_cameras: [String::from("Undefined")],
            image: Vec::new(),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn send_command(&mut self, cmd: u8, param: u32) -> MmResult<()> {
        let mut bytes = [0u8; 5];
        bytes[0] = cmd;
        bytes[1..].copy_from_slice(&param.to_le_bytes());
        self.call_transport(|t| {
            t.purge()?;
            t.send_bytes(&bytes)
        })
    }

    fn enquire(&mut self, cmd: u8) -> MmResult<()> {
        self.call_transport(|t| t.send_bytes(&[ENQUIRE, cmd]))
    }

    fn get_response(&mut self, expected_cmd: u8) -> MmResult<u32> {
        let raw = self.call_transport(|t| t.receive_bytes(5))?;
        if raw.len() < 5 || raw[0] != expected_cmd {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(u32::from_le_bytes([raw[1], raw[2], raw[3], raw[4]]))
    }

    fn get_param(&mut self, cmd: u8) -> MmResult<u32> {
        self.enquire(cmd)?;
        self.get_response(cmd)
    }

    fn set_param(&mut self, cmd: u8, val: u32) -> MmResult<u32> {
        self.send_command(cmd, val)?;
        self.get_response(cmd)
    }

    fn has_physical_camera(&self) -> bool {
        self.used_cameras.iter().any(|camera| camera != "Undefined")
    }

    fn no_physical_camera<T>(&self) -> MmResult<T> {
        Err(MmError::CoreCameraNotAvailable)
    }
}

impl Default for TeensyCameraPulser {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for TeensyCameraPulser {
    fn name(&self) -> &str {
        "TeensySendsPulsesToCamera"
    }

    fn description(&self) -> &str {
        "Use Camera in external trigger mode and provide triggers with Teensy"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.initialized {
            return Ok(());
        }
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        for i in 0..MAX_NUMBER_PHYSICAL_CAMERAS {
            let name = format!("Triggered Camera-{i}");
            self.props
                .define_property(
                    &name,
                    PropertyValue::String(self.used_cameras[i].clone()),
                    false,
                )
                .unwrap();
            self.props
                .set_allowed_values(&name, &["Undefined"])
                .unwrap();
        }
        self.props
            .define_property("Binning", PropertyValue::Integer(1), false)
            .unwrap();

        self.send_command(CMD_VERSION, 0)?;
        self.version = self.get_response(CMD_VERSION)?;
        if self.version < MIN_MM_VERSION {
            return Err(MmError::Err);
        }
        if self.version > MAX_MM_VERSION {
            return Err(MmError::Err);
        }
        self.props
            .define_property("Version", PropertyValue::Integer(self.version as i64), true)
            .unwrap();

        let pulse_us = self.get_param(CMD_PULSE_DUR)?;
        self.pulse_duration_ms = pulse_us as f64 / 1000.0;
        self.props
            .define_property(
                "PulseDuration-ms",
                PropertyValue::Float(self.pulse_duration_ms),
                false,
            )
            .unwrap();

        self.props
            .define_property(
                "Interval-ms_on_top_of_exposure",
                PropertyValue::Float(self.interval_beyond_exposure_ms),
                false,
            )
            .unwrap();

        let wait = self.get_param(CMD_TRIGGER)?;
        self.wait_for_input = wait != 0;
        self.props
            .define_property(
                "Wait_for_Input",
                PropertyValue::String(if self.wait_for_input { "On" } else { "Off" }.into()),
                false,
            )
            .unwrap();
        self.props
            .set_allowed_values("Wait_for_Input", &["Off", "On"])
            .unwrap();

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Version" => Ok(PropertyValue::Integer(self.version as i64)),
            "PulseDuration-ms" => Ok(PropertyValue::Float(self.pulse_duration_ms)),
            "Interval-ms_on_top_of_exposure" => {
                Ok(PropertyValue::Float(self.interval_beyond_exposure_ms))
            }
            "Wait_for_Input" => Ok(PropertyValue::String(
                if self.wait_for_input { "On" } else { "Off" }.into(),
            )),
            "Triggered Camera-0" => Ok(PropertyValue::String(self.used_cameras[0].clone())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "PulseDuration-ms" if self.initialized => {
                let ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                let us = (ms * 1000.0) as u32;
                let response = self.set_param(CMD_PULSE_DUR, us)?;
                if response != us {
                    return Err(MmError::SerialInvalidResponse);
                }
                self.pulse_duration_ms = ms;
                self.props.set(name, PropertyValue::Float(ms))
            }
            "Interval-ms_on_top_of_exposure" if self.initialized => {
                let ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                let us = (self.get_exposure() + ms) * 1000.0;
                let interval = us as u32;
                let response = self.set_param(super::pulse::CMD_INTERVAL, interval)?;
                if response != interval {
                    return Err(MmError::SerialInvalidResponse);
                }
                self.interval_beyond_exposure_ms = ms;
                self.props.set(name, PropertyValue::Float(ms))
            }
            "Wait_for_Input" if self.initialized => {
                let on = val.as_str() == "On";
                let param = if on { 1 } else { 0 };
                let response = self.set_param(CMD_TRIGGER, param)?;
                if response != param {
                    return Err(MmError::SerialInvalidResponse);
                }
                self.wait_for_input = on;
                self.props.set(
                    name,
                    PropertyValue::String(if on { "On" } else { "Off" }.into()),
                )
            }
            "Triggered Camera-0" => {
                let camera = val.as_str();
                if camera == "Undefined" {
                    self.props.set_allowed_values(name, &["Undefined"])?;
                } else {
                    self.props
                        .set_allowed_values(name, &["Undefined", camera])?;
                }
                self.used_cameras[0] = camera.to_string();
                self.props.set(name, PropertyValue::String(camera.into()))
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

impl Camera for TeensyCameraPulser {
    fn snap_image(&mut self) -> MmResult<()> {
        if !self.has_physical_camera() {
            return Err(MmError::CoreCameraNotAvailable);
        }
        let response = self.set_param(CMD_NR_PULSES, 1)?;
        if response != 1 {
            return Err(MmError::SerialInvalidResponse);
        }
        self.set_param(CMD_START, 0).map(|_| ())
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if !self.has_physical_camera() {
            return Err(MmError::CoreCameraNotAvailable);
        }
        Ok(&self.image)
    }

    fn get_image_width(&self) -> u32 {
        0
    }

    fn get_image_height(&self) -> u32 {
        0
    }

    fn get_image_bytes_per_pixel(&self) -> u32 {
        0
    }

    fn get_bit_depth(&self) -> u32 {
        0
    }

    fn get_number_of_components(&self) -> u32 {
        1
    }

    fn get_number_of_channels(&self) -> u32 {
        self.used_cameras
            .iter()
            .filter(|camera| camera.as_str() != "Undefined")
            .count() as u32
    }

    fn get_exposure(&self) -> f64 {
        0.0
    }

    fn set_exposure(&mut self, _exp_ms: f64) -> MmResult<()> {
        self.no_physical_camera()
    }

    fn get_binning(&self) -> i32 {
        1
    }

    fn set_binning(&mut self, _bin: i32) -> MmResult<()> {
        self.no_physical_camera()
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        self.no_physical_camera()
    }

    fn set_roi(&mut self, _roi: ImageRoi) -> MmResult<()> {
        self.no_physical_camera()
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        self.no_physical_camera()
    }

    fn start_sequence_acquisition(&mut self, count: i64, _interval_ms: f64) -> MmResult<()> {
        if !self.has_physical_camera() {
            return Err(MmError::CoreCameraNotAvailable);
        }
        let pulses = u32::try_from(count).map_err(|_| MmError::InvalidInputParam)?;
        let response = self.set_param(CMD_NR_PULSES, pulses)?;
        if response != pulses {
            return Err(MmError::SerialInvalidResponse);
        }
        let interval = ((self.get_exposure() + self.interval_beyond_exposure_ms) * 1000.0) as u32;
        let response = self.set_param(super::pulse::CMD_INTERVAL, interval)?;
        if response != interval {
            return Err(MmError::SerialInvalidResponse);
        }
        self.set_param(CMD_START, 0)?;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        self.set_param(super::pulse::CMD_STOP, 0).map(|_| ())
    }

    fn is_capturing(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn resp(cmd: u8, val: u32) -> Vec<u8> {
        let v = val.to_le_bytes();
        vec![cmd, v[0], v[1], v[2], v[3]]
    }

    #[test]
    fn initializes_upstream_camera_pulser_properties() {
        let t = MockTransport::new()
            .expect_binary(&resp(CMD_VERSION, 1))
            .expect_binary(&resp(CMD_PULSE_DUR, 1_000))
            .expect_binary(&resp(CMD_TRIGGER, 0));
        let mut dev = TeensyCameraPulser::new().with_transport(Box::new(t));

        dev.initialize().unwrap();

        assert_eq!(dev.device_type(), DeviceType::Camera);
        assert_eq!(
            dev.get_property("Triggered Camera-0").unwrap(),
            PropertyValue::String("Undefined".into())
        );
        assert_eq!(
            dev.get_property("PulseDuration-ms").unwrap(),
            PropertyValue::Float(1.0)
        );
        assert_eq!(
            dev.get_property("Wait_for_Input").unwrap(),
            PropertyValue::String("Off".into())
        );
    }

    #[test]
    fn accepts_physical_camera_label_without_storing_camera_pointer() {
        let t = MockTransport::new()
            .expect_binary(&resp(CMD_VERSION, 1))
            .expect_binary(&resp(CMD_PULSE_DUR, 1_000))
            .expect_binary(&resp(CMD_TRIGGER, 0));
        let mut dev = TeensyCameraPulser::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Triggered Camera-0", PropertyValue::String("cam".into()))
            .unwrap();
        assert_eq!(
            dev.get_property("Triggered Camera-0").unwrap(),
            PropertyValue::String("cam".into())
        );
        assert_eq!(dev.get_number_of_channels(), 1);
    }
}
