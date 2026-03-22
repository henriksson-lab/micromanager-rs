//! UC2 Hub — sends JSON commands over serial.

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Hub};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct Uc2Hub {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
}

impl Uc2Hub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();

        Self { props, transport: None, initialized: false }
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

    /// Send a JSON command and read the response line.
    pub fn send_json(&mut self, cmd: &str) -> MmResult<String> {
        self.call_transport(|t| Ok(t.send_recv(cmd)?.trim().to_string()))
    }
}

impl Default for Uc2Hub {
    fn default() -> Self { Self::new() }
}

impl Device for Uc2Hub {
    fn name(&self) -> &str { "UC2Hub" }
    fn description(&self) -> &str { "openUC2 hub device" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }

        // Optional firmware check — tolerate failure (firmware may still be valid)
        let _ = self.send_json(r#"{"task":"/state_get"}"#);

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
    fn device_type(&self) -> DeviceType { DeviceType::Hub }
    fn busy(&self) -> bool { false }
}

impl Hub for Uc2Hub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        Ok(vec![
            "UC2XYStage".into(),
            "UC2ZStage".into(),
            "UC2Shutter".into(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize_ok() {
        let t = MockTransport::new().any(r#"{"state":"ok"}"#);
        let mut hub = Uc2Hub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
    }
}
