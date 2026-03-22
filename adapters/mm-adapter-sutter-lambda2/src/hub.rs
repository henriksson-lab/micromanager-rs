/// Sutter Lambda 2 / Lambda 10-3 Hub.
///
/// Binary protocol:
///   Go online:          send `[0xEE]`, await echo + CR
///   Get controller ID:  send `[0xFD]`, await text reply + CR
///   Get status:         send `[0xCC]`, await status bytes + CR
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Hub};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct Lambda2Hub {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    controller_type: String,
    controller_id: String,
}

impl Lambda2Hub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            controller_type: "10-2".into(),
            controller_id: String::new(),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
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

    /// Send [0xEE] go-online command; controller echoes [0xEE] + CR.
    fn go_online(&mut self) -> MmResult<()> {
        self.call_transport(|t| {
            t.send_bytes(&[0xEE])?;
            let resp = t.receive_bytes(2)?;
            if resp.first() != Some(&0xEE) {
                return Err(MmError::SerialInvalidResponse);
            }
            Ok(())
        })
    }

    /// Query controller type with [0xFD]; returns text like "SC\r" or "10-3 vX.Y\r".
    fn query_controller_type(&mut self) -> MmResult<(String, String)> {
        self.call_transport(|t| {
            t.send_bytes(&[0xFD])?;
            let ans = t.receive_line()?;
            let ans = ans.trim().to_string();
            let ctrl_type = if ans.starts_with("SC") {
                "SC".to_string()
            } else if ans.starts_with("10-3") {
                "10-3".to_string()
            } else {
                "10-2".to_string()
            };
            Ok((ctrl_type, ans))
        })
    }

    pub fn controller_type(&self) -> &str { &self.controller_type }
}

impl Default for Lambda2Hub {
    fn default() -> Self { Self::new() }
}

impl Device for Lambda2Hub {
    fn name(&self) -> &str { "Lambda2Hub" }
    fn description(&self) -> &str { "Sutter Lambda 2 controller hub" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.go_online()?;
        let (ctype, cid) = self.query_controller_type()?;
        self.controller_type = ctype;
        self.controller_id = cid;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "ControllerType" => Ok(PropertyValue::String(self.controller_type.clone())),
            "ControllerID"   => Ok(PropertyValue::String(self.controller_id.clone())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        self.props.set(name, val)
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Hub }
    fn busy(&self) -> bool { false }
}

impl Hub for Lambda2Hub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        Ok(vec![
            "Lambda2-Wheel-A".to_string(),
            "Lambda2-Wheel-B".to_string(),
            "Lambda2-Shutter-A".to_string(),
            "Lambda2-Shutter-B".to_string(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn hub_initialize() {
        // go-online: send 0xEE, recv [0xEE, 0x0D]
        // query type: send 0xFD, recv "10-3 v1.0\r"
        let t = MockTransport::new()
            .expect_binary(&[0xEE, 0x0D])
            .any("10-3 v1.0");
        let mut hub = Lambda2Hub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert_eq!(hub.controller_type(), "10-3");
    }

    #[test]
    fn hub_no_transport() {
        let mut hub = Lambda2Hub::new();
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn detect_installed_devices() {
        let t = MockTransport::new()
            .expect_binary(&[0xEE, 0x0D])
            .any("SC");
        let mut hub = Lambda2Hub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        let devs = hub.detect_installed_devices().unwrap();
        assert!(devs.contains(&"Lambda2-Shutter-A".to_string()));
        assert!(devs.contains(&"Lambda2-Wheel-A".to_string()));
    }
}
