/// Leica DMI scope hub.
///
/// ASCII serial protocol with device-based addressing.
/// Commands: `"<device><command_nr> <params>\r"`
/// Answers:  `"<device><command_nr> <result>\r"`
///
/// Master device address: "70" (g_Master = "70")
/// IL Turret:             "51"
/// Objective Turret:      "76"
/// TL Shutter:            "78"
/// IL Shutter:            "79"
///
/// Stand info command: `"7000\r"` → `"7000 <version> <available_devices>\r"`
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Hub};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct LeicaDMIHub {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    stand_version: String,
}

impl LeicaDMIHub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            stand_version: String::new(),
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

    /// Get stand info: device "70", command 0.
    fn get_stand_info(&mut self) -> MmResult<String> {
        let resp = self.send_recv("7000\r")?;
        // Response: "7000 <firmware_version>"
        if !resp.starts_with("7000") {
            return Err(MmError::SerialInvalidResponse);
        }
        let info = resp[4..].trim().to_string();
        Ok(info)
    }

    pub fn stand_version(&self) -> &str { &self.stand_version }
}

impl Default for LeicaDMIHub {
    fn default() -> Self { Self::new() }
}

impl Device for LeicaDMIHub {
    fn name(&self) -> &str { "LeicaDMI-Hub" }
    fn description(&self) -> &str { "Leica DMI microscope hub" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let version = self.get_stand_info()?;
        self.stand_version = version;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "StandVersion" => Ok(PropertyValue::String(self.stand_version.clone())),
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

impl Hub for LeicaDMIHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        Ok(vec![
            "LeicaDMI-TLShutter".to_string(),
            "LeicaDMI-ILShutter".to_string(),
            "LeicaDMI-ILTurret".to_string(),
            "LeicaDMI-ObjectiveTurret".to_string(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn hub_initialize() {
        let t = MockTransport::new()
            .expect("7000\r", "7000 v3.2.0");
        let mut hub = LeicaDMIHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert!(hub.stand_version().contains("v3.2.0"));
    }

    #[test]
    fn no_transport_error() {
        let mut hub = LeicaDMIHub::new();
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn detect_installed_devices() {
        let t = MockTransport::new()
            .expect("7000\r", "7000 v3.2.0");
        let mut hub = LeicaDMIHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        let devs = hub.detect_installed_devices().unwrap();
        assert!(devs.contains(&"LeicaDMI-TLShutter".to_string()));
        assert!(devs.contains(&"LeicaDMI-ILTurret".to_string()));
    }
}
