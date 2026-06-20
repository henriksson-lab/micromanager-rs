/// TriggerScope MM DAC — analog output channel.
///
/// Protocol: `"SAR<ch>-<range>\n"` to set range, `"SAO<ch>-<value>\n"` to set value.
/// Default voltage range 0-5 V (configurable in the C++ adapter). Answers end with `\r\n`.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, SignalIO};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

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
        props
            .define_property("Channel", PropertyValue::Integer(channel as i64), true)
            .unwrap();
        props
            .define_property(
                "Voltage Range",
                PropertyValue::String("0 - 5V".into()),
                false,
            )
            .unwrap();
        props
            .define_property("Volts", PropertyValue::Float(0.0), false)
            .unwrap();
        props.set_property_limits("Volts", 0.0, 5.0).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            channel,
            voltage: 0.0,
            min_v: 0.0,
            max_v: 5.0,
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

    fn send_voltage_cmd(&mut self, volts: f64) -> MmResult<f64> {
        let ch = self.channel;
        let min_v = self.min_v;
        let max_v = self.max_v;
        let volts = volts.clamp(min_v, max_v);
        let counts = (((volts - min_v) / (max_v - min_v)) * 4095.0) as u32;
        let expected = format!("SAO{}-{}", ch, counts);
        let cmd = format!("{}\n", expected);
        let resp = self.send_recv(&cmd)?;
        if resp != expected && resp != format!("!{}", expected) && !resp.contains("OK") {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(volts)
    }
}

impl Device for TriggerScopeMMDAC {
    fn name(&self) -> &str {
        "TriggerScopeMMDAC"
    }
    fn description(&self) -> &str {
        "ARC TriggerScope MM DAC channel"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Set range (range 1 = 0-5V)
        let ch = self.channel;
        let expected = format!("SAR{}-1", ch);
        let cmd = format!("{}\n", expected);
        let resp = self.send_recv(&cmd)?;
        if resp != expected && resp != format!("!{}", expected) && !resp.contains("OK") {
            return Err(MmError::SerialInvalidResponse);
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Volts" => Ok(PropertyValue::Float(self.voltage)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Volts" => {
                let v = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_signal(v)
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
        DeviceType::SignalIO
    }
    fn busy(&self) -> bool {
        false
    }
}

impl SignalIO for TriggerScopeMMDAC {
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(self.gate_open)
    }

    fn set_signal(&mut self, volts: f64) -> MmResult<()> {
        let volts = self.send_voltage_cmd(volts)?;
        self.voltage = volts;
        Ok(())
    }

    fn get_signal(&self) -> MmResult<f64> {
        Ok(self.voltage)
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((self.min_v, self.max_v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn dac_initialize() {
        let t = MockTransport::new().expect("SAR1-1\n", "SAR1 OK");
        let mut dac = TriggerScopeMMDAC::new(1).with_transport(Box::new(t));
        dac.initialize().unwrap();
        assert!((dac.get_signal().unwrap() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn dac_set_voltage() {
        let t = MockTransport::new()
            .expect("SAR2-1\n", "SAR2 OK")
            .expect("SAO2-4095\n", "!SAO2-4095");
        let mut dac = TriggerScopeMMDAC::new(2).with_transport(Box::new(t));
        dac.initialize().unwrap();
        dac.set_signal(5.0).unwrap();
        assert!((dac.get_signal().unwrap() - 5.0).abs() < 0.01);
    }

    #[test]
    fn dac_accepts_mm_echo_response() {
        let t = MockTransport::new()
            .expect("SAR3-1\n", "SAR3-1")
            .expect("SAO3-2047\n", "SAO3-2047");
        let mut dac = TriggerScopeMMDAC::new(3).with_transport(Box::new(t));
        dac.initialize().unwrap();
        dac.set_signal(2.5).unwrap();
        assert!((dac.get_signal().unwrap() - 2.5).abs() < 0.01);
    }

    #[test]
    fn dac_rejects_unrelated_write_response() {
        let t = MockTransport::new()
            .expect("SAR1-1\n", "SAR1 OK")
            .expect("SAO1-4095\n", "!SAO2-4095");
        let mut dac = TriggerScopeMMDAC::new(1).with_transport(Box::new(t));
        dac.initialize().unwrap();
        assert!(dac.set_signal(5.0).is_err());
    }

    #[test]
    fn dac_out_of_range_clamped_on_write() {
        let t = MockTransport::new()
            .expect("SAR1-1\n", "SAR1 OK")
            .expect("SAO1-4095\n", "!SAO1-4095")
            .expect("SAO1-0\n", "!SAO1-0");
        let mut dac = TriggerScopeMMDAC::new(1).with_transport(Box::new(t));
        dac.initialize().unwrap();
        dac.set_signal(6.0).unwrap();
        assert_eq!(dac.get_signal().unwrap(), 5.0);
        dac.set_signal(-1.0).unwrap();
        assert_eq!(dac.get_signal().unwrap(), 0.0);
    }
}
