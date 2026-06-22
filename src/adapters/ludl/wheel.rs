/// Ludl MAC5000/MAC6000 filter wheel.
///
/// Protocol (TX `\r`, RX `\n`):
///   `Rotat S<dev> M|A <pos>\r` → `:A`  (pos 1-indexed, or 0 for tenth)
///   `Rotat S<dev> M|A H\r`     → `:A`  (home wheel)
///
/// dev: device address; positions: 1-indexed on device, 0-indexed in MicroManager.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;
use std::time::{Duration, Instant};

const DEFAULT_NUM_POSITIONS: u64 = 6;

pub struct LudlWheel {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    device: u8,
    wheel_number: u8,
    position: u64,
    num_positions: u64,
    home_timeout_s: f64,
    gate_open: bool,
}

impl LudlWheel {
    pub fn new(device: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "LudlDeviceNumberWheel",
                PropertyValue::Integer(device as i64),
                false,
            )
            .unwrap();
        props
            .define_property("LudlWheelNumber", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .define_property(
                "Fiter Positions",
                PropertyValue::Integer(DEFAULT_NUM_POSITIONS as i64),
                false,
            )
            .unwrap();
        props
            .define_property("Home-Timeout-(s)", PropertyValue::Float(10.0), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            device,
            wheel_number: 1,
            position: 0,
            num_positions: DEFAULT_NUM_POSITIONS,
            home_timeout_s: 10.0,
            gate_open: true,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(RefCell::new(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_ref() {
            Some(t) => f(t.borrow_mut().as_mut()),
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

    fn check_a(resp: &str) -> MmResult<&str> {
        let s = resp.trim();
        if let Some(rest) = s.strip_prefix(":A") {
            Ok(rest.trim())
        } else {
            Err(MmError::LocallyDefined(format!("Ludl error: {}", s)))
        }
    }

    fn wheel_selector(&self) -> &'static str {
        if self.wheel_number == 1 {
            "M"
        } else {
            "A"
        }
    }

    fn device_position(&self, pos: u64) -> u64 {
        if self.num_positions == 10 && pos == 9 {
            0
        } else {
            pos + 1
        }
    }

    fn poll_module_busy(&self) -> bool {
        self.call_transport(|t| {
            if t.purge().is_err() || t.send("STATUS S\r").is_err() {
                return Ok(false);
            }
            Ok(match t.receive_line() {
                Ok(resp) => match resp.trim().as_bytes().first().copied() {
                    Some(b'N') => false,
                    Some(b'B') => true,
                    _ => true,
                },
                Err(_) => true,
            })
        })
        .unwrap_or(false)
    }

    fn home_wheel(&mut self) -> MmResult<()> {
        let r = self.cmd(&format!(
            "Rotat S{} {} H",
            self.device,
            self.wheel_selector()
        ))?;
        Self::check_a(&r)?;

        let timeout = Duration::from_secs_f64(self.home_timeout_s.max(0.0));
        let start = Instant::now();
        while start.elapsed() < timeout {
            if !self.poll_module_busy() {
                self.position = 0;
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }

        self.position = 0;
        Ok(())
    }
}

impl Default for LudlWheel {
    fn default() -> Self {
        Self::new(1)
    }
}

impl Device for LudlWheel {
    fn name(&self) -> &str {
        "LudlWheel"
    }
    fn description(&self) -> &str {
        "Ludl MAC5000/MAC6000 filter wheel"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.home_wheel()?;
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
        match name {
            "LudlDeviceNumberWheel" => {
                if self.initialized {
                    return Err(MmError::InvalidPropertyValue);
                }
                let device = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !(1..=5).contains(&device) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.device = device as u8;
            }
            "LudlWheelNumber" => {
                let wheel = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !(1..=2).contains(&wheel) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.wheel_number = wheel as u8;
            }
            "Fiter Positions" => {
                let count = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if count != 6 && count != 10 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.num_positions = count as u64;
            }
            "Home-Timeout-(s)" => {
                self.home_timeout_s = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
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
        self.poll_module_busy()
    }
}

impl StateDevice for LudlWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!(
                "Position {} out of range",
                pos
            )));
        }
        let r = self.cmd(&format!(
            "Rotat S{} {} {}",
            self.device,
            self.wheel_selector(),
            self.device_position(pos)
        ))?;
        Self::check_a(&r)?;
        self.position = pos;
        Ok(())
    }
    fn get_position(&self) -> MmResult<u64> {
        Ok(self.position)
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
            .expect("Rotat S1 M H\r", ":A")
            .expect("STATUS S\r", "N");
        let mut w = LudlWheel::new(1).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert_eq!(w.get_position().unwrap(), 0);
    }

    #[test]
    fn move_to_position() {
        let t = MockTransport::new().any(":A 1").any("N").any(":A");
        let mut w = LudlWheel::new(1).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position(3).unwrap();
        assert_eq!(w.get_position().unwrap(), 3);
    }

    #[test]
    fn out_of_range_fails() {
        let t = MockTransport::new().any(":A 1").any("N");
        let mut w = LudlWheel::new(1).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert!(w.set_position(6).is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(LudlWheel::new(1).initialize().is_err());
    }

    #[test]
    fn ten_position_wheel_uses_zero_for_tenth_device_position() {
        let t = MockTransport::new()
            .any(":A 1")
            .any("N")
            .expect("Rotat S1 M 0\r", ":A");
        let mut w = LudlWheel::new(1).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_property("Fiter Positions", PropertyValue::Integer(10))
            .unwrap();
        w.set_position(9).unwrap();
        assert_eq!(w.get_position().unwrap(), 9);
    }

    #[test]
    fn upstream_property_names_labels_and_second_wheel_selector() {
        let t = MockTransport::new()
            .expect("Rotat S1 A H\r", ":A")
            .expect("STATUS S\r", "N")
            .expect("Rotat S1 A 2\r", ":A");
        let mut w = LudlWheel::new(1).with_transport(Box::new(t));
        assert!(w.has_property("LudlDeviceNumberWheel"));
        assert!(w.has_property("LudlWheelNumber"));
        assert!(w.has_property("Fiter Positions"));
        assert!(w.has_property("Home-Timeout-(s)"));
        assert!(!w.has_property("DeviceAddress"));
        assert!(!w.has_property("NumPositions"));

        w.set_property("LudlWheelNumber", PropertyValue::Integer(2))
            .unwrap();
        w.initialize().unwrap();

        assert_eq!(w.get_position_label(0).unwrap(), "Filter-1");
        w.set_position_by_label("Filter-2").unwrap();
        assert_eq!(w.get_position().unwrap(), 1);
    }

    #[test]
    fn busy_polls_upstream_status_s() {
        let t = MockTransport::new().expect("STATUS S\r", "B");
        let w = LudlWheel::new(1).with_transport(Box::new(t));
        assert!(w.busy());
    }

    #[test]
    fn initialize_waits_for_home_busy_to_clear() {
        let t = MockTransport::new()
            .expect("Rotat S1 M H\r", ":A")
            .expect("STATUS S\r", "B")
            .expect("STATUS S\r", "N");
        let mut w = LudlWheel::new(1).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert_eq!(w.get_position().unwrap(), 0);
    }
}
