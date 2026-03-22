/// TriggerScope MM DAC — analog output channel.
///
/// Protocol: `"SAR<ch>-<range>\n"` to set range, `"DAC<ch>-<value>\n"` to set value.
/// Voltage range 0–10 V (configurable). Answers end with `\r\n`.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, SignalIO};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct TriggerScopeMMDAC {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    channel: u8,
    voltage: f64,
    min_v: f64,
    max_v: f64,
    gate_open: bool,
}

impl TriggerScopeMMDAC {
    pub fn new(channel: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Channel", PropertyValue::Integer(channel as i64), true).unwrap();
        props.define_property("VoltageRange", PropertyValue::String("0 - 10".into()), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            channel,
            voltage: 0.0,
            min_v: 0.0,
            max_v: 10.0,
            gate_open: true,
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

    fn send_recv(&mut self, cmd: &str) -> MmResult<String> {
        self.call_transport(|t| Ok(t.send_recv(cmd)?.trim().to_string()))
    }

    fn send_voltage_cmd(&mut self, volts: f64) -> MmResult<()> {
        let ch = self.channel;
        let max_v = self.max_v;
        // 16-bit count for 0..max_v range
        let counts = ((volts / max_v) * 65535.0).round() as u32;
        let cmd = format!("DAC{:02}-{}\n", ch, counts);
        let resp = self.send_recv(&cmd)?;
        if !resp.contains("OK") && !resp.contains("DAC") {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(())
    }
}

impl Device for TriggerScopeMMDAC {
    fn name(&self) -> &str { "TriggerScopeMMDAC" }
    fn description(&self) -> &str { "ARC TriggerScope MM DAC channel" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Set range (range 1 = 0-10V)
        let ch = self.channel;
        let cmd = format!("SAR{:02}-1\n", ch);
        let _resp = self.send_recv(&cmd)?;
        // Zero output
        self.send_voltage_cmd(0.0)?;
        self.voltage = 0.0;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send_voltage_cmd(0.0);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Voltage" => Ok(PropertyValue::Float(self.voltage)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Voltage" => {
                let v = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_signal(v)
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::SignalIO }
    fn busy(&self) -> bool { false }
}

impl SignalIO for TriggerScopeMMDAC {
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> { Ok(self.gate_open) }

    fn set_signal(&mut self, volts: f64) -> MmResult<()> {
        if volts < self.min_v || volts > self.max_v {
            return Err(MmError::InvalidPropertyValue);
        }
        self.send_voltage_cmd(volts)?;
        self.voltage = volts;
        Ok(())
    }

    fn get_signal(&self) -> MmResult<f64> { Ok(self.voltage) }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((self.min_v, self.max_v)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn dac_initialize() {
        let t = MockTransport::new()
            .expect("SAR01-1\n", "SAR01 OK")
            .expect("DAC01-0\n", "DAC01 OK");
        let mut dac = TriggerScopeMMDAC::new(1).with_transport(Box::new(t));
        dac.initialize().unwrap();
        assert!((dac.get_signal().unwrap() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn dac_set_voltage() {
        let t = MockTransport::new()
            .expect("SAR02-1\n", "SAR02 OK")
            .expect("DAC02-0\n", "DAC02 OK")
            .expect("DAC02-32768\n", "DAC02 OK");
        let mut dac = TriggerScopeMMDAC::new(2).with_transport(Box::new(t));
        dac.initialize().unwrap();
        dac.set_signal(5.0).unwrap();
        assert!((dac.get_signal().unwrap() - 5.0).abs() < 0.01);
    }

    #[test]
    fn dac_out_of_range() {
        let t = MockTransport::new()
            .expect("SAR01-1\n", "SAR01 OK")
            .expect("DAC01-0\n", "DAC01 OK");
        let mut dac = TriggerScopeMMDAC::new(1).with_transport(Box::new(t));
        dac.initialize().unwrap();
        assert!(dac.set_signal(11.0).is_err());
    }
}
