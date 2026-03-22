/// TriggerScope TTL channel — digital output.
///
/// ASCII serial protocol, `\n` terminated.
///   Set TTL high: `"TTL<ch> 1\n"` → `"TTL<ch> OK\n"`
///   Set TTL low:  `"TTL<ch> 0\n"` → `"TTL<ch> OK\n"`
///   Get TTL:      `"TTL<ch>?\n"`   → `"TTL<ch> <0|1>\n"`
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct TriggerScopeTTL {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    channel: u8,
    state: bool,
    gate_open: bool,
}

impl TriggerScopeTTL {
    pub fn new(channel: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Channel", PropertyValue::Integer(channel as i64), true).unwrap();
        props.define_property("State", PropertyValue::Integer(0), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            channel,
            state: false,
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

    fn send_state(&mut self, high: bool) -> MmResult<()> {
        let ch = self.channel;
        let val = if high { 1 } else { 0 };
        let cmd = format!("TTL{:02} {}\n", ch, val);
        let resp = self.send_recv(&cmd)?;
        if !resp.contains("OK") {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(())
    }
}

impl Device for TriggerScopeTTL {
    fn name(&self) -> &str { "TriggerScopeTTL" }
    fn description(&self) -> &str { "ARC TriggerScope TTL channel" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.send_state(false)?;
        self.state = false;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send_state(false);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::Integer(if self.state { 1 } else { 0 })),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_position(v as u64)
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::State }
    fn busy(&self) -> bool { false }
}

impl StateDevice for TriggerScopeTTL {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos > 1 { return Err(MmError::UnknownPosition); }
        let high = pos == 1;
        if self.initialized {
            self.send_state(high)?;
        }
        self.state = high;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> { Ok(if self.state { 1 } else { 0 }) }
    fn get_number_of_positions(&self) -> u64 { 2 }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        match pos {
            0 => Ok("Low".to_string()),
            1 => Ok("High".to_string()),
            _ => Err(MmError::UnknownPosition),
        }
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        match label {
            "Low"  => self.set_position(0),
            "High" => self.set_position(1),
            _ => Err(MmError::UnknownLabel(label.to_string())),
        }
    }

    fn set_position_label(&mut self, _pos: u64, _label: &str) -> MmResult<()> {
        Err(MmError::NotSupported)
    }

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> { Ok(self.gate_open) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn ttl_initialize_low() {
        let t = MockTransport::new()
            .expect("TTL01 0\n", "TTL01 OK");
        let mut ttl = TriggerScopeTTL::new(1).with_transport(Box::new(t));
        ttl.initialize().unwrap();
        assert_eq!(ttl.get_position().unwrap(), 0);
    }

    #[test]
    fn ttl_set_high_then_low() {
        let t = MockTransport::new()
            .expect("TTL03 0\n", "TTL03 OK")
            .expect("TTL03 1\n", "TTL03 OK")
            .expect("TTL03 0\n", "TTL03 OK");
        let mut ttl = TriggerScopeTTL::new(3).with_transport(Box::new(t));
        ttl.initialize().unwrap();
        ttl.set_position(1).unwrap();
        assert_eq!(ttl.get_position().unwrap(), 1);
        ttl.set_position(0).unwrap();
        assert_eq!(ttl.get_position().unwrap(), 0);
    }

    #[test]
    fn ttl_invalid_position() {
        let t = MockTransport::new()
            .expect("TTL01 0\n", "TTL01 OK");
        let mut ttl = TriggerScopeTTL::new(1).with_transport(Box::new(t));
        ttl.initialize().unwrap();
        assert!(ttl.set_position(2).is_err());
    }
}
