/// Leica DMI filter turret / objective nosepiece (StateDevice).
///
/// Protocol (ASCII, `\r` terminated):
///   IL Turret (reflector) device address: "51"
///   Objective Turret device address:      "76"
///
///   Set position:   `"<dev>22 <pos>\r"` → `"<dev>22 <pos>\r"`
///   Get position:   `"<dev>23\r"`       → `"<dev>23 <pos>\r"`
///
/// Positions are 1-based in the Leica protocol; we store 0-based internally.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurretType {
    ILTurret,          // reflector, address "51", 6 positions
    ObjectiveTurret,   // objectives, address "76", 6 positions
}

pub struct LeicaDMITurret {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    turret_type: TurretType,
    position: u64,
    num_positions: u64,
    labels: Vec<String>,
    gate_open: bool,
}

impl LeicaDMITurret {
    pub fn new(turret_type: TurretType) -> Self {
        let num_positions = 6u64;
        let labels: Vec<String> = (0..num_positions).map(|i| format!("Position-{}", i)).collect();
        let mut props = PropertyMap::new();
        props.define_property("State", PropertyValue::Integer(0), false).unwrap();
        let type_str = match turret_type {
            TurretType::ILTurret        => "IL",
            TurretType::ObjectiveTurret => "Objective",
        };
        props.define_property("TurretType", PropertyValue::String(type_str.into()), true).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            turret_type,
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

    fn device_addr(&self) -> &'static str {
        match self.turret_type {
            TurretType::ILTurret        => "51",
            TurretType::ObjectiveTurret => "76",
        }
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

    fn send_position(&mut self, pos: u64) -> MmResult<()> {
        let dev = self.device_addr();
        // Leica positions are 1-based
        let cmd = format!("{}22 {}\r", dev, pos + 1);
        let resp = self.send_recv(&cmd)?;
        let expected_prefix = format!("{}22", dev);
        if !resp.starts_with(&expected_prefix) {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(())
    }

    fn query_position(&mut self) -> MmResult<u64> {
        let dev = self.device_addr();
        let cmd = format!("{}23\r", dev);
        let resp = self.send_recv(&cmd)?;
        let prefix = format!("{}23", dev);
        if !resp.starts_with(&prefix) {
            return Err(MmError::SerialInvalidResponse);
        }
        let val_str = resp[prefix.len()..].trim();
        let pos_1based: u64 = val_str.parse().map_err(|_| MmError::SerialInvalidResponse)?;
        Ok(pos_1based.saturating_sub(1))
    }
}

impl Device for LeicaDMITurret {
    fn name(&self) -> &str { "LeicaDMITurret" }
    fn description(&self) -> &str { "Leica DMI turret/nosepiece" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let pos = self.query_position()?;
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

impl StateDevice for LeicaDMITurret {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized {
            self.send_position(pos)?;
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
    fn il_turret_initialize_and_move() {
        // Init queries position: "5123\r" → "5123 2" (1-based pos 2 → internal 1)
        // Move to pos 2: "5122 3\r" (send pos+1=3) → "5122 3"
        let t = MockTransport::new()
            .expect("5123\r", "5123 2")   // query → pos 1 (0-based) from Leica 1-based 2
            .expect("5122 3\r", "5122 3"); // set to pos 2 (Leica 3)
        let mut turret = LeicaDMITurret::new(TurretType::ILTurret).with_transport(Box::new(t));
        turret.initialize().unwrap();
        assert_eq!(turret.get_position().unwrap(), 1);
        turret.set_position(2).unwrap();
        assert_eq!(turret.get_position().unwrap(), 2);
    }

    #[test]
    fn objective_turret_initialize() {
        let t = MockTransport::new()
            .expect("7623\r", "7623 1");  // pos 0 (0-based)
        let mut turret = LeicaDMITurret::new(TurretType::ObjectiveTurret).with_transport(Box::new(t));
        turret.initialize().unwrap();
        assert_eq!(turret.get_position().unwrap(), 0);
    }

    #[test]
    fn out_of_range_rejected() {
        let t = MockTransport::new()
            .expect("5123\r", "5123 1");
        let mut turret = LeicaDMITurret::new(TurretType::ILTurret).with_transport(Box::new(t));
        turret.initialize().unwrap();
        assert!(turret.set_position(6).is_err());
    }

    #[test]
    fn label_navigation() {
        let t = MockTransport::new()
            .expect("5123\r", "5123 1")
            .expect("5122 4\r", "5122 4");
        let mut turret = LeicaDMITurret::new(TurretType::ILTurret).with_transport(Box::new(t));
        turret.initialize().unwrap();
        turret.set_position_label(3, "DAPI").unwrap();
        turret.set_position_by_label("DAPI").unwrap();
        assert_eq!(turret.get_position().unwrap(), 3);
    }
}
