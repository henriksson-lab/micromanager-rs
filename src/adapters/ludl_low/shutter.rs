/// Ludl Low-level shutter adapter.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};

pub struct LudlLowShutter {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    module_id: u8,
    shutter_num: u8,
    open: Cell<bool>,
}

impl LudlLowShutter {
    pub fn new(module_id: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("ID", PropertyValue::Integer(module_id as i64), false)
            .unwrap();
        props
            .set_allowed_values("ID", &["17", "18", "19", "20", "21"])
            .unwrap();
        props
            .define_property("LudlShutterNumber", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .set_allowed_values("LudlShutterNumber", &["1", "2", "3"])
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            module_id,
            shutter_num: 1,
            open: Cell::new(false),
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

    fn shutter_command_byte(&self, open: bool) -> MmResult<u8> {
        match (self.shutter_num, open) {
            (1, true) => Ok(74),
            (1, false) => Ok(75),
            (2, true) => Ok(76),
            (2, false) => Ok(77),
            (3, true) => Ok(79),
            (3, false) => Ok(80),
            _ => Err(MmError::InvalidPropertyValue),
        }
    }

    fn set_shutter_position(&self, open: bool) -> MmResult<()> {
        let cmd = [self.module_id, self.shutter_command_byte(open)?, 0, 58];
        self.send_raw(&cmd)?;
        self.open.set(open);
        Ok(())
    }

    fn read_shutter_position(&self) -> MmResult<bool> {
        self.call_transport(|t| t.purge())?;
        self.send_raw(&[self.module_id, 115, 1, 58])?;
        let reply = self.read_one()?;
        let open = match self.shutter_num {
            1 => (reply & 4) != 0,
            2 => (reply & 8) != 0,
            3 => (reply & 16) != 0,
            _ => return Err(MmError::InvalidPropertyValue),
        };
        self.open.set(open);
        Ok(open)
    }

    fn ensure_runtime_properties(&mut self) -> MmResult<()> {
        if !self.props.has_property("State") {
            self.props.define_property(
                "State",
                PropertyValue::Integer(if self.open.get() { 1 } else { 0 }),
                false,
            )?;
            self.props.set_allowed_values("State", &["0", "1"])?;
        }
        Ok(())
    }
}

impl Default for LudlLowShutter {
    fn default() -> Self {
        Self::new(17)
    }
}

impl Device for LudlLowShutter {
    fn name(&self) -> &str {
        "LudlShutter"
    }
    fn description(&self) -> &str {
        "Ludl shutter adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        self.send_command_level_low()?;
        if self.busy_status()? {
            return Err(MmError::LocallyDefined(
                "Ludl low-level shutter busy".into(),
            ));
        }
        self.read_shutter_position()?;
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
            "State" => Ok(PropertyValue::Integer(if self.read_shutter_position()? {
                1
            } else {
                0
            })),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
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
            "LudlShutterNumber" => {
                let shutter = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !(1..=3).contains(&shutter) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.shutter_num = shutter as u8;
                self.props.set(name, val)
            }
            "State" => {
                let open = match val.as_i64().ok_or(MmError::InvalidPropertyValue)? {
                    0 => false,
                    1 => true,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.set_shutter_position(open)
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
        DeviceType::Shutter
    }
    fn busy(&self) -> bool {
        self.busy_status().unwrap_or(true)
    }
}

impl Shutter for LudlLowShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.set_shutter_position(open)
    }

    fn get_open(&self) -> MmResult<bool> {
        self.read_shutter_position()
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_closed() {
        let t = MockTransport::new().expect_binary(b"b").expect_binary(&[0]);
        let mut s = LudlLowShutter::new(17).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.open.get());
    }

    #[test]
    fn initialize_open() {
        let t = MockTransport::new().expect_binary(b"b").expect_binary(&[4]);
        let mut s = LudlLowShutter::new(17).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.open.get());
    }

    #[test]
    fn open_close() {
        let t = MockTransport::new().expect_binary(b"b").expect_binary(&[0]);
        let mut s = LudlLowShutter::new(17).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.open.get());
        s.set_open(false).unwrap();
        assert!(!s.open.get());
    }

    #[test]
    fn no_transport_error() {
        assert!(LudlLowShutter::new(17).initialize().is_err());
    }

    #[test]
    fn fire_is_unsupported_like_upstream() {
        let mut s = LudlLowShutter::new(17);
        assert!(s.fire(1.0).is_err());
    }

    #[test]
    fn upstream_name_and_description() {
        let s = LudlLowShutter::new(17);
        assert_eq!(s.name(), "LudlShutter");
        assert_eq!(s.description(), "Ludl shutter adapter");
    }

    #[test]
    fn runtime_state_property_created_on_initialize() {
        let t = MockTransport::new().expect_binary(b"b").expect_binary(&[0]);
        let mut s = LudlLowShutter::new(17).with_transport(Box::new(t));

        assert!(!s.has_property("State"));

        s.initialize().unwrap();

        assert!(s.has_property("State"));
        assert!(s.property_names().contains(&"State".to_string()));
    }
}
