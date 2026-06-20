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
use std::time::{Duration, Instant};

const DEFAULT_NUM_POSITIONS: u64 = 10;

pub struct PriorWheel {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    id: u8,
    position: u64, // 0-indexed internally
    num_positions: u64,
    gate_open: bool,
    speed: i64,
    delay_ms: f64,
    changed_time: Option<Instant>,
}

impl PriorWheel {
    pub fn new(id: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
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
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("Label", PropertyValue::String("Filter-1".into()), false)
            .unwrap();
        props
            .define_property("Speed", PropertyValue::Integer(3), false)
            .unwrap();
        props
            .define_property("Delay", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property(
                "Closed_Position",
                PropertyValue::String(String::new()),
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
            speed: 3,
            delay_ms: 0.0,
            changed_time: None,
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

    fn clear_port(&self) -> MmResult<()> {
        self.call_transport(|t| t.purge())
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

    fn mark_changed(&mut self) {
        self.changed_time = Some(Instant::now());
    }

    fn delay_duration(&self) -> Duration {
        Duration::from_secs_f64((self.delay_ms.max(0.0)) / 1000.0)
    }

    fn closed_position(&self) -> u64 {
        self.props
            .get("Closed_Position")
            .ok()
            .and_then(|v| v.as_str().parse::<u64>().ok())
            .unwrap_or(0)
    }

    fn set_physical_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!(
                "Position {} out of range",
                pos
            )));
        }
        self.clear_port()?;
        let r = self.cmd(&format!("7,{},{}", self.id, pos + 1))?;
        Self::check_r(&r)?;
        self.mark_changed();
        Ok(())
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
        self.clear_port()?;
        self.cmd("COMP 0")?;
        let home = self.cmd(&format!("7,{},h", self.id))?;
        self.mark_changed();
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
        match name {
            "State" => Ok(PropertyValue::Integer(self.position as i64)),
            "Label" => Ok(PropertyValue::String(format!(
                "Filter-{}",
                self.position + 1
            ))),
            "Speed" => Ok(PropertyValue::Integer(self.speed)),
            "Delay" => Ok(PropertyValue::Float(self.delay_ms)),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => return Err(MmError::InvalidPropertyValue),
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if pos < 0 {
                    return Err(MmError::UnknownPosition);
                }
                return self.set_position(pos as u64);
            }
            "Label" => return self.set_position_by_label(val.as_str()),
            "NumPositions" => {
                let n = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u64;
                self.num_positions = n;
            }
            "Speed" => {
                self.speed = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            }
            "Delay" => {
                self.delay_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            }
            _ => {}
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
        self.changed_time
            .map(|t| t.elapsed() < self.delay_duration())
            .unwrap_or(false)
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
        if self.gate_open {
            self.set_physical_position(pos)?;
        } else {
            self.set_physical_position(self.closed_position())?;
        }
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
        if self.gate_open == open {
            return Ok(());
        }
        if open {
            self.set_physical_position(self.position)?;
        } else {
            self.set_physical_position(self.closed_position())?;
        }
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
    fn busy_uses_configured_delay_after_move() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .any("R")
            .expect("7,1,2\r", "R");
        let mut w = PriorWheel::new(1).with_transport(Box::new(t));
        w.set_property("Delay", PropertyValue::Float(1000.0))
            .unwrap();
        w.initialize().unwrap();
        assert!(w.busy());
    }

    #[test]
    fn closed_gate_moves_to_closed_position_but_keeps_logical_state() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .any("R")
            .expect("7,1,2\r", "R")
            .expect("7,1,4\r", "R")
            .expect("7,1,4\r", "R")
            .expect("7,1,6\r", "R")
            .expect("7,1\r", "6");
        let mut w = PriorWheel::new(1).with_transport(Box::new(t));
        w.set_property("Closed_Position", PropertyValue::String("3".into()))
            .unwrap();
        w.initialize().unwrap();
        w.set_gate_open(false).unwrap();
        w.set_position(5).unwrap();
        assert_eq!(w.get_property("State").unwrap(), PropertyValue::Integer(5));
        w.set_gate_open(true).unwrap();
        assert_eq!(w.get_position().unwrap(), 5);
    }

    #[test]
    fn no_transport_error() {
        assert!(PriorWheel::new(1).initialize().is_err());
    }
}
