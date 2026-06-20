//! ESP32 input monitor generic device.

use parking_lot::Mutex;

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

const PROP_PIN: &str = "Pin";
const PROP_PULL_UP: &str = "Pull-Up-Resistor";
const PROP_DIGITAL_INPUT: &str = "DigitalInput";

pub struct Esp32Input {
    props: PropertyMap,
    transport: Mutex<Option<Box<dyn Transport>>>,
    initialized: bool,
    pin: Option<u8>,
    pull_up: bool,
}

impl Esp32Input {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property(PROP_PIN, PropertyValue::String("All".into()))
            .unwrap();
        props
            .set_allowed_values(PROP_PIN, &["All", "0", "1", "2", "3", "4", "5"])
            .unwrap();
        props
            .define_pre_init_property(PROP_PULL_UP, PropertyValue::String("On".into()))
            .unwrap();
        props
            .set_allowed_values(PROP_PULL_UP, &["On", "Off"])
            .unwrap();

        Self {
            props,
            transport: Mutex::new(None),
            initialized: false,
            pin: None,
            pull_up: true,
        }
    }

    pub fn with_transport(self, t: Box<dyn Transport>) -> Self {
        *self.transport.lock() = Some(t);
        self
    }

    fn with_transport_ref<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        let mut guard = self.transport.lock();
        match guard.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn parse_prefixed_integer(response: &str, prefix: char) -> MmResult<i64> {
        let mut parts = response.trim().split(',');
        let command = parts.next().unwrap_or_default();
        let value = parts
            .next()
            .ok_or(MmError::SerialInvalidResponse)?
            .trim()
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        if command != prefix.to_string() {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(value)
    }

    fn read_digital_input(&self) -> MmResult<i64> {
        let test_pin = self.pin.unwrap_or(6);
        self.with_transport_ref(|t| {
            t.send(&format!("L,{}", test_pin))?;
            Self::parse_prefixed_integer(&t.receive_line()?, 'L')
        })
    }

    fn read_analog_input(&self, channel: u8) -> MmResult<i64> {
        self.with_transport_ref(|t| {
            t.send(&format!("A,{}", channel))?;
            Self::parse_prefixed_integer(&t.receive_line()?, 'A')
        })
    }

    fn set_pull_up(&self, pin: u8, state: bool) -> MmResult<()> {
        self.with_transport_ref(|t| t.send(&format!("D,{},{}", pin, u8::from(state))))
    }

    fn analog_property_channel(name: &str) -> Option<u8> {
        name.strip_prefix("AnalogInput= ")?.parse().ok()
    }
}

impl Default for Esp32Input {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Esp32Input {
    fn name(&self) -> &str {
        "ESP32-Input"
    }

    fn description(&self) -> &str {
        "ADC"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.lock().is_none() {
            return Err(MmError::NotConnected);
        }
        let pin_text = self.props.get(PROP_PIN)?.as_str().to_string();
        self.pin = if pin_text == "All" {
            None
        } else {
            Some(
                pin_text
                    .parse()
                    .map_err(|_| MmError::InvalidPropertyValue)?,
            )
        };
        self.pull_up = self.props.get(PROP_PULL_UP)?.as_str() == "On";

        if !self.props.has_property(PROP_DIGITAL_INPUT) {
            self.props
                .define_property(PROP_DIGITAL_INPUT, PropertyValue::Integer(0), true)?;
        }
        let start = self.pin.unwrap_or(0);
        let end = self.pin.unwrap_or(5);
        for channel in start..=end {
            let prop = format!("AnalogInput= {}", channel);
            if !self.props.has_property(&prop) {
                self.props
                    .define_property(prop, PropertyValue::Float(0.0), true)?;
            }
            self.set_pull_up(channel, self.pull_up)?;
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if name == PROP_DIGITAL_INPUT {
            return Ok(PropertyValue::Integer(self.read_digital_input()?));
        }
        if let Some(channel) = Self::analog_property_channel(name) {
            if self.props.has_property(name) {
                return Ok(PropertyValue::Integer(self.read_analog_input(channel)?));
            }
        }
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        self.props.set(name, val)
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
        DeviceType::Generic
    }

    fn busy(&self) -> bool {
        false
    }
}

impl Generic for Esp32Input {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_all_pins_sets_pullups_and_defines_inputs() {
        let t = MockTransport::new();
        let mut input = Esp32Input::new().with_transport(Box::new(t));
        input.initialize().unwrap();
        assert!(input.has_property(PROP_DIGITAL_INPUT));
        assert!(input.has_property("AnalogInput= 0"));
        assert!(input.has_property("AnalogInput= 5"));
    }

    #[test]
    fn before_get_reads_digital_and_analog_values() {
        let t = MockTransport::new()
            .expect("L,6", "L,1")
            .expect("A,2", "A,597");
        let mut input = Esp32Input::new().with_transport(Box::new(t));
        input.initialize().unwrap();

        assert_eq!(
            input.get_property(PROP_DIGITAL_INPUT).unwrap(),
            PropertyValue::Integer(1)
        );
        assert_eq!(
            input.get_property("AnalogInput= 2").unwrap(),
            PropertyValue::Integer(597)
        );
    }
}
