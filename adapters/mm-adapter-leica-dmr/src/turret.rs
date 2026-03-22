/// Leica DMR reflected light filter / objective nosepiece turret.
///
/// Protocol:
///   Set position:  device=rLFA(8/9), command=2, data=pos  → `"<DD>002<pos>\r"`
///   Get position:  device=rLFA, command=10                 → `"<DD>010<pos>\r"`
///
/// Objective nosepiece uses device_id=20, commands 2 (set) and 10 (get).
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct LeicaDMRTurret {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    device_id: u8,
    position: u64,
    num_positions: u64,
    labels: Vec<String>,
    gate_open: bool,
}

impl LeicaDMRTurret {
    /// `device_id`: rLFA4=8, rLFA8=9, ObjNosepiece=20
    /// `num_positions`: typically 4, 8, or 6
    pub fn new(device_id: u8, num_positions: u64) -> Self {
        let labels: Vec<String> = (0..num_positions).map(|i| format!("Position-{}", i)).collect();
        let mut props = PropertyMap::new();
        props.define_property("State", PropertyValue::Integer(0), false).unwrap();
        props.define_property("DeviceID", PropertyValue::Integer(device_id as i64), true).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            device_id,
            position: 0,
            num_positions,
            labels,
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

    fn send_position_cmd(&mut self, pos: u64) -> MmResult<()> {
        let dev = self.device_id;
        // Positions are 1-based in the Leica protocol
        let cmd = format!("{:02}002{}\r", dev, pos + 1);
        let resp = self.send_recv(&cmd)?;
        let prefix = format!("{:02}002", dev);
        if !resp.starts_with(&prefix) {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(())
    }

    fn query_position_cmd(&mut self) -> MmResult<u64> {
        let dev = self.device_id;
        let cmd = format!("{:02}010\r", dev);
        let resp = self.send_recv(&cmd)?;
        let prefix = format!("{:02}010", dev);
        if !resp.starts_with(&prefix) {
            return Err(MmError::SerialInvalidResponse);
        }
        let val_str = resp[prefix.len()..].trim();
        let pos_1based: u64 = val_str.parse().map_err(|_| MmError::SerialInvalidResponse)?;
        Ok(pos_1based.saturating_sub(1))
    }
}

impl Device for LeicaDMRTurret {
    fn name(&self) -> &str { "LeicaDMRTurret" }
    fn description(&self) -> &str { "Leica DMR filter turret or objective nosepiece" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let pos = self.query_position_cmd()?;
        self.position = pos;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::Integer(self.position as i64)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u64;
                self.set_position(pos)
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

impl StateDevice for LeicaDMRTurret {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized {
            self.send_position_cmd(pos)?;
        }
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> { Ok(self.position) }
    fn get_number_of_positions(&self) -> u64 { self.num_positions }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels.get(pos as usize).cloned().ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self.labels.iter().position(|l| l == label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64;
        self.set_position(pos)
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        if pos >= self.num_positions { return Err(MmError::UnknownPosition); }
        self.labels[pos as usize] = label.to_string();
        Ok(())
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
    fn rl_turret_initialize_and_move() {
        // device=8 (rLFA4), init: get pos → 1 (0-based 0), move to pos 2 → send 3
        let t = MockTransport::new()
            .expect("08010\r", "080101")    // get pos: Leica 1-based 1 → 0-based 0
            .expect("080023\r", "080023");  // set to pos 2 → send Leica 3
        let mut turret = LeicaDMRTurret::new(8, 4).with_transport(Box::new(t));
        turret.initialize().unwrap();
        assert_eq!(turret.get_position().unwrap(), 0);
        turret.set_position(2).unwrap();
        assert_eq!(turret.get_position().unwrap(), 2);
    }

    #[test]
    fn out_of_range_rejected() {
        let t = MockTransport::new()
            .expect("08010\r", "080101");
        let mut turret = LeicaDMRTurret::new(8, 4).with_transport(Box::new(t));
        turret.initialize().unwrap();
        assert!(turret.set_position(4).is_err());
    }

    #[test]
    fn label_navigation() {
        let t = MockTransport::new()
            .expect("08010\r", "080101")
            .expect("080023\r", "080023");
        let mut turret = LeicaDMRTurret::new(8, 4).with_transport(Box::new(t));
        turret.initialize().unwrap();
        turret.set_position_label(2, "GFP").unwrap();
        turret.set_position_by_label("GFP").unwrap();
        assert_eq!(turret.get_position().unwrap(), 2);
    }
}
