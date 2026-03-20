/// Ludl MAC5000/MAC6000 shutter.
///
/// Protocol (TX `\r`, RX `\n`):
///   `OPEN S<dev> <shutter>\r`  → `:A`
///   `CLOSE S<dev> <shutter>\r` → `:A`
///   `RDSTAT S<dev>\r`          → `:A <bitmask>` (bit N = shutter N open)
///
/// dev: device address (default 1); shutter: 1-indexed shutter number.
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Shutter};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

pub struct LudlShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    device: u8,
    shutter: u8,
    is_open: bool,
}

impl LudlShutter {
    pub fn new(device: u8, shutter: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("DeviceAddress", PropertyValue::Integer(device as i64), false).unwrap();
        props.define_property("ShutterIndex", PropertyValue::Integer(shutter as i64), false).unwrap();
        Self { props, transport: None, initialized: false, device, shutter, is_open: false }
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

    fn check_a(resp: &str) -> MmResult<&str> {
        let s = resp.trim();
        if let Some(rest) = s.strip_prefix(":A") { Ok(rest.trim()) }
        else { Err(MmError::LocallyDefined(format!("Ludl error: {}", s))) }
    }
}

impl Default for LudlShutter { fn default() -> Self { Self::new(1, 1) } }

impl Device for LudlShutter {
    fn name(&self) -> &str { "LudlShutter" }
    fn description(&self) -> &str { "Ludl MAC5000/MAC6000 shutter" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Read status bitmask
        let r = self.cmd(&format!("RDSTAT S{}", self.device))?;
        let body = Self::check_a(&r)?;
        let mask: u32 = body.trim().parse().unwrap_or(0);
        // bit (shutter-1) = open
        self.is_open = (mask >> (self.shutter - 1)) & 1 == 1;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.cmd(&format!("CLOSE S{} {}", self.device, self.shutter));
            self.is_open = false;
            self.initialized = false;
        }
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

impl Shutter for LudlShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open {
            format!("OPEN S{} {}", self.device, self.shutter)
        } else {
            format!("CLOSE S{} {}", self.device, self.shutter)
        };
        let r = self.cmd(&cmd)?;
        Self::check_a(&r)?;
        self.is_open = open;
        Ok(())
    }
    fn get_open(&self) -> MmResult<bool> { Ok(self.is_open) }
    fn fire(&mut self, _dt: f64) -> MmResult<()> { self.set_open(true) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    #[test]
    fn initialize_closed() {
        let t = MockTransport::new().any(":A 0"); // bitmask 0 = all closed
        let mut s = LudlShutter::new(1, 1).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn initialize_open() {
        let t = MockTransport::new().any(":A 1"); // bit 0 = shutter 1 open
        let mut s = LudlShutter::new(1, 1).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn open_close() {
        let t = MockTransport::new().any(":A 0").any(":A").any(":A");
        let mut s = LudlShutter::new(1, 1).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() { assert!(LudlShutter::new(1, 1).initialize().is_err()); }
}
