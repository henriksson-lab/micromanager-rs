/// Ludl MAC5000/MAC6000 filter wheel.
///
/// Protocol (TX `\r`, RX `\n`):
///   `Rotat S<dev> M <pos>\r` → `:A`  (M = main wheel, pos 1-indexed)
///   `WHERE F\r`              → `:A <pos>` (current 1-indexed wheel position)
///
/// dev: device address; positions: 1-indexed on device, 0-indexed in MicroManager.
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, StateDevice};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

const DEFAULT_NUM_POSITIONS: u64 = 6;

pub struct LudlWheel {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    device: u8,
    position: u64,
    num_positions: u64,
    gate_open: bool,
}

impl LudlWheel {
    pub fn new(device: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("DeviceAddress", PropertyValue::Integer(device as i64), false).unwrap();
        props.define_property("NumPositions", PropertyValue::Integer(DEFAULT_NUM_POSITIONS as i64), false).unwrap();
        Self { props, transport: None, initialized: false, device, position: 0, num_positions: DEFAULT_NUM_POSITIONS, gate_open: true }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where F: FnOnce(&mut dyn Transport) -> MmResult<R> {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let c = format!("{}\r", command);
        self.call_transport(|t| { let r = t.send_recv(&c)?; Ok(r.trim().to_string()) })
    }

    fn check_a(resp: &str) -> MmResult<&str> {
        let s = resp.trim();
        if let Some(rest) = s.strip_prefix(":A") { Ok(rest.trim()) }
        else { Err(MmError::LocallyDefined(format!("Ludl error: {}", s))) }
    }
}

impl Default for LudlWheel { fn default() -> Self { Self::new(1) } }

impl Device for LudlWheel {
    fn name(&self) -> &str { "LudlWheel" }
    fn description(&self) -> &str { "Ludl MAC5000/MAC6000 filter wheel" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let r = self.cmd("WHERE F")?;
        let body = Self::check_a(&r)?;
        let pos1: u64 = body.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(1);
        self.position = pos1.saturating_sub(1);
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "NumPositions" {
            self.num_positions = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u64;
        }
        self.props.set(name, val)
    }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::State }
    fn busy(&self) -> bool { false }
}

impl StateDevice for LudlWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!("Position {} out of range", pos)));
        }
        let r = self.cmd(&format!("Rotat S{} M {}", self.device, pos + 1))?;
        Self::check_a(&r)?;
        self.position = pos;
        Ok(())
    }
    fn get_position(&self) -> MmResult<u64> { Ok(self.position) }
    fn get_number_of_positions(&self) -> u64 { self.num_positions }
    fn get_position_label(&self, pos: u64) -> MmResult<String> { Ok(format!("Position-{}", pos + 1)) }
    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos: u64 = label.strip_prefix("Position-")
            .and_then(|s| s.parse::<u64>().ok())
            .map(|p| p.saturating_sub(1))
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))?;
        self.set_position(pos)
    }
    fn set_position_label(&mut self, _pos: u64, _label: &str) -> MmResult<()> { Ok(()) }
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> { self.gate_open = open; Ok(()) }
    fn get_gate_open(&self) -> MmResult<bool> { Ok(self.gate_open) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    #[test]
    fn initialize() {
        let t = MockTransport::new().any(":A 2"); // position 2 (1-indexed) → 1 (0-indexed)
        let mut w = LudlWheel::new(1).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert_eq!(w.get_position().unwrap(), 1);
    }

    #[test]
    fn move_to_position() {
        let t = MockTransport::new().any(":A 1").any(":A");
        let mut w = LudlWheel::new(1).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position(3).unwrap();
        assert_eq!(w.get_position().unwrap(), 3);
    }

    #[test]
    fn out_of_range_fails() {
        let t = MockTransport::new().any(":A 1");
        let mut w = LudlWheel::new(1).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert!(w.set_position(6).is_err());
    }

    #[test]
    fn no_transport_error() { assert!(LudlWheel::new(1).initialize().is_err()); }
}
