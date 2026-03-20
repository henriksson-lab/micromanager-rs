/// CARVII shutter.
///
/// Protocol (TX `\r`):
///   `S0\r`   → echo "S0\r"   close shutter
///   `S1\r`   → echo "S1\r"   open shutter
///   `rS\r`   → "rS<0|1>\r"  query state (response[2] = '0' or '1')
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Shutter};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

pub struct CarviiShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    open: bool,
}

impl CarviiShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, transport: None, initialized: false, open: false }
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
        let full = format!("{}\r", command);
        self.call_transport(|t| { let r = t.send_recv(&full)?; Ok(r.trim().to_string()) })
    }
}

impl Default for CarviiShutter { fn default() -> Self { Self::new() } }

impl Device for CarviiShutter {
    fn name(&self) -> &str { "CarviiShutter" }
    fn description(&self) -> &str { "CARVII Shutter" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let resp = self.cmd("rS")?;
        self.open = resp.as_bytes().get(2) == Some(&b'1');
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized { let _ = self.set_open(false); self.initialized = false; }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> { self.props.set(name, val) }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Shutter }
    fn busy(&self) -> bool { false }
}

impl Shutter for CarviiShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.cmd(if open { "S1" } else { "S0" })?;
        self.open = open;
        Ok(())
    }
    fn get_open(&self) -> MmResult<bool> { Ok(self.open) }
    fn fire(&mut self, _delta_t: f64) -> MmResult<()> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    #[test]
    fn initialize_closed() {
        let t = MockTransport::new().expect("rS\r", "rS0");
        let mut s = CarviiShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn open_close() {
        let t = MockTransport::new()
            .expect("rS\r", "rS0")
            .expect("S1\r", "S1")
            .expect("S0\r", "S0");
        let mut s = CarviiShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() { assert!(CarviiShutter::new().initialize().is_err()); }
}
