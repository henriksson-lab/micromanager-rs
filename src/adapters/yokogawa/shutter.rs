/// Yokogawa CSU-X1 shutter.
///
/// Protocol:
///   `SHO\r`    → `A`           open shutter
///   `SHC\r`    → `A`           close shutter
///   `SH, ?\r`  → `OPEN\rA` or `CLOSED\rA`  query state
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};
use std::time::Instant;

pub struct CsuXShutter {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    open: Cell<bool>,
    delay_ms: f64,
    changed_time: Cell<Instant>,
}

impl CsuXShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("State", PropertyValue::String("Closed".into()), false)
            .unwrap();
        props
            .set_allowed_values("State", &["Closed", "Open"])
            .unwrap();
        props
            .define_property("Delay", PropertyValue::Float(0.0), false)
            .unwrap();
        props.set_property_limits("Delay", 0.0, f64::MAX).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            open: Cell::new(false),
            delay_ms: 0.0,
            changed_time: Cell::new(Instant::now()),
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
        let full = format!("{}\r", command);
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }

    fn query_open(&self) -> MmResult<bool> {
        let _resp = self.cmd("SH, ?")?;
        // Upstream CSUXHub::GetShutterPosition reads the answer but then
        // unconditionally assigns the open state.
        let open = true;
        self.open.set(open);
        Ok(open)
    }

    fn check_ack(resp: &str) -> MmResult<()> {
        match resp.trim_end().chars().last() {
            Some('A') => Ok(()),
            Some('N') => Err(MmError::LocallyDefined(format!(
                "CSU-X shutter NAK: {}",
                resp
            ))),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }
}

impl Default for CsuXShutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CsuXShutter {
    fn name(&self) -> &str {
        "CSUX-Shutter"
    }
    fn description(&self) -> &str {
        "CSUX Shutter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let open = self.query_open()?;
        self.props.set(
            "State",
            PropertyValue::String(if open { "Open" } else { "Closed" }.into()),
        )?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::String(
                if self.get_open()? { "Open" } else { "Closed" }.into(),
            )),
            "Delay" => Ok(PropertyValue::Float(self.delay_ms)),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "State" => {
                let state = val.as_str();
                if state == "Open" {
                    self.set_open(true)
                } else if state == "Closed" {
                    self.set_open(false)
                } else {
                    Err(MmError::InvalidPropertyValue)
                }
            }
            "Delay" => {
                let delay = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if delay < 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.delay_ms = delay;
                self.props.set(name, PropertyValue::Float(delay))
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
        self.changed_time.get().elapsed().as_secs_f64() * 1000.0 < self.delay_ms
    }
}

impl Shutter for CsuXShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open { "SHO" } else { "SHC" };
        let resp = self.cmd(cmd)?;
        Self::check_ack(&resp)?;
        self.open.set(open);
        self.changed_time.set(Instant::now());
        self.props.set(
            "State",
            PropertyValue::String(if open { "Open" } else { "Closed" }.into()),
        )?;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        if self.initialized && self.transport.is_some() {
            self.query_open()
        } else {
            Ok(self.open.get())
        }
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
    fn initialize_query_matches_upstream_open_fallthrough() {
        let t = MockTransport::new()
            .expect("SH, ?\r", "CLOSED\rA")
            .expect("SH, ?\r", "CLOSED\rA");
        let mut s = CsuXShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn open_close() {
        let t = MockTransport::new()
            .expect("SH, ?\r", "CLOSED\rA")
            .expect("SHO\r", "A")
            .expect("SH, ?\r", "OPEN\rA")
            .expect("SHC\r", "A")
            .expect("SH, ?\r", "CLOSED\rA");
        let mut s = CsuXShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() {
        assert!(CsuXShutter::new().initialize().is_err());
    }

    #[test]
    fn fire_is_unsupported_like_upstream() {
        let t = MockTransport::new().expect("SH, ?\r", "CLOSED\rA");
        let mut s = CsuXShutter::new().with_transport(Box::new(t));
        assert!(s.has_property("State"));
        s.initialize().unwrap();
        assert_eq!(s.fire(1.0).unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn state_property_uses_upstream_labels() {
        let t = MockTransport::new()
            .expect("SH, ?\r", "CLOSED\rA")
            .expect("SHO\r", "A")
            .expect("SH, ?\r", "OPEN\rA");
        let mut s = CsuXShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("State", PropertyValue::String("Open".into()))
            .unwrap();
        assert_eq!(
            s.get_property("State").unwrap(),
            PropertyValue::String("Open".into())
        );
    }

    #[test]
    fn busy_uses_delay_timer_after_set_open() {
        let t = MockTransport::new()
            .expect("SH, ?\r", "CLOSED\rA")
            .expect("SHO\r", "A");
        let mut s = CsuXShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("Delay", PropertyValue::Float(40.0)).unwrap();
        s.set_open(true).unwrap();
        assert!(s.busy());
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(!s.busy());
    }

    #[test]
    fn negative_delay_is_rejected() {
        let mut s = CsuXShutter::new();
        assert_eq!(
            s.set_property("Delay", PropertyValue::Float(-1.0)),
            Err(MmError::InvalidPropertyValue)
        );
    }

    #[test]
    fn port_is_locked_after_initialize() {
        let t = MockTransport::new().expect("SH, ?\r", "CLOSED\rA");
        let mut s = CsuXShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.set_property("Port", PropertyValue::String("COM2".into())),
            Err(MmError::InvalidPropertyValue)
        );
    }

    #[test]
    fn set_open_requires_ack_like_upstream_hub() {
        let t = MockTransport::new()
            .expect("SH, ?\r", "CLOSED\rA")
            .expect("SHO\r", "OK");
        let mut s = CsuXShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.set_open(true), Err(MmError::SerialInvalidResponse));
        assert!(s.open.get());
    }
}
