/// ASI FW1000 Shutter.
///
/// Protocol (TX `\n\r`):
///   `SO <n>\n\r`   → echo + "1"   open shutter n
///   `SC <n>\n\r`   → echo + "0"   close shutter n
///   `SQ <n>\n\r`   → state char   query shutter (1=open, 0=closed)
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct AsiFw1000Shutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    shutter_nr: u8,
    open: bool,
}

impl AsiFw1000Shutter {
    pub fn new(shutter_nr: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("ShutterNr", PropertyValue::Integer(shutter_nr as i64), false).unwrap();
        Self { props, transport: None, initialized: false, shutter_nr, open: false }
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
        let full = format!("{}\n\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }
}

impl Default for AsiFw1000Shutter { fn default() -> Self { Self::new(1) } }

impl Device for AsiFw1000Shutter {
    fn name(&self) -> &str { "AsiFw1000Shutter" }
    fn description(&self) -> &str { "ASI FW1000 Shutter" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Query shutter state
        let resp = self.cmd(&format!("SQ {}", self.shutter_nr))?;
        self.open = resp.trim_end().ends_with('1');
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.set_open(false);
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

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Shutter }
    fn busy(&self) -> bool { false }
}

impl Shutter for AsiFw1000Shutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open {
            format!("SO {}", self.shutter_nr)
        } else {
            format!("SC {}", self.shutter_nr)
        };
        self.cmd(&cmd)?;
        self.open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.open) }
    fn fire(&mut self, _delta_t: f64) -> MmResult<()> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize_closed() {
        let t = MockTransport::new().expect("SQ 1\n\r", "SQ 1 0");
        let mut s = AsiFw1000Shutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn open_close() {
        let t = MockTransport::new()
            .expect("SQ 1\n\r", "SQ 1 0")
            .expect("SO 1\n\r", "SO 1 1")
            .expect("SC 1\n\r", "SC 1 0");
        let mut s = AsiFw1000Shutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() { assert!(AsiFw1000Shutter::new(1).initialize().is_err()); }
}
