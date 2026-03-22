/// Leica DMI shutter (TL or IL).
///
/// Protocol (ASCII, `\r` terminated):
///   TL Shutter device address: "78"
///   IL Shutter device address: "79"
///
///   Open shutter:   `"<dev>32 1\r"` → `"<dev>32 1\r"`
///   Close shutter:  `"<dev>32 0\r"` → `"<dev>32 0\r"`
///   Get state:      `"<dev>33\r"`   → `"<dev>33 <0|1>\r"`
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutterType {
    TransmittedLight,   // device address "78"
    IncidentLight,      // device address "79"
}

pub struct LeicaDMIShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    shutter_type: ShutterType,
    is_open: bool,
}

impl LeicaDMIShutter {
    pub fn new(shutter_type: ShutterType) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        let type_str = match shutter_type {
            ShutterType::TransmittedLight => "TL",
            ShutterType::IncidentLight    => "IL",
        };
        props.define_property("ShutterType", PropertyValue::String(type_str.into()), true).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            shutter_type,
            is_open: false,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn device_addr(&self) -> &'static str {
        match self.shutter_type {
            ShutterType::TransmittedLight => "78",
            ShutterType::IncidentLight    => "79",
        }
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn send_recv(&mut self, cmd: &str) -> MmResult<String> {
        self.call_transport(|t| Ok(t.send_recv(cmd)?.trim().to_string()))
    }

    fn send_open(&mut self, open: bool) -> MmResult<()> {
        let dev = self.device_addr();
        let val = if open { 1 } else { 0 };
        let cmd = format!("{}32 {}\r", dev, val);
        let resp = self.send_recv(&cmd)?;
        let expected_prefix = format!("{}32", dev);
        if !resp.starts_with(&expected_prefix) {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(())
    }

    pub fn query_state(&mut self) -> MmResult<bool> {
        let dev = self.device_addr();
        let cmd = format!("{}33\r", dev);
        let resp = self.send_recv(&cmd)?;
        let prefix = format!("{}33", dev);
        if !resp.starts_with(&prefix) {
            return Err(MmError::SerialInvalidResponse);
        }
        let val: &str = resp[prefix.len()..].trim();
        Ok(val == "1")
    }
}

impl Device for LeicaDMIShutter {
    fn name(&self) -> &str { "LeicaDMIShutter" }
    fn description(&self) -> &str { "Leica DMI shutter" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.send_open(false)?;
        self.is_open = false;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send_open(false);
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

impl Shutter for LeicaDMIShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.send_open(open)?;
        self.is_open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.is_open) }

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
    fn tl_shutter_open_close() {
        let t = MockTransport::new()
            .expect("7832 0\r", "7832 0")   // init close
            .expect("7832 1\r", "7832 1")   // open
            .expect("7832 0\r", "7832 0");  // close
        let mut s = LeicaDMIShutter::new(ShutterType::TransmittedLight).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn il_shutter_open_close() {
        let t = MockTransport::new()
            .expect("7932 0\r", "7932 0")
            .expect("7932 1\r", "7932 1");
        let mut s = LeicaDMIShutter::new(ShutterType::IncidentLight).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn fire_opens_closes() {
        let t = MockTransport::new()
            .expect("7832 0\r", "7832 0")   // init
            .expect("7832 1\r", "7832 1")   // fire open
            .expect("7832 0\r", "7832 0");  // fire close
        let mut s = LeicaDMIShutter::new(ShutterType::TransmittedLight).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.fire(5.0).unwrap();
        assert!(!s.get_open().unwrap());
    }
}
