/// TriggerScope MM DAC — analog output channel.
///
/// Protocol: `"SAR<ch>-<range>\n"` to set range, `"SAO<ch>-<value>\n"` to set value.
/// Default voltage range 0-5 V (configurable in the C++ adapter). Answers end with `\r\n`.
use super::hub::SharedTriggerScopeMMTransport;
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, SignalIO};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::sync::{Arc, Mutex};

pub struct TriggerScopeMMDAC {
    props: PropertyMap,
    transport: Option<SharedTriggerScopeMMTransport>,
    initialized: bool,
    channel: u8,
    name: String,
    voltage: f64,
    min_v: f64,
    max_v: f64,
    gate_open: bool,
    is_ts16: bool,
    range_code: u8,
    sequence_on: bool,
    sequence_rising: bool,
    blanking: bool,
    blank_on_low: bool,
    nr_events: usize,
    sequence: Vec<f64>,
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
        props
            .set_allowed_values(
                "Voltage Range",
                &["0 - 5V", "0 - 10V", "-5 - 5V", "-10 - 10V", "-2 - 2V"],
            )
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(1), false)
            .unwrap();
        props.set_allowed_values("State", &["0", "1"]).unwrap();
        props
            .define_property("Sequence", PropertyValue::String("On".into()), false)
            .unwrap();
        props
            .set_allowed_values("Sequence", &["On", "Off"])
            .unwrap();
        props
            .define_property(
                "Sequence Trigger Edge",
                PropertyValue::String("Rising".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("Sequence Trigger Edge", &["Falling", "Rising"])
            .unwrap();
        props
            .define_property("Blanking", PropertyValue::String("Off".into()), false)
            .unwrap();
        props
            .set_allowed_values("Blanking", &["Off", "On"])
            .unwrap();
        props
            .define_property("Blank On", PropertyValue::String("Low".into()), false)
            .unwrap();
        props
            .set_allowed_values("Blank On", &["Low", "High"])
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            channel,
            name: format!("TS_DAC{:02}", channel),
            voltage: 0.0,
            min_v: 0.0,
            max_v: 5.0,
            gate_open: true,
            is_ts16: false,
            range_code: 1,
            sequence_on: true,
            sequence_rising: true,
            blanking: false,
            blank_on_low: true,
            nr_events: 50,
            sequence: Vec::new(),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(Arc::new(Mutex::new(t)));
        self
    }

    pub fn with_shared_transport(mut self, transport: SharedTriggerScopeMMTransport) -> Self {
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
                    MmError::LocallyDefined("TriggerScope MM transport lock poisoned".into())
                })?;
                f(guard.as_mut())
            }
            None => Err(MmError::NotConnected),
        }
    }

    fn send_recv(&mut self, cmd: &str) -> MmResult<String> {
        self.call_transport(|t| {
            t.purge()?;
            Ok(t.send_recv(cmd)?.trim().to_string())
        })
    }

    fn validate_command_response(cmd: &str, resp: &str) -> MmResult<()> {
        let cmd = cmd.trim();
        let resp = resp.trim();
        if resp == cmd || resp == format!("!{}", cmd) || resp.contains("OK") {
            return Ok(());
        }
        if resp.starts_with(cmd) {
            return Ok(());
        }
        if resp
            .strip_prefix('!')
            .is_some_and(|unprefixed| unprefixed.starts_with(cmd))
        {
            return Ok(());
        }
        Err(MmError::SerialInvalidResponse)
    }

    fn send_voltage_cmd(&mut self, volts: f64) -> MmResult<f64> {
        let ch = self.channel;
        let min_v = self.min_v;
        let max_v = self.max_v;
        let volts = volts.clamp(min_v, max_v);
        let max_count = if self.is_ts16 { 65535.0 } else { 4095.0 };
        let counts = (((volts - min_v) / (max_v - min_v)) * max_count) as u32;
        let expected = format!("SAO{}-{}", ch, counts);
        let cmd = format!("{}\n", expected);
        let resp = self.send_recv(&cmd)?;
        Self::validate_command_response(&expected, &resp)?;
        Ok(volts)
    }

    pub fn set_ts16(&mut self, is_ts16: bool) {
        self.is_ts16 = is_ts16;
    }

    fn apply_range(&mut self, label: &str) -> MmResult<()> {
        let (code, min_v, max_v) = match label {
            "0 - 5V" => (1, 0.0, 5.0),
            "0 - 10V" => (2, 0.0, 10.0),
            "-5 - 5V" => (3, -5.0, 5.0),
            "-10 - 10V" => (4, -10.0, 10.0),
            "-2 - 2V" => (5, -2.0, 2.0),
            _ => return Err(MmError::InvalidPropertyValue),
        };
        self.range_code = code;
        self.min_v = min_v;
        self.max_v = max_v;
        if let Some(entry) = self.props.entry_mut("Volts") {
            entry.lower_limit = min_v;
            entry.upper_limit = max_v;
        }
        Ok(())
    }

    fn send_blanking_command(&mut self) -> MmResult<()> {
        let cmd = format!(
            "BAO{}-{}-{}",
            self.channel,
            if self.blanking { 1 } else { 0 },
            if self.blank_on_low { 0 } else { 1 }
        );
        let resp = self.send_recv(&format!("{}\n", cmd))?;
        Self::validate_command_response(&cmd, &resp)
    }

    pub fn clear_da_sequence(&mut self) -> MmResult<()> {
        self.sequence.clear();
        let cmd = format!("PAC{}", self.channel);
        let resp = self.send_recv(&format!("{}\n", cmd))?;
        Self::validate_command_response(&cmd, &resp)
    }

    pub fn add_to_da_sequence(&mut self, voltage: f64) -> MmResult<()> {
        if self.sequence.len() >= self.nr_events {
            return Err(MmError::SequenceTooLarge);
        }
        self.sequence.push(voltage);
        Ok(())
    }

    pub fn send_da_sequence(&mut self) -> MmResult<()> {
        let clear = format!("PAC{}", self.channel);
        let resp = self.send_recv(&format!("{}\n", clear))?;
        Self::validate_command_response(&clear, &resp)?;

        let max_count = if self.is_ts16 { 65535.0 } else { 4095.0 };
        let mut cmd = format!("PAO{}-0", self.channel);
        for volts in &self.sequence {
            let volts = volts.clamp(self.min_v, self.max_v);
            let counts = (((volts - self.min_v) / (self.max_v - self.min_v)) * max_count) as u32;
            cmd.push_str(&format!("-{}", counts));
        }
        let resp = self.send_recv(&format!("{}\n", cmd))?;
        Self::validate_command_response(&cmd, &resp)
    }

    pub fn start_da_sequence(&mut self) -> MmResult<()> {
        let cmd = format!(
            "PAS{}-1-{}",
            self.channel,
            if self.sequence_rising { 1 } else { 0 }
        );
        let resp = self.send_recv(&format!("{}\n", cmd))?;
        Self::validate_command_response(&cmd, &resp)
    }

    pub fn stop_da_sequence(&mut self) -> MmResult<()> {
        let cmd = format!(
            "PAS{}-0-{}",
            self.channel,
            if self.sequence_rising { 1 } else { 0 }
        );
        let resp = self.send_recv(&format!("{}\n", cmd))?;
        Self::validate_command_response(&cmd, &resp)
    }
}

