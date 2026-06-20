/// Prior Scientific ProScan shutter.
///
/// Protocol (TX `\r`, RX `\r`):
///   `8,<id>,0\r`    → open shutter (0 = open)
///   `8,<id>,1\r`    → close shutter (1 = closed)
///   `8,<id>\r`      → query state → `0` (open) or `1` (closed)
///
/// id: shutter index (1–3).
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

pub struct PriorShutter {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    id: u8,
    is_open: bool,
}

impl PriorShutter {
    pub fn new(id: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("ShutterId", PropertyValue::Integer(id as i64), false)
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            id,
            is_open: false,
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
}

impl Default for PriorShutter {
    fn default() -> Self {
        Self::new(1)
    }
}

impl Device for PriorShutter {
    fn name(&self) -> &str {
        "PriorShutter"
    }
    fn description(&self) -> &str {
        "Prior Scientific ProScan shutter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        self.cmd("COMP 0")?;
        // Query initial state
        let r = self.cmd(&format!("8,{}", self.id))?;
        self.is_open = r.trim() == "0";
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
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
        DeviceType::Shutter
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Shutter for PriorShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        // 0 = open, 1 = closed
        let val = if open { 0 } else { 1 };
        let resp = self.cmd(&format!("8,{},{}", self.id, val))?;
        if !resp.starts_with('R') {
            return Err(MmError::LocallyDefined(format!(
                "Prior shutter error: {}",
                resp
            )));
        }
        self.is_open = open;
        Ok(())
    }
    fn get_open(&self) -> MmResult<bool> {
        let r = self.cmd(&format!("8,{}", self.id))?;
        Ok(r.trim() == "0")
    }
    fn fire(&mut self, _dt: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_closed() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .expect("8,1\r", "1")
            .expect("8,1\r", "1"); // state = closed
        let mut s = PriorShutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn initialize_open() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .expect("8,1\r", "0")
            .expect("8,1\r", "0"); // state = open
        let mut s = PriorShutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn open_close() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .expect("8,1\r", "1")
            .expect("8,1,0\r", "R")
            .expect("8,1\r", "0")
            .expect("8,1,1\r", "R")
            .expect("8,1\r", "1");
        let mut s = PriorShutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn fire_unsupported() {
        let mut s = PriorShutter::new(1);
        assert_eq!(s.fire(1.0).unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn no_transport_error() {
        assert!(PriorShutter::new(1).initialize().is_err());
    }
}
