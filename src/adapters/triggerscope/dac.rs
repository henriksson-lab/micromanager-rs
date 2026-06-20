/// TriggerScope DAC channel — analog output.
///
/// ASCII serial protocol, `\n` terminated.
///   Set DAC voltage: `"DAC<ch>,<value>\n"` → controller response
///   Get DAC voltage: `"DAC<ch>?\n"`         → `"DAC<ch> <value>\n"`
///
/// Voltage range: 0.0-10.0 V (12-bit or 16-bit DAC).
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, SignalIO};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

use super::hub::SharedTriggerScopeTransport;

pub struct TriggerScopeDAC {
    props: PropertyMap,
    transport: Option<SharedTriggerScopeTransport>,
    initialized: bool,
    channel: u8,
    name: String,
    voltage: f64,
    gate_open: bool,
    sequence: Vec<f64>,
    is_ts16: bool,
}

impl TriggerScopeDAC {
    pub fn new(channel: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Channel", PropertyValue::Integer(channel as i64), true)
            .unwrap();
        props
            .define_property("Volts", PropertyValue::Float(0.0), false)
            .unwrap();
        props.set_property_limits("Volts", 0.0, 10.0).unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("State", &["0", "1"]).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            channel,
            name: format!("TriggerScope-DAC{:02}", channel),
            voltage: 0.0,
            gate_open: true,
            sequence: Vec::new(),
            is_ts16: false,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(std::sync::Arc::new(std::sync::Mutex::new(t)));
        self
    }

    pub fn with_shared_transport(mut self, transport: SharedTriggerScopeTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_mut() {
            Some(t) => {
                let mut guard = t.lock().map_err(|_| {
                    MmError::LocallyDefined("TriggerScope transport lock poisoned".into())
                })?;
                f(guard.as_mut())
            }
            None => Err(MmError::NotConnected),
        }
    }

    fn send_recv(&mut self, cmd: &str) -> MmResult<String> {
        self.call_transport(|t| {
            t.purge()?;
            match t.send_recv(cmd) {
                Ok(resp) if !resp.trim().is_empty() => Ok(resp.trim().to_string()),
                _ => {
                    t.purge()?;
                    Ok(t.send_recv(cmd)?.trim().to_string())
                }
            }
        })
    }

    fn send_voltage(&mut self, volts: f64) -> MmResult<f64> {
        let ch = self.channel;
        let volts = volts.clamp(0.0, 10.0);
        let max_count = if self.is_ts16 { 65535.0 } else { 4095.0 };
        let counts = ((volts / 10.0) * max_count) as u32;
        let cmd = format!("DAC{},{}\n", ch, counts);
        let _ = self.send_recv(&cmd)?;
        Ok(volts)
    }

    pub fn set_ts16(&mut self, is_ts16: bool) {
        self.is_ts16 = is_ts16;
    }

    pub fn clear_da_sequence(&mut self) -> MmResult<()> {
        self.sequence.clear();
        Ok(())
    }

    pub fn add_to_da_sequence(&mut self, voltage: f64) -> MmResult<()> {
        self.sequence.push(voltage);
        Ok(())
    }

    pub fn send_da_sequence(&mut self) -> MmResult<()> {
        let ch = self.channel;
        let max_count = if self.is_ts16 { 65535.0 } else { 4095.0 };
        self.send_recv(&format!("CLEAR_DAC,{}\n", ch))?;
        let values: Vec<f64> = self.sequence.clone();
        for (idx, volts) in values.into_iter().enumerate() {
            let clamped = volts.clamp(0.0, 10.0);
            let counts = ((clamped / 10.0) * max_count) as u32;
            self.send_recv(&format!("PROG_DAC,{},{},{}\n", idx + 1, ch, counts))?;
        }
        Ok(())
    }

    pub fn start_da_sequence(&mut self) -> MmResult<()> {
        self.send_recv("ARM\n")?;
        Ok(())
    }

    pub fn stop_da_sequence(&mut self) -> MmResult<()> {
        Ok(())
    }

    #[allow(dead_code)]
    fn query_voltage(&mut self) -> MmResult<f64> {
        let ch = self.channel;
        let cmd = format!("DAC{:02}?\n", ch);
        let resp = self.send_recv(&cmd)?;
        // Response format: "DAC01 2048"
        let parts: Vec<&str> = resp.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(MmError::SerialInvalidResponse);
        }
        let counts: f64 = parts[1]
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        Ok((counts / 4095.0) * 10.0)
    }
}

impl Device for TriggerScopeDAC {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "ARC TriggerScope DAC channel"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
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
            "State" => Ok(PropertyValue::Integer(if self.gate_open { 1 } else { 0 })),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Volts" => {
                let v = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_signal(v)
            }
            "State" => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_gate_open(v == 1)
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

impl SignalIO for TriggerScopeDAC {
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
        if self.initialized {
            let volts = if open { self.voltage } else { 0.0 };
            self.send_voltage(volts)?;
        }
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(self.gate_open)
    }

    fn set_signal(&mut self, volts: f64) -> MmResult<()> {
        let volts = volts.clamp(0.0, 10.0);
        if self.gate_open {
            self.send_voltage(volts)?;
        }
        self.voltage = volts;
        Ok(())
    }

    fn get_signal(&self) -> MmResult<f64> {
        Ok(self.voltage)
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((0.0, 10.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn dac_initialize_zeroes_output() {
        let t = MockTransport::new();
        let mut dac = TriggerScopeDAC::new(1).with_transport(Box::new(t));
        dac.initialize().unwrap();
        assert!((dac.get_signal().unwrap() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn set_signal_mid_range() {
        let t = MockTransport::new().expect("DAC2,1023\n", "DAC2 OK"); // set ~2.5V
        let mut dac = TriggerScopeDAC::new(2).with_transport(Box::new(t));
        dac.initialize().unwrap();
        dac.set_signal(2.5).unwrap();
        assert!((dac.get_signal().unwrap() - 2.5).abs() < 0.01);
    }

    #[test]
    fn out_of_range_clamped() {
        let t = MockTransport::new()
            .expect("DAC1,4095\n", "DAC1 OK")
            .expect("DAC1,0\n", "DAC1 OK");
        let mut dac = TriggerScopeDAC::new(1).with_transport(Box::new(t));
        dac.initialize().unwrap();
        dac.set_signal(11.0).unwrap();
        assert_eq!(dac.get_signal().unwrap(), 10.0);
        dac.set_signal(-1.0).unwrap();
        assert_eq!(dac.get_signal().unwrap(), 0.0);
    }

    #[test]
    fn set_signal_accepts_non_ok_echo_response() {
        let t = MockTransport::new().expect("DAC1,2047\n", "DAC1,2047");
        let mut dac = TriggerScopeDAC::new(1).with_transport(Box::new(t));
        dac.initialize().unwrap();
        dac.set_signal(5.0).unwrap();
        assert!((dac.get_signal().unwrap() - 5.0).abs() < 0.01);
    }
}
