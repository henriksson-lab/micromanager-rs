/// Prior Scientific ProScan filter wheel.
///
/// Protocol (TX `\r`, RX `\r`):
///   `7,<id>,h\r`    → home wheel (h = literal char 'h')
///   `7,<id>,<pos>\r`→ move to position (1-indexed); response `R\r`
///   `7,<id>\r`      → query current position (returns 1-indexed integer)
///
/// id: wheel index (1–3); positions: 1–N.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

const DEFAULT_NUM_POSITIONS: u64 = 10;

pub struct PriorWheel {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    id: u8,
    position: u64, // 0-indexed internally
    num_positions: u64,
    gate_open: bool,
}

impl PriorWheel {
    pub fn new(id: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("WheelId", PropertyValue::Integer(id as i64), false)
            .unwrap();
        props
            .define_property(
                "NumPositions",
                PropertyValue::Integer(DEFAULT_NUM_POSITIONS as i64),
                false,
            )
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            id,
            position: 0,
            num_positions: DEFAULT_NUM_POSITIONS,
            gate_open: true,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = RefCell::new(Some(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.borrow_mut().as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let c = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&c)?;
            Ok(r.trim().to_string())
        })
    }

    fn check_r(resp: &str) -> MmResult<()> {
        if resp.trim() == "R" {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!(
                "Prior wheel error: {}",
                resp
            )))
        }
    }
}

impl Default for PriorWheel {
    fn default() -> Self {
        Self::new(1)
    }
}

impl Device for PriorWheel {
    fn name(&self) -> &str {
        "PriorWheel"
    }
    fn description(&self) -> &str {
        "Prior Scientific ProScan filter wheel"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        self.cmd("COMP 0")?;
        let home = self.cmd(&format!("7,{},h", self.id))?;
        if home.starts_with('E') && home.len() > 2 {
            return Err(MmError::LocallyDefined(format!(
                "Prior wheel error: {}",
                home
            )));
        }
        self.set_position(1)?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "NumPositions" {
            let n = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u64;
            self.num_positions = n;
        }
        self.props.set(name, val)
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

impl StateDevice for PriorWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!(
                "Position {} out of range",
                pos
            )));
        }
        // Device uses 1-indexed positions
        let r = self.cmd(&format!("7,{},{}", self.id, pos + 1))?;
        Self::check_r(&r)?;
        self.position = pos;
        Ok(())
    }
    fn get_position(&self) -> MmResult<u64> {
        let resp = self.cmd(&format!("7,{}", self.id))?;
        let one_based: u64 = resp
            .trim()
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        Ok(one_based.saturating_sub(1))
    }
    fn get_number_of_positions(&self) -> u64 {
        self.num_positions
    }
    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        Ok(format!("Filter-{}", pos + 1))
    }
    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos: u64 = label
            .strip_prefix("Filter-")
            .and_then(|s| s.parse::<u64>().ok())
            .map(|p| p.saturating_sub(1))
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))?;
        self.set_position(pos)
    }
    fn set_position_label(&mut self, _pos: u64, _label: &str) -> MmResult<()> {
        Ok(())
    }
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
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
    fn initialize() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .any("R")
            .expect("7,1,2\r", "R")
            .expect("7,1\r", "2");
        let mut w = PriorWheel::new(1).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert_eq!(w.get_position().unwrap(), 1);
    }

    #[test]
    fn move_to_position() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .any("R")
            .expect("7,1,2\r", "R")
            .expect("7,1,5\r", "R")
            .expect("7,1\r", "5");
        let mut w = PriorWheel::new(1).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position(4).unwrap();
        assert_eq!(w.get_position().unwrap(), 4);
    }

    #[test]
    fn out_of_range_fails() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .any("R")
            .expect("7,1,2\r", "R");
        let mut w = PriorWheel::new(1).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert!(w.set_position(10).is_err()); // default 10 positions (0-9)
    }

    #[test]
    fn no_transport_error() {
        assert!(PriorWheel::new(1).initialize().is_err());
    }
}
