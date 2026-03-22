//! ESP32Pwm — SignalIO device using ASCII command `O,<channel>,<value>`.
//! Value range 0.0–100.0 (percent duty cycle, or arbitrary float for laser power).

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, SignalIO};
use micromanager::types::{DeviceType, PropertyValue};

pub type PwmWriter = std::sync::Arc<dyn Fn(u8, f64) -> MmResult<()> + Send + Sync>;

pub struct Esp32Pwm {
    props: PropertyMap,
    initialized: bool,
    channel: u8,
    signal: f64,
    gate_open: bool,
    gated_signal: f64,
    writer: Option<PwmWriter>,
}

impl Esp32Pwm {
    pub fn new(channel: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Signal", PropertyValue::Float(0.0), false).unwrap();
        props.define_property("Channel", PropertyValue::Integer(channel as i64), true).unwrap();
        Self {
            props,
            initialized: false,
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
    fn name(&self) -> &str { "ESP32-PWM" }
    fn description(&self) -> &str { "ESP32 PWM channel" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.writer.is_none() { return Err(MmError::CommHubMissing); }
        self.write_signal(0.0)?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized { let _ = self.write_signal(0.0); self.initialized = false; }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if name == "Signal" { return Ok(PropertyValue::Float(self.signal)); }
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Signal" {
            let v = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            if self.initialized && self.gate_open { self.write_signal(v)?; }
            self.signal = v;
            self.gated_signal = v;
            return Ok(());
        }
        self.props.set(name, val)
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::SignalIO }
    fn busy(&self) -> bool { false }
}

impl SignalIO for Esp32Pwm {
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
        if open { self.write_signal(self.gated_signal)?; } else { self.write_signal(0.0)?; }
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> { Ok(self.gate_open) }

    fn set_signal(&mut self, val: f64) -> MmResult<()> {
        if self.gate_open && self.initialized { self.write_signal(val)?; }
        self.signal = val;
        self.gated_signal = val;
        Ok(())
    }

    fn get_signal(&self) -> MmResult<f64> { Ok(self.signal) }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((0.0, 100.0)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn set_signal_recorded() {
        let log: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        let writer: PwmWriter = Arc::new(move |_ch, v| { log2.lock().unwrap().push(v); Ok(()) });
        let mut pwm = Esp32Pwm::new(0).with_writer(writer);
        pwm.initialize().unwrap();
        pwm.set_signal(75.0).unwrap();
        assert_eq!(log.lock().unwrap().last().copied().unwrap(), 75.0);
    }
}
