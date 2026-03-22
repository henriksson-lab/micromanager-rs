/// TriggerScope MM TTL — 8-channel digital output bank.
///
/// Protocol: `"PDN<group>\n"` to query number of patterns.
///           `"TTL<group>-<byte_value>\n"` to set TTL state byte.
///
/// Group 0 = TTL channels 1-8, Group 1 = channels 9-16.
/// The byte value sets all 8 lines simultaneously (0-255).
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct TriggerScopeMMTTL {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    pin_group: u8,
    state_byte: u8,
    gate_open: bool,
}

impl TriggerScopeMMTTL {
    /// `pin_group`: 0 for TTL1-8, 1 for TTL9-16.
    pub fn new(pin_group: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("PinGroup", PropertyValue::Integer(pin_group as i64), true).unwrap();
        props.define_property("State", PropertyValue::Integer(0), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            pin_group,
            state_byte: 0,
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

    fn send_state(&mut self, val: u8) -> MmResult<()> {
        let grp = self.pin_group;
        let cmd = format!("TTL{}-{}\n", grp, val);
        let resp = self.send_recv(&cmd)?;
        if !resp.contains("OK") && !resp.contains("TTL") {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(())
    }
}

impl Device for TriggerScopeMMTTL {
    fn name(&self) -> &str { "TriggerScopeMMTTL" }
    fn description(&self) -> &str { "ARC TriggerScope MM TTL bank" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Query number of patterns for this group
        let grp = self.pin_group;
        let cmd = format!("PDN{}\n", grp);
        let _resp = self.send_recv(&cmd)?;
        // Initialize to all zeros
        self.send_state(0)?;
        self.state_byte = 0;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send_state(0);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::Integer(self.state_byte as i64)),
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

impl StateDevice for TriggerScopeMMTTL {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos > 255 { return Err(MmError::UnknownPosition); }
        if self.initialized {
            self.send_state(pos as u8)?;
        }
        self.state_byte = pos as u8;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> { Ok(self.state_byte as u64) }
    fn get_number_of_positions(&self) -> u64 { 256 }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        if pos > 255 { return Err(MmError::UnknownPosition); }
        Ok(format!("{:08b}", pos))
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let v = u64::from_str_radix(label, 2).map_err(|_| MmError::UnknownLabel(label.to_string()))?;
        self.set_position(v)
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
    fn ttl_initialize() {
        let t = MockTransport::new()
            .expect("PDN0\n", "PDN0-50")
            .expect("TTL0-0\n", "TTL0 OK");
        let mut ttl = TriggerScopeMMTTL::new(0).with_transport(Box::new(t));
        ttl.initialize().unwrap();
        assert_eq!(ttl.get_position().unwrap(), 0);
    }

    #[test]
    fn ttl_set_state_byte() {
        let t = MockTransport::new()
            .expect("PDN1\n", "PDN1-50")
            .expect("TTL1-0\n", "TTL1 OK")
            .expect("TTL1-170\n", "TTL1 OK"); // 0b10101010
        let mut ttl = TriggerScopeMMTTL::new(1).with_transport(Box::new(t));
        ttl.initialize().unwrap();
        ttl.set_position(0b10101010).unwrap();
        assert_eq!(ttl.get_position().unwrap(), 0b10101010);
    }

    #[test]
    fn ttl_out_of_range() {
        let t = MockTransport::new()
            .expect("PDN0\n", "PDN0-50")
            .expect("TTL0-0\n", "TTL0 OK");
        let mut ttl = TriggerScopeMMTTL::new(0).with_transport(Box::new(t));
        ttl.initialize().unwrap();
        assert!(ttl.set_position(256).is_err());
    }
}
