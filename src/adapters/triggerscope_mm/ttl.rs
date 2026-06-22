/// TriggerScope MM TTL — 8-channel digital output bank.
///
/// Protocol: `"PDN<group>\n"` to query number of patterns.
///           `"SDO<group>-<byte_value>\n"` to set TTL state byte.
///
/// Group 0 = TTL channels 1-8, Group 1 = channels 9-16.
/// The byte value sets all 8 lines simultaneously (0-255).
use super::hub::SharedTriggerScopeMMTransport;
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::sync::{Arc, Mutex};

pub struct TriggerScopeMMTTL {
    props: PropertyMap,
    transport: Option<SharedTriggerScopeMMTransport>,
    initialized: bool,
    pin_group: u8,
    name: String,
    state_byte: u8,
    cur_pos: u8,
    gate_open: bool,
    is_closed: bool,
    sequence_on: bool,
    sequence_rising: bool,
    blanking: bool,
    blank_on_low: bool,
    num_patterns: usize,
    sequence: Vec<u8>,
}

impl TriggerScopeMMTTL {
    /// `pin_group`: 0 for TTL1-8, 1 for TTL9-16.
    pub fn new(pin_group: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("PinGroup", PropertyValue::Integer(pin_group as i64), true)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            pin_group,
            name: if pin_group == 0 {
                "TS_TTL1-8".to_string()
            } else {
                "TS_TTL9-16".to_string()
            },
            state_byte: 0,
            cur_pos: 0,
            gate_open: true,
            is_closed: true,
            sequence_on: true,
            sequence_rising: true,
            blanking: false,
            blank_on_low: true,
            num_patterns: 50,
            sequence: Vec::new(),
        }
    }

    fn define_runtime_properties(&mut self) -> MmResult<()> {
        if self.props.has_property("State") {
            return Ok(());
        }

        self.props
            .define_property("Sequence", PropertyValue::String("On".into()), false)?;
        self.props.set_allowed_values("Sequence", &["On", "Off"])?;
        self.props.define_property(
            "Sequence Trigger Edge",
            PropertyValue::String("Rising".into()),
            false,
        )?;
        self.props
            .set_allowed_values("Sequence Trigger Edge", &["Falling", "Rising"])?;
        self.props
            .define_property("Blanking", PropertyValue::String("Off".into()), false)?;
        self.props.set_allowed_values("Blanking", &["Off", "On"])?;
        self.props
            .define_property("Blank On", PropertyValue::String("Low".into()), false)?;
        self.props
            .set_allowed_values("Blank On", &["Low", "High"])?;
        self.props
            .define_property("State", PropertyValue::Integer(0), false)?;
        self.props.set_property_limits("State", 0.0, 255.0)?;

        for ttl_nr in 1..=8 {
            let pin_nr = ttl_nr + (self.pin_group as usize * 8);
            let name = if pin_nr == 9 {
                "TTL-09".to_string()
            } else {
                format!("TTL-{}", pin_nr)
            };
            self.props
                .define_property(name.clone(), PropertyValue::Integer(0), false)?;
            self.props.set_property_limits(&name, 0.0, 1.0)?;
        }

        Ok(())
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

    fn send_state(&mut self, val: u8) -> MmResult<()> {
        let grp = self.pin_group;
        let expected = format!("SDO{}-{}", grp, val);
        let cmd = format!("{}\n", expected);
        let resp = self.send_recv(&cmd)?;
        Self::validate_command_response(&expected, &resp)
    }

    fn send_blanking_command(&mut self) -> MmResult<()> {
        let cmd = format!(
            "BDO{}-{}-{}",
            self.pin_group,
            if self.blanking { 1 } else { 0 },
            if self.blank_on_low { 0 } else { 1 }
        );
        let resp = self.send_recv(&format!("{}\n", cmd))?;
        Self::validate_command_response(&cmd, &resp)
    }

    pub fn clear_sequence(&mut self) -> MmResult<()> {
        self.sequence.clear();
        let cmd = format!("PDC{}", self.pin_group);
        let resp = self.send_recv(&format!("{}\n", cmd))?;
        Self::validate_command_response(&cmd, &resp)
    }

    pub fn add_to_sequence(&mut self, value: u8) -> MmResult<()> {
        if self.sequence.len() >= self.num_patterns {
            return Err(MmError::SequenceTooLarge);
        }
        self.sequence.push(value);
        Ok(())
    }

    pub fn send_sequence(&mut self) -> MmResult<()> {
        let clear = format!("PDC{}", self.pin_group);
        let resp = self.send_recv(&format!("{}\n", clear))?;
        Self::validate_command_response(&clear, &resp)?;
        let mut cmd = format!("PDO{}-0", self.pin_group);
        for value in &self.sequence {
            cmd.push_str(&format!("-{}", value));
        }
        let resp = self.send_recv(&format!("{}\n", cmd))?;
        Self::validate_command_response(&cmd, &resp)
    }

    pub fn start_sequence(&mut self) -> MmResult<()> {
        let cmd = format!(
            "PDS{}-1-{}",
            self.pin_group,
            if self.sequence_rising { 1 } else { 0 }
        );
        let resp = self.send_recv(&format!("{}\n", cmd))?;
        Self::validate_command_response(&cmd, &resp)
    }

    pub fn stop_sequence(&mut self) -> MmResult<()> {
        let cmd = format!(
            "PDS{}-0-{}",
            self.pin_group,
            if self.sequence_rising { 1 } else { 0 }
        );
        let resp = self.send_recv(&format!("{}\n", cmd))?;
        Self::validate_command_response(&cmd, &resp)
    }
}

