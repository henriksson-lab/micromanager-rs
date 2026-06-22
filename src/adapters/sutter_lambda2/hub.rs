/// Sutter Lambda 2 / Lambda 10-3 Hub.
///
/// Binary protocol:
///   Go online:          send `[0xEE]`, await echo + CR
///   Get controller ID:  send `[0xFD]`, await text reply + CR
///   Get status:         send `[0xCC]`, await status bytes + CR
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Hub};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct Lambda2Hub {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    controller_type: String,
    controller_id: String,
    motors_enabled: bool,
}

impl Lambda2Hub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "Motors Enabled",
                PropertyValue::String("True".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("Motors Enabled", &["True", "False"])
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            controller_type: "10-2".into(),
            controller_id: String::new(),
            motors_enabled: true,
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

    pub fn controller_type(&self) -> &str {
        &self.controller_type
    }

    fn set_motors_enabled(&mut self, enabled: bool) -> MmResult<()> {
        let cmd = if enabled { 0xCF } else { 0xCE };
        self.call_transport(|t| {
            t.send_bytes(&[cmd])?;
            let resp = t.receive_bytes(2)?;
            if resp.len() != 2 || resp[0] != cmd || resp[1] != 0x0D {
                return Err(MmError::SerialInvalidResponse);
            }
            Ok(())
        })?;
        self.motors_enabled = enabled;
        self.props.set(
            "Motors Enabled",
            PropertyValue::String(if enabled { "True" } else { "False" }.into()),
        )
    }
}

impl Default for Lambda2Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Lambda2Hub {
    fn name(&self) -> &str {
        "SutterHub"
    }
    fn description(&self) -> &str {
        "Sutter Lambda 2 controller hub"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.go_online()?;
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
            "ControllerID" => Ok(PropertyValue::String(self.controller_id.clone())),
            "Motors Enabled" => Ok(PropertyValue::String(
                if self.motors_enabled { "True" } else { "False" }.into(),
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Motors Enabled" => match val.as_str() {
                "True" => self.set_motors_enabled(true),
                "False" => self.set_motors_enabled(false),
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
        DeviceType::Hub
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Hub for Lambda2Hub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        Ok(vec![
            "Wheel-A".to_string(),
            "Wheel-B".to_string(),
            "Wheel-C".to_string(),
            "Shutter-A".to_string(),
            "Shutter-B".to_string(),
            "VF-5".to_string(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn hub_initialize() {
        // go-online: send 0xEE, recv [0xEE, 0x0D]
        let t = MockTransport::new().expect_binary(&[0xEE, 0x0D]);
        let mut hub = Lambda2Hub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert_eq!(hub.controller_type(), "10-2");
    }

    #[test]
    fn hub_no_transport() {
        let mut hub = Lambda2Hub::new();
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn detect_installed_devices() {
        let t = MockTransport::new().expect_binary(&[0xEE, 0x0D]);
        let mut hub = Lambda2Hub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        let devs = hub.detect_installed_devices().unwrap();
        assert_eq!(
            devs,
            vec![
                "Wheel-A".to_string(),
                "Wheel-B".to_string(),
                "Wheel-C".to_string(),
                "Shutter-A".to_string(),
                "Shutter-B".to_string(),
                "VF-5".to_string(),
            ]
        );
    }

    #[test]
    fn motors_enabled_property_sends_upstream_commands() {
        let t = MockTransport::new()
            .expect_binary(&[0xEE, 0x0D])
            .expect_binary(&[0xCE, 0x0D])
            .expect_binary(&[0xCF, 0x0D]);
        let mut hub = Lambda2Hub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        hub.set_property("Motors Enabled", PropertyValue::String("False".into()))
            .unwrap();
        assert_eq!(
            hub.get_property("Motors Enabled").unwrap(),
            PropertyValue::String("False".into())
        );
        hub.set_property("Motors Enabled", PropertyValue::String("True".into()))
            .unwrap();
        assert_eq!(
            hub.get_property("Motors Enabled").unwrap(),
            PropertyValue::String("True".into())
        );
    }

    #[test]
    fn invalid_motors_enabled_value_rejected_without_transport() {
        let mut hub = Lambda2Hub::new();
        assert_eq!(
            hub.set_property("Motors Enabled", PropertyValue::String("Maybe".into())),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            hub.get_property("Motors Enabled").unwrap(),
            PropertyValue::String("True".into())
        );
    }
}
