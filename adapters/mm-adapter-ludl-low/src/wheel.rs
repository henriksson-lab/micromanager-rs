/// Ludl Low-level filter wheel (EFILS module).
///
/// Commands (CR terminated, low-level ASCII):
///   `FW,<module_id>,<wheel_num>,<pos>\r` → `:A\r\n` (set position, 1-based)
///   `FWQ,<module_id>,<wheel_num>\r`      → `<pos>\r\n` (query position)
///
/// The module ID is typically 17–21 (EFILS cards).
/// Wheel number is 1 or 2.  Positions are 1-based (1..=num_pos).
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct LudlLowWheel {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    module_id: u8,
    wheel_num: u8,
    num_positions: u64,
    position: u64,
    labels: Vec<String>,
    gate_open: bool,
}

impl LudlLowWheel {
    pub fn new(module_id: u8, wheel_num: u8, num_positions: u64) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        let labels: Vec<String> = (0..num_positions)
            .map(|i| format!("Filter-{}", i + 1))
            .collect();
        Self {
            props,
            transport: None,
            initialized: false,
            module_id,
            wheel_num,
            num_positions,
            position: 0,
            labels,
            gate_open: true,
        }
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

    fn check_ack(resp: &str) -> MmResult<()> {
        if resp.starts_with(":A") || resp == "A" {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("Ludl wheel error: {}", resp)))
        }
    }
}

impl Default for LudlLowWheel { fn default() -> Self { Self::new(17, 1, 6) } }

impl Device for LudlLowWheel {
    fn name(&self) -> &str { "LudlLow-FilterWheel" }
    fn description(&self) -> &str { "Ludl Low-level filter wheel" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let resp = self.cmd(&format!("FWQ,{},{}", self.module_id, self.wheel_num))?;
        let pos: u64 = resp.trim().parse().unwrap_or(1);
        self.position = pos.saturating_sub(1); // convert 1-based to 0-based
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::Integer(self.position as i64)),
            "Label" => Ok(PropertyValue::String(
                self.labels.get(self.position as usize).cloned().unwrap_or_default()
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u64;
                self.set_position(pos)
            }
            "Label" => {
                let label = val.as_str().to_string();
                self.set_position_by_label(&label)
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

impl StateDevice for LudlLowWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        // Ludl uses 1-based positions
        let hw_pos = pos + 1;
        let resp = self.cmd(&format!("FW,{},{},{}", self.module_id, self.wheel_num, hw_pos))?;
        Self::check_ack(&resp)?;
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
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
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
    fn initialize() {
        let t = MockTransport::new().any("3"); // starts at position 3 (1-based) → 2 (0-based)
        let mut s = LudlLowWheel::new(17, 1, 6).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_position().unwrap(), 2);
    }

    #[test]
    fn set_position() {
        let t = MockTransport::new().any("1").any(":A");
        let mut s = LudlLowWheel::new(17, 1, 6).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position(4).unwrap();
        assert_eq!(s.get_position().unwrap(), 4);
    }

    #[test]
    fn out_of_range() {
        let t = MockTransport::new().any("1");
        let mut s = LudlLowWheel::new(17, 1, 6).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_position(6).is_err());
    }

    #[test]
    fn num_positions() {
        assert_eq!(LudlLowWheel::new(17, 1, 6).get_number_of_positions(), 6);
    }

    #[test]
    fn position_labels() {
        let mut w = LudlLowWheel::new(17, 1, 6);
        w.set_position_label(0, "DAPI").unwrap();
        assert_eq!(w.get_position_label(0).unwrap(), "DAPI");
    }

    #[test]
    fn set_by_label() {
        let t = MockTransport::new().any("1").any(":A");
        let mut s = LudlLowWheel::new(17, 1, 6).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_label(2, "GFP").unwrap();
        s.set_position_by_label("GFP").unwrap();
        assert_eq!(s.get_position().unwrap(), 2);
    }

    #[test]
    fn gate() {
        let mut w = LudlLowWheel::new(17, 1, 6);
        assert!(w.get_gate_open().unwrap());
        w.set_gate_open(false).unwrap();
        assert!(!w.get_gate_open().unwrap());
    }

    #[test]
    fn no_transport_error() { assert!(LudlLowWheel::new(17, 1, 6).initialize().is_err()); }
}