impl Device for TriggerScopeMMTTL {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "ARC TriggerScope MM TTL bank"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Query number of patterns for this group
        let grp = self.pin_group;
        let expected = format!("PDN{}", grp);
        let cmd = format!("{}\n", expected);
        let resp = self.send_recv(&cmd)?;
        Self::validate_command_response(&expected, &resp)?;
        if let Some(value) = resp.split('-').nth(1).and_then(|v| v.parse::<usize>().ok()) {
            self.num_patterns = value;
        }
        self.define_runtime_properties()?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" if self.props.has_property(name) => {
                Ok(PropertyValue::Integer(self.cur_pos as i64))
            }
            "Sequence" if self.props.has_property(name) => Ok(PropertyValue::String(
                if self.sequence_on { "On" } else { "Off" }.into(),
            )),
            "Sequence Trigger Edge" if self.props.has_property(name) => Ok(PropertyValue::String(
                if self.sequence_rising {
                    "Rising"
                } else {
                    "Falling"
                }
                .into(),
            )),
            "Blanking" if self.props.has_property(name) => Ok(PropertyValue::String(
                if self.blanking { "On" } else { "Off" }.into(),
            )),
            "Blank On" if self.props.has_property(name) => Ok(PropertyValue::String(
                if self.blank_on_low { "Low" } else { "High" }.into(),
            )),
            _ if name.starts_with("TTL-") && self.props.has_property(name) => {
                let pin = name
                    .trim_start_matches("TTL-")
                    .parse::<usize>()
                    .map_err(|_| MmError::InvalidProperty)?;
                let first_pin = self.pin_group as usize * 8 + 1;
                if pin < first_pin || pin >= first_pin + 8 {
                    return Err(MmError::InvalidProperty);
                }
                let bit = pin - first_pin;
                Ok(PropertyValue::Integer(((self.cur_pos >> bit) & 1) as i64))
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" if self.props.has_property(name) => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_position(v as u64)
            }
            "Sequence" if self.props.has_property(name) => {
                let label = val.to_string();
                self.sequence_on = match label.as_str() {
                    "On" => true,
                    "Off" => false,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.props.set(name, PropertyValue::String(label))
            }
            "Sequence Trigger Edge" if self.props.has_property(name) => {
                let label = val.to_string();
                self.sequence_rising = match label.as_str() {
                    "Rising" => true,
                    "Falling" => false,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.props.set(name, PropertyValue::String(label))
            }
            "Blanking" if self.props.has_property(name) => {
                let label = val.to_string();
                self.blanking = match label.as_str() {
                    "On" => true,
                    "Off" => false,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.send_blanking_command()?;
                self.props.set(name, PropertyValue::String(label))
            }
            "Blank On" if self.props.has_property(name) => {
                let label = val.to_string();
                self.blank_on_low = match label.as_str() {
                    "Low" => true,
                    "High" => false,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.send_blanking_command()?;
                self.props.set(name, PropertyValue::String(label))
            }
            _ if name.starts_with("TTL-") && self.props.has_property(name) => {
                let pin = name
                    .trim_start_matches("TTL-")
                    .parse::<usize>()
                    .map_err(|_| MmError::InvalidProperty)?;
                let first_pin = self.pin_group as usize * 8 + 1;
                if pin < first_pin || pin >= first_pin + 8 {
                    return Err(MmError::InvalidProperty);
                }
                let bit = pin - first_pin;
                let enabled = val.as_i64().ok_or(MmError::InvalidPropertyValue)? != 0;
                let mut pos = self.cur_pos;
                if enabled {
                    pos |= 1 << bit;
                } else {
                    pos &= !(1 << bit);
                }
                self.set_position(pos as u64)
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
        DeviceType::State
    }
    fn busy(&self) -> bool {
        false
    }
}

impl StateDevice for TriggerScopeMMTTL {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos > 255 {
            return Err(MmError::UnknownPosition);
        }
        let pos = pos as u8;
        if self.initialized {
            if self.gate_open {
                if pos != self.cur_pos || self.is_closed {
                    self.send_state(pos)?;
                    self.is_closed = false;
                }
            } else if !self.is_closed {
                self.send_state(0)?;
                self.is_closed = true;
            }
        }
        self.state_byte = if self.gate_open { pos } else { 0 };
        self.cur_pos = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        Ok(self.cur_pos as u64)
    }
    fn get_number_of_positions(&self) -> u64 {
        256
    }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        if pos > 255 {
            return Err(MmError::UnknownPosition);
        }
        Ok(format!("{:08b}", pos))
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let v =
            u64::from_str_radix(label, 2).map_err(|_| MmError::UnknownLabel(label.to_string()))?;
        self.set_position(v)
    }

    fn set_position_label(&mut self, _pos: u64, _label: &str) -> MmResult<()> {
        Err(MmError::NotSupported)
    }

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        if self.gate_open != open {
            self.gate_open = open;
            let value = if open { self.cur_pos } else { 0 };
            self.send_state(value)?;
            self.state_byte = value;
            self.is_closed = !open;
        }
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(self.gate_open)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn ttl_initialize() {
        let t = MockTransport::new().expect("PDN0\n", "PDN0-50");
        let mut ttl = TriggerScopeMMTTL::new(0).with_transport(Box::new(t));
        assert!(ttl.has_property("PinGroup"));
        assert!(!ttl.has_property("State"));
        assert!(!ttl.has_property("TTL-1"));
        assert!(ttl.get_property("State").is_err());
        ttl.initialize().unwrap();
        assert!(ttl.has_property("State"));
        assert!(ttl.has_property("Sequence"));
        assert!(ttl.has_property("TTL-1"));
        assert_eq!(ttl.get_position().unwrap(), 0);
    }

    #[test]
    fn ttl_set_state_byte() {
        let t = MockTransport::new()
            .expect("PDN1\n", "PDN1-50")
            .expect("SDO1-170\n", "!SDO1-170"); // 0b10101010
        let mut ttl = TriggerScopeMMTTL::new(1).with_transport(Box::new(t));
        ttl.initialize().unwrap();
        ttl.set_position(0b10101010).unwrap();
        assert_eq!(ttl.get_position().unwrap(), 0b10101010);
    }

    #[test]
    fn ttl_accepts_mm_echo_response() {
        let t = MockTransport::new()
            .expect("PDN1\n", "PDN1-50")
            .expect("SDO1-170\n", "SDO1-170"); // 0b10101010
        let mut ttl = TriggerScopeMMTTL::new(1).with_transport(Box::new(t));
        ttl.initialize().unwrap();
        ttl.set_position(0b10101010).unwrap();
        assert_eq!(ttl.get_position().unwrap(), 0b10101010);
    }

    #[test]
    fn ttl_rejects_unrelated_write_response() {
        let t = MockTransport::new()
            .expect("PDN0\n", "PDN0-50")
            .expect("SDO0-170\n", "!SDO1-170");
        let mut ttl = TriggerScopeMMTTL::new(0).with_transport(Box::new(t));
        ttl.initialize().unwrap();
        assert!(ttl.set_position(0b10101010).is_err());
    }

    #[test]
    fn ttl_out_of_range() {
        let t = MockTransport::new().expect("PDN0\n", "PDN0-50");
        let mut ttl = TriggerScopeMMTTL::new(0).with_transport(Box::new(t));
        ttl.initialize().unwrap();
        assert!(ttl.set_position(256).is_err());
    }

    #[test]
    fn ttl_line_properties_gate_blanking_and_sequence_commands() {
        let t = MockTransport::new()
            .expect("PDN0\n", "!PDN0-3")
            .expect("SDO0-2\n", "!SDO0-2")
            .expect("SDO0-0\n", "!SDO0-0")
            .expect("SDO0-2\n", "!SDO0-2")
            .expect("BDO0-1-0\n", "!BDO0-1-0")
            .expect("PDC0\n", "!PDC0")
            .expect("PDO0-0-1-2\n", "!PDO0-0-1-2")
            .expect("PDS0-1-1\n", "!PDS0-1-1")
            .expect("PDS0-0-1\n", "!PDS0-0-1");
        let mut ttl = TriggerScopeMMTTL::new(0).with_transport(Box::new(t));
        ttl.initialize().unwrap();
        assert!(ttl.has_property("TTL-1"));
        ttl.set_property("TTL-2", PropertyValue::Integer(1))
            .unwrap();
        assert_eq!(ttl.get_position().unwrap(), 2);
        ttl.set_gate_open(false).unwrap();
        assert_eq!(ttl.get_position().unwrap(), 2);
        ttl.set_gate_open(true).unwrap();
        ttl.set_property("Blanking", PropertyValue::String("On".into()))
            .unwrap();
        ttl.add_to_sequence(1).unwrap();
        ttl.add_to_sequence(2).unwrap();
        ttl.send_sequence().unwrap();
        ttl.start_sequence().unwrap();
        ttl.stop_sequence().unwrap();
    }
}