impl Device for TriggerScopeMMDAC {
    fn name(&self) -> &str {
        &self.name
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
        Self::validate_command_response(&expected, &resp)?;
        let pan = format!("PAN{}", ch);
        let resp = self.send_recv(&format!("{}\n", pan))?;
        Self::validate_command_response(&pan, &resp)?;
        if let Some(value) = resp.split('-').nth(1).and_then(|v| v.parse::<usize>().ok()) {
            self.nr_events = value;
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
            "Sequence" => Ok(PropertyValue::String(
                if self.sequence_on { "On" } else { "Off" }.into(),
            )),
            "Sequence Trigger Edge" => Ok(PropertyValue::String(
                if self.sequence_rising {
                    "Rising"
                } else {
                    "Falling"
                }
                .into(),
            )),
            "Blanking" => Ok(PropertyValue::String(
                if self.blanking { "On" } else { "Off" }.into(),
            )),
            "Blank On" => Ok(PropertyValue::String(
                if self.blank_on_low { "Low" } else { "High" }.into(),
            )),
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
            "Voltage Range" => {
                let label = val.to_string();
                self.apply_range(&label)?;
                self.props.set(name, PropertyValue::String(label))
            }
            "Sequence" => {
                let label = val.to_string();
                self.sequence_on = match label.as_str() {
                    "On" => true,
                    "Off" => false,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.props.set(name, PropertyValue::String(label))
            }
            "Sequence Trigger Edge" => {
                let label = val.to_string();
                self.sequence_rising = match label.as_str() {
                    "Rising" => true,
                    "Falling" => false,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.props.set(name, PropertyValue::String(label))
            }
            "Blanking" => {
                let label = val.to_string();
                self.blanking = match label.as_str() {
                    "On" => true,
                    "Off" => false,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.send_blanking_command()?;
                self.props.set(name, PropertyValue::String(label))
            }
            "Blank On" => {
                let label = val.to_string();
                self.blank_on_low = match label.as_str() {
                    "Low" => true,
                    "High" => false,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.send_blanking_command()?;
                self.props.set(name, PropertyValue::String(label))
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
        let volts = if open { self.voltage } else { 0.0 };
        self.send_voltage_cmd(volts)?;
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(self.gate_open)
    }

    fn set_signal(&mut self, volts: f64) -> MmResult<()> {
        let volts = volts.clamp(self.min_v, self.max_v);
        if self.gate_open {
            self.send_voltage_cmd(volts)?;
        }
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
        let t = MockTransport::new()
            .expect("SAR1-1\n", "SAR1 OK")
            .expect("PAN1\n", "PAN1-560");
        let mut dac = TriggerScopeMMDAC::new(1).with_transport(Box::new(t));
        dac.initialize().unwrap();
        assert!((dac.get_signal().unwrap() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn dac_set_voltage() {
        let t = MockTransport::new()
            .expect("SAR2-1\n", "SAR2 OK")
            .expect("PAN2\n", "PAN2-560")
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
            .expect("PAN3\n", "PAN3-560")
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
            .expect("PAN1\n", "PAN1-560")
            .expect("SAO1-4095\n", "!SAO2-4095");
        let mut dac = TriggerScopeMMDAC::new(1).with_transport(Box::new(t));
        dac.initialize().unwrap();
        assert!(dac.set_signal(5.0).is_err());
    }

    #[test]
    fn dac_out_of_range_clamped_on_write() {
        let t = MockTransport::new()
            .expect("SAR1-1\n", "SAR1 OK")
            .expect("PAN1\n", "PAN1-560")
            .expect("SAO1-4095\n", "!SAO1-4095")
            .expect("SAO1-0\n", "!SAO1-0");
        let mut dac = TriggerScopeMMDAC::new(1).with_transport(Box::new(t));
        dac.initialize().unwrap();
        dac.set_signal(6.0).unwrap();
        assert_eq!(dac.get_signal().unwrap(), 5.0);
        dac.set_signal(-1.0).unwrap();
        assert_eq!(dac.get_signal().unwrap(), 0.0);
    }

    #[test]
    fn dac_gate_blanking_and_sequence_commands() {
        let t = MockTransport::new()
            .expect("SAR1-1\n", "!SAR1-1")
            .expect("PAN1\n", "!PAN1-3")
            .expect("SAO1-2047\n", "!SAO1-2047")
            .expect("SAO1-0\n", "!SAO1-0")
            .expect("SAO1-2047\n", "!SAO1-2047")
            .expect("BAO1-1-0\n", "!BAO1-1-0")
            .expect("PAC1\n", "!PAC1")
            .expect("PAO1-0-0-4095\n", "!PAO1-0-0-4095")
            .expect("PAS1-1-1\n", "!PAS1-1-1")
            .expect("PAS1-0-1\n", "!PAS1-0-1");
        let mut dac = TriggerScopeMMDAC::new(1).with_transport(Box::new(t));
        dac.initialize().unwrap();
        dac.set_signal(2.5).unwrap();
        dac.set_gate_open(false).unwrap();
        assert_eq!(dac.get_signal().unwrap(), 2.5);
        dac.set_gate_open(true).unwrap();
        dac.set_property("Blanking", PropertyValue::String("On".into()))
            .unwrap();
        dac.add_to_da_sequence(0.0).unwrap();
        dac.add_to_da_sequence(5.0).unwrap();
        dac.send_da_sequence().unwrap();
        dac.start_da_sequence().unwrap();
        dac.stop_da_sequence().unwrap();
    }
}
