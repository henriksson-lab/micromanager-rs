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
use std::time::{Duration, Instant};

pub struct PriorShutter {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    name: String,
    initialized: bool,
    id: u8,
    is_open: bool,
    delay_ms: f64,
    changed_time: Option<Instant>,
}

impl PriorShutter {
    pub fn new(id: u8) -> Self {
        Self::with_name(format!("Shutter-{}", id), id)
    }

    pub fn with_name(name: impl Into<String>, id: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            name: name.into(),
            initialized: false,
            id,
            is_open: false,
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

    fn mark_changed(&mut self) {
        self.changed_time = Some(Instant::now());
    }

    fn delay_duration(&self) -> Duration {
        Duration::from_secs_f64(self.delay_ms.max(0.0) / 1000.0)
    }

    fn ensure_runtime_properties(&mut self) -> MmResult<()> {
        if !self.props.has_property("State") {
            self.props
                .define_property("State", PropertyValue::Integer(0), false)?;
            self.props.set_allowed_values("State", &["0", "1"])?;
            self.props
                .define_property("Delay", PropertyValue::Float(self.delay_ms), false)?;
        }
        Ok(())
    }
}

impl Default for PriorShutter {
    fn default() -> Self {
        Self::new(1)
    }
}

impl Device for PriorShutter {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "Prior shutter adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        self.clear_port()?;
        self.cmd("COMP 0")?;
        self.ensure_runtime_properties()?;
        // Query initial state
        let r = self.cmd(&format!("8,{}", self.id))?;
        self.is_open = r.trim() == "0";
        self.mark_changed();
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
        match name {
            "State" if self.props.has_property("State") => {
                Ok(PropertyValue::Integer(if self.get_open()? { 1 } else { 0 }))
            }
            "Delay" if self.props.has_property("Delay") => Ok(PropertyValue::Float(self.delay_ms)),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "State" if self.props.has_property("State") => {
                let open = val.as_i64().ok_or(MmError::InvalidPropertyValue)? != 0;
                self.set_open(open)
            }
            "Delay" if self.props.has_property("Delay") => {
                self.delay_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.delay_ms))
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
        self.changed_time
            .map(|t| t.elapsed() < self.delay_duration())
            .unwrap_or(false)
    }
}

impl Shutter for PriorShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        // 0 = open, 1 = closed
        let val = if open { 0 } else { 1 };
        self.clear_port()?;
        let resp = self.cmd(&format!("8,{},{}", self.id, val))?;
        if !resp.starts_with('R') {
            return Err(MmError::LocallyDefined(format!(
                "Prior shutter error: {}",
                resp
            )));
        }
        self.is_open = open;
        self.mark_changed();
        Ok(())
    }
    fn get_open(&self) -> MmResult<bool> {
        self.clear_port()?;
        let r = self.cmd(&format!("8,{}", self.id))?;
        if r.starts_with('E') && r.len() > 2 {
            return Err(MmError::LocallyDefined(format!(
                "Prior shutter error: {}",
                r
            )));
        }
        if r.is_empty() {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(r.starts_with('0'))
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
    fn busy_uses_configured_delay_after_set_open() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .expect("8,1\r", "1")
            .expect("8,1,0\r", "R");
        let mut s = PriorShutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("Delay", PropertyValue::Float(1000.0))
            .unwrap();
        s.set_open(true).unwrap();
        assert!(s.busy());
    }

    #[test]
    fn fire_unsupported() {
        let mut s = PriorShutter::new(1);
        assert_eq!(s.fire(1.0).unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn upstream_identity_and_property_surface() {
        let mut s = PriorShutter::new(2);
        assert_eq!(s.name(), "Shutter-2");
        assert_eq!(s.description(), "Prior shutter adapter");
        assert!(s.has_property("Port"));
        assert!(!s.has_property("State"));
        assert!(!s.has_property("Delay"));
        assert!(!s.has_property("ShutterId"));
        assert!(s.get_property("State").is_err());
        assert!(s.set_property("Delay", PropertyValue::Float(1.0)).is_err());
    }

    #[test]
    fn initialize_creates_runtime_properties_like_upstream() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .expect("8,1\r", "1");
        let mut s = PriorShutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.has_property("State"));
        assert!(s.has_property("Delay"));
        let entry = s.props.entry("State").unwrap();
        assert_eq!(entry.allowed_values, vec!["0", "1"]);
    }

    #[test]
    fn get_open_rejects_empty_query_response_like_upstream() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .expect("8,1\r", "1")
            .expect("8,1\r", "");
        let mut s = PriorShutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_open(), Err(MmError::SerialInvalidResponse));
    }

    #[test]
    fn get_open_rejects_controller_error_query_response_like_upstream() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .expect("8,1\r", "1")
            .expect("8,1\r", "E,5");
        let mut s = PriorShutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(matches!(s.get_open(), Err(MmError::LocallyDefined(_))));
    }

    #[test]
    fn no_transport_error() {
        assert!(PriorShutter::new(1).initialize().is_err());
    }
}
