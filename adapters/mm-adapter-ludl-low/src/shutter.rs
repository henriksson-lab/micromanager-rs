/// Ludl Low-level shutter adapter.
///
/// The Ludl Mac 2000/5000 uses a two-byte binary low-level command protocol.
/// Command bytes are preceded by 0xFF (escape byte) + command byte.
///
/// For the shutter the high-level ASCII equivalents sent by this adapter:
///   Open shutter:   `SO,<device_num>,1\r`  → `:A\r\n`
///   Close shutter:  `SO,<device_num>,0\r`  → `:A\r\n`
///   Query:          `SQ,<device_num>\r`    → `<state>\r\n`
///
/// Response `:A` = ack (ok); `:N` = nack (error).
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct LudlLowShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    device_num: u8,
    open: bool,
}

impl LudlLowShutter {
    pub fn new(device_num: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, transport: None, initialized: false, device_num, open: false }
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
        let c = format!("{}\r", command);
        self.call_transport(|t| { let r = t.send_recv(&c)?; Ok(r.trim().to_string()) })
    }

    fn check_ack(resp: &str) -> MmResult<()> {
        if resp.starts_with(":A") || resp == "A" {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("Ludl error: {}", resp)))
        }
    }
}

impl Default for LudlLowShutter { fn default() -> Self { Self::new(1) } }

impl Device for LudlLowShutter {
    fn name(&self) -> &str { "LudlLow-Shutter" }
    fn description(&self) -> &str { "Ludl Low-level shutter" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Query initial state
        let resp = self.cmd(&format!("SQ,{}", self.device_num))?;
        self.open = resp.trim() == "1";
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

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

impl Shutter for LudlLowShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let state = if open { 1 } else { 0 };
        let resp = self.cmd(&format!("SO,{},{}", self.device_num, state))?;
        Self::check_ack(&resp)?;
        self.open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.open) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        self.set_open(true)?;
        self.set_open(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize_closed() {
        let t = MockTransport::new().any("0");
        let mut s = LudlLowShutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn initialize_open() {
        let t = MockTransport::new().any("1");
        let mut s = LudlLowShutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn open_close() {
        let t = MockTransport::new().any("0").any(":A").any(":A");
        let mut s = LudlLowShutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn error_fails() {
        let t = MockTransport::new().any("0").any(":N-1");
        let mut s = LudlLowShutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_open(true).is_err());
    }

    #[test]
    fn no_transport_error() { assert!(LudlLowShutter::new(1).initialize().is_err()); }
}
