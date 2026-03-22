/// TriggerScope Hub.
///
/// ASCII serial protocol, `\n` terminated.
///   Identify: send `"*\n"`, recv firmware banner like `"ARC TRIGGERSCOPE 16 v1.2\n"`
///   Status:   send `"STAT?\n"`, recv status string
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Hub};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct TriggerScopeHub {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    firmware_version: String,
    is_ts16: bool,
}

impl TriggerScopeHub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            firmware_version: String::new(),
            is_ts16: false,
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

    fn send_recv(&mut self, cmd: &str) -> MmResult<String> {
        self.call_transport(|t| Ok(t.send_recv(cmd)?.trim().to_string()))
    }

    pub fn firmware_version(&self) -> &str { &self.firmware_version }
    pub fn is_ts16(&self) -> bool { self.is_ts16 }
}

impl Default for TriggerScopeHub {
    fn default() -> Self { Self::new() }
}

impl Device for TriggerScopeHub {
    fn name(&self) -> &str { "TriggerScope-Hub" }
    fn description(&self) -> &str { "ARC TriggerScope hub" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Send identification query
        let banner = self.send_recv("*\n")?;
        if !banner.contains("ARC TRIGGERSCOPE") && !banner.contains("ARC_LED") {
            return Err(MmError::SerialInvalidResponse);
        }
        self.is_ts16 = banner.contains("ARC TRIGGERSCOPE 16") || banner.contains("ARC_LED 16");
        self.firmware_version = banner.clone();
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "FirmwareVersion" => Ok(PropertyValue::String(self.firmware_version.clone())),
            "DACBits" => Ok(PropertyValue::Integer(if self.is_ts16 { 16 } else { 12 })),
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

impl Hub for TriggerScopeHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        let mut devs = Vec::new();
        for i in 1..=16 {
            devs.push(format!("TriggerScope-DAC{:02}", i));
            devs.push(format!("TriggerScope-TTL{:02}", i));
        }
        Ok(devs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize_ts16() {
        let t = MockTransport::new()
            .expect("*\n", "ARC TRIGGERSCOPE 16 v1.65");
        let mut hub = TriggerScopeHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert!(hub.is_ts16());
        assert!(hub.firmware_version().contains("v1.65"));
    }

    #[test]
    fn initialize_ts12() {
        let t = MockTransport::new()
            .expect("*\n", "ARC TRIGGERSCOPE v1.50");
        let mut hub = TriggerScopeHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert!(!hub.is_ts16());
    }

    #[test]
    fn invalid_banner_rejected() {
        let t = MockTransport::new()
            .expect("*\n", "UNKNOWN DEVICE v1.0");
        let mut hub = TriggerScopeHub::new().with_transport(Box::new(t));
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn no_transport_error() {
        let mut hub = TriggerScopeHub::new();
        assert!(hub.initialize().is_err());
    }
}
