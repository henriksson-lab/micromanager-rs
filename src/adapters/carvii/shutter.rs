/// CARVII shutter.
///
/// Protocol (TX `\r`):
///   `S0\r`   → echo "S0\r"   close shutter
///   `S1\r`   → echo "S1\r"   open shutter
///   `rS\r`   → "rS<0|1>\r"  query state (response[2] = '0' or '1')
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};

pub struct CarviiShutter {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    open: Cell<bool>,
}

impl CarviiShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Name", PropertyValue::String("CARVII Shutter".into()), true)
            .unwrap();
        props
            .define_property(
                "Description",
                PropertyValue::String("CARVII Shutter".into()),
                true,
            )
            .unwrap();
        props
            .define_property("State", PropertyValue::String("Closed".into()), false)
            .unwrap();
        props
            .set_allowed_values("State", &["Closed", "Open"])
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
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

    fn cmd(&self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        self.call_transport(|t| {
            t.purge()?;
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    fn query_open(&self) -> MmResult<bool> {
        let resp = self.cmd("rS")?;
        let open = resp.as_bytes().get(2) == Some(&b'1');
        self.open.set(open);
        Ok(open)
    }
}

impl Default for CarviiShutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CarviiShutter {
    fn name(&self) -> &str {
        "CARVII Shutter"
    }
    fn description(&self) -> &str {
        "CARVII Shutter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        self.query_open()?;
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
            "State" => Ok(PropertyValue::String(
                if self.query_open()? { "Open" } else { "Closed" }.into(),
            )),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => match val.as_str() {
                "Open" => self.set_open(true),
                "Closed" => self.set_open(false),
                _ => Err(MmError::InvalidPropertyValue),
            },
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
        false
    }
}

impl Shutter for CarviiShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.cmd(if open { "S1" } else { "S0" })?;
        self.open.set(open);
        Ok(())
    }
    fn get_open(&self) -> MmResult<bool> {
        self.query_open()
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
        let t = MockTransport::new().expect("rS\r", "rS0");
        let mut s = CarviiShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.open.get());
        assert_eq!(
            s.get_property("Name").unwrap(),
            PropertyValue::String("CARVII Shutter".into())
        );
        assert_eq!(
            s.get_property("Description").unwrap(),
            PropertyValue::String("CARVII Shutter".into())
        );
    }

    #[test]
    fn open_close() {
        let t = MockTransport::new()
            .expect("rS\r", "rS0")
            .expect("S1\r", "S1")
            .expect("rS\r", "rS1")
            .expect("S0\r", "S0");
        let mut s = CarviiShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.open.get());
    }

    #[test]
    fn no_transport_error() {
        assert!(CarviiShutter::new().initialize().is_err());
    }

    #[test]
    fn fire_is_unsupported() {
        let mut s = CarviiShutter::new();
        assert_eq!(s.fire(1.0), Err(MmError::UnsupportedCommand));
    }

    #[test]
    fn shutdown_does_not_close_like_upstream() {
        let t = MockTransport::new()
            .expect("rS\r", "rS0")
            .expect("S1\r", "S1")
            .expect("rS\r", "rS1");
        let mut s = CarviiShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        s.shutdown().unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn get_property_state_queries_live_state() {
        let t = MockTransport::new()
            .expect("rS\r", "rS0")
            .expect("rS\r", "rS1");
        let mut s = CarviiShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.get_property("State").unwrap(),
            PropertyValue::String("Open".into())
        );
    }
}
