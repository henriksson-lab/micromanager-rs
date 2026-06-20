//! Arduino32 input monitor generic device.

use parking_lot::Mutex;

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

const PROP_PIN: &str = "Pin";
const PROP_PULL_UP: &str = "Pull-Up-Resistor";
const PROP_DIGITAL_INPUT: &str = "DigitalInput";

pub struct Arduino32Input {
    props: PropertyMap,
    transport: Mutex<Option<Box<dyn Transport>>>,
    initialized: bool,
    pin: Option<u8>,
    pull_up: bool,
}

impl Arduino32Input {
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

    fn receive_n(t: &mut dyn Transport, n: usize) -> MmResult<Vec<u8>> {
        let raw = t.receive_bytes(n)?;
        if raw.len() < n {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(raw)
    }

    fn read_digital_input(&self) -> MmResult<i64> {
        let pin = self.pin;
        self.with_transport_ref(|t| {
            t.send_bytes(&[40])?;
            let answer = Self::receive_n(t, 2)?;
            if answer[0] != 40 {
                return Err(MmError::SerialInvalidResponse);
            }
            let mut state = answer[1];
            if let Some(pin) = pin {
                state >>= pin;
                state &= state & 1;
            }
            Ok(state as i64)
        })
    }

    fn read_analog_input(&self, channel: u8) -> MmResult<i64> {
        self.with_transport_ref(|t| {
            t.send_bytes(&[41, channel])?;
            let answer = Self::receive_n(t, 4)?;
            if answer[0] != 41 || answer[1] != channel {
                return Err(MmError::SerialInvalidResponse);
            }
            Ok((((answer[2] as u16) << 8) | answer[3] as u16) as i64)
        })
    }

    fn set_pull_up(&self, pin: u8, state: bool) -> MmResult<()> {
        self.with_transport_ref(|t| {
            t.send_bytes(&[42, pin, u8::from(state)])?;
            let answer = Self::receive_n(t, 3)?;
            if answer[0] != 42 || answer[1] != pin {
                return Err(MmError::SerialInvalidResponse);
            }
            Ok(())
        })
    }

    fn analog_property_channel(name: &str) -> Option<u8> {
        name.strip_prefix("AnalogInput")?.parse().ok()
    }
}

impl Default for Arduino32Input {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Arduino32Input {
    fn name(&self) -> &str {
        "Arduino32-Input"
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
            let prop = format!("AnalogInput{}", channel);
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

impl Generic for Arduino32Input {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_all_pins_sets_pullups_and_defines_inputs() {
        let mut t = MockTransport::new();
        for pin in 0..=5 {
            t = t.expect_binary(&[42, pin, 1]);
        }
        let mut input = Arduino32Input::new().with_transport(Box::new(t));
        input.initialize().unwrap();
        assert!(input.has_property(PROP_DIGITAL_INPUT));
        assert!(input.has_property("AnalogInput0"));
        assert!(input.has_property("AnalogInput5"));
    }

    #[test]
    fn before_get_reads_digital_and_analog_values() {
        let mut t = MockTransport::new();
        for pin in 0..=5 {
            t = t.expect_binary(&[42, pin, 1]);
        }
        t = t.expect_binary(&[40, 0b0010_0101]);
        t = t.expect_binary(&[41, 2, 0x01, 0x23]);
        let mut input = Arduino32Input::new().with_transport(Box::new(t));
        input.initialize().unwrap();

        assert_eq!(
            input.get_property(PROP_DIGITAL_INPUT).unwrap(),
            PropertyValue::Integer(0b0010_0101)
        );
        assert_eq!(
            input.get_property("AnalogInput2").unwrap(),
            PropertyValue::Integer(0x0123)
        );
    }
}
