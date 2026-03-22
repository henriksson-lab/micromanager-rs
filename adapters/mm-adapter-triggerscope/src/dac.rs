/// TriggerScope DAC channel — analog output.
///
/// ASCII serial protocol, `\n` terminated.
///   Set DAC voltage: `"DAC<ch> <value>\n"` → `"DAC<ch> OK\n"`
///   Get DAC voltage: `"DAC<ch>?\n"`         → `"DAC<ch> <value>\n"`
///
/// Voltage range: 0.0 – 5.0 V (12-bit or 16-bit DAC).
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, SignalIO};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct TriggerScopeDAC {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    channel: u8,
    voltage: f64,
    gate_open: bool,
}

impl TriggerScopeDAC {
    pub fn new(channel: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Channel", PropertyValue::Integer(channel as i64), true).unwrap();
        props.define_property("Voltage", PropertyValue::Float(0.0), false).unwrap();
        props.set_property_limits("Voltage", 0.0, 5.0).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            channel,
            voltage: 0.0,
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

    fn send_voltage(&mut self, volts: f64) -> MmResult<()> {
        let ch = self.channel;
        // Convert voltage to 12-bit count (0-4095 for 0-5V)
        let counts = ((volts / 5.0) * 4095.0).round() as u32;
        let cmd = format!("DAC{:02} {}\n", ch, counts);
        let resp = self.send_recv(&cmd)?;
        if !resp.contains("OK") {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(())
    }

    fn query_voltage(&mut self) -> MmResult<f64> {
        let ch = self.channel;
        let cmd = format!("DAC{:02}?\n", ch);
        let resp = self.send_recv(&cmd)?;
        // Response format: "DAC01 2048"
        let parts: Vec<&str> = resp.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(MmError::SerialInvalidResponse);
        }
        let counts: f64 = parts[1].parse().map_err(|_| MmError::SerialInvalidResponse)?;
        Ok((counts / 4095.0) * 5.0)
    }
}

impl Device for TriggerScopeDAC {
    fn name(&self) -> &str { "TriggerScopeDAC" }
    fn description(&self) -> &str { "ARC TriggerScope DAC channel" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Set voltage to 0 on init
        self.send_voltage(0.0)?;
        self.voltage = 0.0;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send_voltage(0.0);
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

impl SignalIO for TriggerScopeDAC {
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> { Ok(self.gate_open) }

    fn set_signal(&mut self, volts: f64) -> MmResult<()> {
        if volts < 0.0 || volts > 5.0 {
            return Err(MmError::InvalidPropertyValue);
        }
        self.send_voltage(volts)?;
        self.voltage = volts;
        Ok(())
    }

    fn get_signal(&self) -> MmResult<f64> { Ok(self.voltage) }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((0.0, 5.0)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn dac_initialize_zeroes_output() {
        let t = MockTransport::new()
            .expect("DAC01 0\n", "DAC01 OK");
        let mut dac = TriggerScopeDAC::new(1).with_transport(Box::new(t));
        dac.initialize().unwrap();
        assert!((dac.get_signal().unwrap() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn set_signal_mid_range() {
        let t = MockTransport::new()
            .expect("DAC02 0\n", "DAC02 OK")    // init
            .expect("DAC02 2048\n", "DAC02 OK"); // set ~2.5V
        let mut dac = TriggerScopeDAC::new(2).with_transport(Box::new(t));
        dac.initialize().unwrap();
        dac.set_signal(2.5).unwrap();
        assert!((dac.get_signal().unwrap() - 2.5).abs() < 0.01);
    }

    #[test]
    fn out_of_range_rejected() {
        let t = MockTransport::new()
            .expect("DAC01 0\n", "DAC01 OK");
        let mut dac = TriggerScopeDAC::new(1).with_transport(Box::new(t));
        dac.initialize().unwrap();
        assert!(dac.set_signal(6.0).is_err());
        assert!(dac.set_signal(-1.0).is_err());
    }
}
