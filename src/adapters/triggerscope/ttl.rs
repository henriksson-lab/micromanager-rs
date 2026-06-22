/// TriggerScope TTL channel — digital output.
///
/// ASCII serial protocol, `\n` terminated.
///   Set TTL high: `"TTL<ch>,1\n"` → controller response
///   Set TTL low:  `"TTL<ch>,0\n"` → controller response
///   Get TTL:      `"TTL<ch>?\n"`   → `"TTL<ch> <0|1>\n"`
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

use super::hub::SharedTriggerScopeTransport;

pub struct TriggerScopeTTL {
    props: PropertyMap,
    transport: Option<SharedTriggerScopeTransport>,
    initialized: bool,
    channel: u8,
    name: String,
    state: bool,
    position: u64,
    gate_open: bool,
    sequence_on: bool,
    sequence: Vec<u8>,
}

impl TriggerScopeTTL {
    pub fn new(channel: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Channel", PropertyValue::Integer(channel as i64), true)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            channel,
            name: if channel == 0 {
                "TriggerScope-TTL-Master".to_string()
            } else {
                format!("TriggerScope-TTL{:02}", channel)
            },
            state: false,
            position: 0,
            gate_open: true,
            sequence_on: true,
            sequence: Vec::new(),
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

    fn send_state(&mut self, high: bool) -> MmResult<()> {
        let ch = self.channel;
        let val = if high { 1 } else { 0 };
        let cmd = format!("TTL{},{}\n", ch, val);
        let _ = self.send_recv(&cmd)?;
        Ok(())
    }

    fn ensure_runtime_properties(&mut self) -> MmResult<()> {
        if self.channel == 0 && !self.props.has_property("Sequence") {
            self.props
                .define_property("Sequence", PropertyValue::String("On".into()), false)?;
            self.props.set_allowed_values("Sequence", &["On", "Off"])?;
        }
        if !self.props.has_property("State") {
            self.props
                .define_property("State", PropertyValue::Integer(0), false)?;
            self.props.set_property_limits(
                "State",
                0.0,
                if self.channel == 0 { 16.0 } else { 1.0 },
            )?;
        }
        if !self.props.has_property("Label") {
            self.props
                .define_property("Label", PropertyValue::String(String::new()), false)?;
        }
        Ok(())
    }

    pub fn clear_sequence(&mut self) -> MmResult<()> {
        self.sequence.clear();
        Ok(())
    }

    pub fn add_to_sequence(&mut self, value: u8) -> MmResult<()> {
        self.sequence.push(value);
        Ok(())
    }

    pub fn load_sequence(&mut self) -> MmResult<()> {
        let ch = self.channel;
        self.send_recv(&format!("CLEAR_TTL,{}\n", ch))?;
        let sequence = self.sequence.clone();
        for (idx, value) in sequence.into_iter().enumerate() {
            self.send_recv(&format!("PROG_TTL,{},{},{}\n", idx + 1, ch, value))?;
        }
        Ok(())
    }

    pub fn start_sequence(&mut self) -> MmResult<()> {
        self.send_recv("ARM\n")?;
        Ok(())
    }

    pub fn stop_sequence(&mut self) -> MmResult<()> {
        Ok(())
    }
}

impl Device for TriggerScopeTTL {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "ARC TriggerScope TTL channel"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.initialized {
            return Ok(());
        }
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.ensure_runtime_properties()?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" if self.props.has_property("State") => {
                Ok(PropertyValue::Integer(self.position as i64))
            }
            "Label" if self.props.has_property("Label") => self
                .get_position_label(self.position)
                .map(PropertyValue::String),
            "Sequence" if self.props.has_property("Sequence") => Ok(PropertyValue::String(
                if self.sequence_on { "On" } else { "Off" }.into(),
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" if self.props.has_property("State") => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_position(v as u64)
            }
            "Sequence" if self.props.has_property("Sequence") => {
                let s = val.to_string();
                match s.as_str() {
                    "On" => self.sequence_on = true,
                    "Off" => self.sequence_on = false,
                    _ => return Err(MmError::InvalidPropertyValue),
                }
                self.props.set(name, PropertyValue::String(s))
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

impl StateDevice for TriggerScopeTTL {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        let max_pos = if self.channel == 0 { 16 } else { 1 };
        if pos > max_pos {
            return Err(MmError::UnknownPosition);
        }
        let high = pos > 0;
        if self.initialized {
            self.send_state(high)?;
        }
        self.state = high;
        self.position = pos;
        self.gate_open = high;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        Ok(self.position)
    }
    fn get_number_of_positions(&self) -> u64 {
        if self.channel == 0 {
            17
        } else {
            2
        }
    }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        if self.channel == 0 {
            match pos {
                0 => Ok("Closed".to_string()),
                1..=16 => Ok(format!("TTL{:02}", pos)),
                _ => Err(MmError::UnknownPosition),
            }
        } else {
            match pos {
                0 => Ok("Closed".to_string()),
                1 => Ok("Open".to_string()),
                _ => Err(MmError::UnknownPosition),
            }
        }
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        match label {
            "Closed" | "Low" => self.set_position(0),
            "Open" | "High" => self.set_position(1),
            _ if self.channel == 0 && label.starts_with("TTL") => {
                let pos = label[3..]
                    .parse::<u64>()
                    .map_err(|_| MmError::UnknownLabel(label.to_string()))?;
                self.set_position(pos)
            }
            _ => Err(MmError::UnknownLabel(label.to_string())),
        }
    }

    fn set_position_label(&mut self, _pos: u64, _label: &str) -> MmResult<()> {
        Err(MmError::NotSupported)
    }

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.set_position(if open { 1 } else { 0 })
    }

    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(self.gate_open)
    }
}

