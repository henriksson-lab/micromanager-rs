/// Ludl Low-level filter wheel (EFILS module).
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};

pub struct LudlLowWheel {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    module_id: u8,
    wheel_num: u8,
    num_positions: u64,
    position: Cell<u64>,
    labels: Vec<String>,
    gate_open: bool,
}

impl LudlLowWheel {
    pub fn new(module_id: u8, wheel_num: u8, num_positions: u64) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property(
                "LudlWheelNumber",
                PropertyValue::Integer(wheel_num as i64),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("LudlWheelNumber", &["1", "2"])
            .unwrap();
        props
            .define_property("ID", PropertyValue::Integer(module_id as i64), false)
            .unwrap();
        props
            .set_allowed_values("ID", &["17", "18", "19", "20", "21"])
            .unwrap();
        let labels: Vec<String> = (0..num_positions)
            .map(|i| format!("Filter-{}", i + 1))
            .collect();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            module_id,
            wheel_num,
            num_positions,
            position: Cell::new(0),
            labels,
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
        let mut transport = self.transport.borrow_mut();
        match transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn send_command_level_low(&self) -> MmResult<()> {
        self.call_transport(|t| t.send_bytes(&[255, 66]))
    }

    fn send_raw(&self, bytes: &[u8]) -> MmResult<()> {
        self.call_transport(|t| t.send_bytes(bytes))
    }

    fn read_one(&self) -> MmResult<u8> {
        self.call_transport(|t| {
            let bytes = t.receive_bytes(1)?;
            bytes.first().copied().ok_or(MmError::SerialTimeout)
        })
    }

    fn busy_status(&self) -> MmResult<bool> {
        self.call_transport(|t| t.purge())?;
        self.send_raw(&[self.module_id, 63, 58])?;
        match self.read_one()? {
            b'b' => Ok(false),
            b'B' => Ok(true),
            other => Err(MmError::LocallyDefined(format!(
                "Unrecognized Ludl status byte: {other}"
            ))),
        }
    }

    fn home_wheel(&self) -> MmResult<()> {
        let cmd_byte = match self.wheel_num {
            1 => 72,
            2 => 71,
            _ => return Err(MmError::InvalidPropertyValue),
        };
        self.send_raw(&[self.module_id, cmd_byte, 0, 58])
    }

    fn set_wheel_position_raw(&self, pos: u64) -> MmResult<()> {
        let idx = usize::try_from(pos).map_err(|_| MmError::UnknownPosition)?;
        let wheel1 = [49, 50, 51, 52, 53, 54];
        let wheel2 = [33, 64, 35, 36, 37, 94];
        let cmd_byte = match self.wheel_num {
            1 => wheel1.get(idx),
            2 => wheel2.get(idx),
            _ => None,
        }
        .copied()
        .ok_or(MmError::UnknownPosition)?;
        self.send_raw(&[self.module_id, cmd_byte, 0, 58])?;
        self.position.set(pos);
        Ok(())
    }

    fn read_wheel_position(&self) -> MmResult<u64> {
        self.call_transport(|t| t.purge())?;
        let cmd_byte = match self.wheel_num {
            1 => 97,
            2 => 98,
            _ => return Err(MmError::InvalidPropertyValue),
        };
        self.send_raw(&[self.module_id, cmd_byte, 1, 58])?;
        let reply = self.read_one()?;
        if reply == 0 {
            return Err(MmError::SerialInvalidResponse);
        }
        let pos = u64::from(reply - 1);
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        self.position.set(pos);
        Ok(pos)
    }

    fn ensure_runtime_properties(&mut self) -> MmResult<()> {
        if !self.props.has_property("State") {
            self.props.define_property(
                "State",
                PropertyValue::Integer(self.position.get() as i64),
                false,
            )?;
        }
        if !self.props.has_property("Label") {
            self.props.define_property(
                "Label",
                PropertyValue::String("Undefined".into()),
                false,
            )?;
        }
        Ok(())
    }
}

impl Default for LudlLowWheel {
    fn default() -> Self {
        Self::new(17, 1, 6)
    }
}

impl Device for LudlLowWheel {
    fn name(&self) -> &str {
        "LudlWheel"
    }
    fn description(&self) -> &str {
        "Ludl filter wheel adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        self.send_command_level_low()?;
        if self.busy_status()? {
            return Err(MmError::LocallyDefined("Ludl module busy".into()));
        }
        self.home_wheel()?;
        self.read_wheel_position()?;
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
            "State" => Ok(PropertyValue::Integer(self.read_wheel_position()? as i64)),
            "Label" => Ok(PropertyValue::String(
                self.labels
                    .get(self.position.get() as usize)
                    .cloned()
                    .unwrap_or_default(),
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
            "ID" => {
                if self.initialized {
                    return Err(MmError::InvalidPropertyValue);
                }
                let id = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !(17..=21).contains(&id) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.module_id = id as u8;
                self.props.set(name, val)
            }
            "LudlWheelNumber" => {
                let wheel = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !(1..=2).contains(&wheel) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.wheel_num = wheel as u8;
                self.props.set(name, val)
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
        self.busy_status().unwrap_or(true)
    }
}

impl StateDevice for LudlLowWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        self.set_wheel_position_raw(pos)
    }

    fn get_position(&self) -> MmResult<u64> {
        self.read_wheel_position()
    }

    fn get_number_of_positions(&self) -> u64 {
        self.num_positions
    }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels
            .get(pos as usize)
            .cloned()
            .ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self
            .labels
            .iter()
            .position(|l| l == label)
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
        let t = MockTransport::new().expect_binary(b"b").expect_binary(&[3]); // starts at position 3 (1-based) -> 2 (0-based)
        let mut s = LudlLowWheel::new(17, 1, 6).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.position.get(), 2);
    }

    #[test]
    fn set_position() {
        let t = MockTransport::new()
            .expect_binary(b"b")
            .expect_binary(&[1])
            .expect_binary(&[3]);
        let mut s = LudlLowWheel::new(17, 1, 6).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position(4).unwrap();
        assert_eq!(s.position.get(), 4);
    }

    #[test]
    fn out_of_range() {
        let t = MockTransport::new()
            .expect_binary(b"b")
            .expect_binary(&[1])
            .expect_binary(&[3]);
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
        let t = MockTransport::new()
            .expect_binary(b"b")
            .expect_binary(&[1])
            .expect_binary(&[3]);
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
    fn no_transport_error() {
        assert!(LudlLowWheel::new(17, 1, 6).initialize().is_err());
    }

    #[test]
    fn upstream_name_and_description() {
        let w = LudlLowWheel::new(17, 1, 6);
        assert_eq!(w.name(), "LudlWheel");
        assert_eq!(w.description(), "Ludl filter wheel adapter");
    }

    #[test]
    fn runtime_state_and_label_properties_created_on_initialize() {
        let t = MockTransport::new().expect_binary(b"b").expect_binary(&[1]);
        let mut w = LudlLowWheel::new(17, 1, 6).with_transport(Box::new(t));

        assert!(!w.has_property("State"));
        assert!(!w.has_property("Label"));

        w.initialize().unwrap();

        assert!(w.has_property("State"));
        assert!(w.has_property("Label"));
        assert!(w.property_names().contains(&"State".to_string()));
        assert!(w.property_names().contains(&"Label".to_string()));
    }
}
