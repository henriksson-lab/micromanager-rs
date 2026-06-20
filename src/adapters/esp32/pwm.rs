//! ESP32Pwm — SignalIO device using ASCII command `O,<channel>,<value>`.
//! Value range 0.0–100.0 (percent duty cycle, or arbitrary float for laser power).

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, SignalIO};
use crate::types::{DeviceType, PropertyValue};

pub type PwmWriter = std::sync::Arc<dyn Fn(u8, f64) -> MmResult<()> + Send + Sync>;

pub struct Esp32Pwm {
    props: PropertyMap,
    initialized: bool,
    name: String,
    channel: u8,
    signal: f64,
    gate_open: bool,
    gated_signal: f64,
    writer: Option<PwmWriter>,
}

impl Esp32Pwm {
    pub fn new(channel: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Power %", PropertyValue::Float(100.0), false)
            .unwrap();
        props
            .define_property("Volts", PropertyValue::Float(0.0), false)
            .unwrap();
        Self {
            props,
            initialized: false,
            name: format!("ESP32-PWM{}", channel),
            channel,
            signal: 0.0,
            gate_open: true,
            gated_signal: 0.0,
            writer: None,
        }
    }

    pub fn with_writer(mut self, writer: PwmWriter) -> Self {
        self.writer = Some(writer);
        self
    }

    fn write_signal(&self, val: f64) -> MmResult<()> {
        let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
        writer(self.channel, val)
    }
}

impl Device for Esp32Pwm {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "ESP32 PWM channel"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.writer.is_none() {
            return Err(MmError::CommHubMissing);
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if name == "Volts" {
            return Ok(PropertyValue::Float(self.signal));
        }
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Volts" {
            let v = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            if self.initialized && self.gate_open {
                self.write_signal(v)?;
            }
            self.signal = v;
            self.gated_signal = v;
            self.props.set(name, PropertyValue::Float(v))?;
            return Ok(());
        }
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
        DeviceType::SignalIO
    }
    fn busy(&self) -> bool {
        false
    }
}

impl SignalIO for Esp32Pwm {
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
        if open {
            self.gated_signal = self.signal;
            self.write_signal(self.signal)?;
        } else {
            self.gated_signal = 0.0;
            self.write_signal(0.0)?;
        }
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(self.gate_open)
    }

    fn set_signal(&mut self, val: f64) -> MmResult<()> {
        if self.gate_open && self.initialized {
            self.write_signal(val)?;
        }
        self.signal = val;
        self.gated_signal = val;
        Ok(())
    }

    fn get_signal(&self) -> MmResult<f64> {
        Err(MmError::UnsupportedCommand)
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        let max = self
            .props
            .get("Power %")?
            .as_f64()
            .ok_or(MmError::InvalidPropertyValue)?;
        Ok((0.0, max))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn set_signal_recorded() {
        let log: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        let writer: PwmWriter = Arc::new(move |_ch, v| {
            log2.lock().unwrap().push(v);
            Ok(())
        });
        let mut pwm = Esp32Pwm::new(0).with_writer(writer);
        pwm.initialize().unwrap();
        pwm.set_signal(75.0).unwrap();
        assert_eq!(log.lock().unwrap().last().copied().unwrap(), 75.0);
    }

    #[test]
    fn upstream_properties_name_and_get_signal() {
        let pwm = Esp32Pwm::new(3);
        assert_eq!(pwm.name(), "ESP32-PWM3");
        assert!(pwm.has_property("Power %"));
        assert!(pwm.has_property("Volts"));
        assert!(!pwm.has_property("Signal"));
        assert_eq!(pwm.get_signal().unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn power_property_controls_reported_limits() {
        let mut pwm = Esp32Pwm::new(0);
        assert_eq!(pwm.get_limits().unwrap(), (0.0, 100.0));
        pwm.set_property("Power %", PropertyValue::Float(42.5))
            .unwrap();
        assert_eq!(pwm.get_limits().unwrap(), (0.0, 42.5));
    }

    #[test]
    fn initialize_and_shutdown_do_not_zero_output() {
        let log: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        let writer: PwmWriter = Arc::new(move |_ch, v| {
            log2.lock().unwrap().push(v);
            Ok(())
        });
        let mut pwm = Esp32Pwm::new(0).with_writer(writer);
        pwm.initialize().unwrap();
        pwm.shutdown().unwrap();
        assert!(log.lock().unwrap().is_empty());
    }

    #[test]
    fn gate_reopens_to_last_signal_like_upstream() {
        let log: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        let writer: PwmWriter = Arc::new(move |_ch, v| {
            log2.lock().unwrap().push(v);
            Ok(())
        });
        let mut pwm = Esp32Pwm::new(0).with_writer(writer);
        pwm.initialize().unwrap();
        pwm.set_signal(33.0).unwrap();
        pwm.set_gate_open(false).unwrap();
        pwm.set_gate_open(true).unwrap();
        assert_eq!(&*log.lock().unwrap(), &[33.0, 0.0, 33.0]);
    }
}