impl Shutter for TriggerScopeTTL {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.set_gate_open(open)
    }

    fn get_open(&self) -> MmResult<bool> {
        self.get_gate_open()
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::NotSupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn ttl_initialize_low() {
        let t = MockTransport::new();
        let mut ttl = TriggerScopeTTL::new(1).with_transport(Box::new(t));
        assert!(!ttl.has_property("State"));
        assert!(!ttl.has_property("Label"));
        ttl.initialize().unwrap();
        assert!(ttl.has_property("State"));
        assert!(ttl.has_property("Label"));
        assert_eq!(ttl.get_position().unwrap(), 0);
    }

    #[test]
    fn ttl_set_high_then_low() {
        let t = MockTransport::new()
            .expect("TTL3,1\n", "TTL3 OK")
            .expect("TTL3,0\n", "TTL3 OK");
        let mut ttl = TriggerScopeTTL::new(3).with_transport(Box::new(t));
        ttl.initialize().unwrap();
        ttl.set_position(1).unwrap();
        assert_eq!(ttl.get_position().unwrap(), 1);
        ttl.set_position(0).unwrap();
        assert_eq!(ttl.get_position().unwrap(), 0);
    }

    #[test]
    fn ttl_invalid_position() {
        let t = MockTransport::new();
        let mut ttl = TriggerScopeTTL::new(1).with_transport(Box::new(t));
        ttl.initialize().unwrap();
        assert!(ttl.set_position(2).is_err());
    }

    #[test]
    fn ttl_accepts_non_ok_echo_response() {
        let t = MockTransport::new().expect("TTL3,1\n", "TTL3,1");
        let mut ttl = TriggerScopeTTL::new(3).with_transport(Box::new(t));
        ttl.initialize().unwrap();
        ttl.set_position(1).unwrap();
        assert_eq!(ttl.get_position().unwrap(), 1);
    }

    #[test]
    fn ttl_master_exposes_positions_labels_and_sequences() {
        let t = MockTransport::new()
            .expect("CLEAR_TTL,0\n", "CLEAR_TTL,0")
            .expect("PROG_TTL,1,0,4\n", "PROG_TTL,1,0,4")
            .expect("PROG_TTL,2,0,8\n", "PROG_TTL,2,0,8")
            .expect("ARM\n", "ARM");
        let mut ttl = TriggerScopeTTL::new(0).with_transport(Box::new(t));
        ttl.initialize().unwrap();
        assert_eq!(ttl.name(), "TriggerScope-TTL-Master");
        assert_eq!(ttl.get_number_of_positions(), 17);
        assert_eq!(ttl.get_position_label(0).unwrap(), "Closed");
        assert_eq!(ttl.get_position_label(16).unwrap(), "TTL16");
        ttl.add_to_sequence(4).unwrap();
        ttl.add_to_sequence(8).unwrap();
        ttl.load_sequence().unwrap();
        ttl.start_sequence().unwrap();
    }
}
