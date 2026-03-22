/// Leica DMR scope hub.
///
/// ASCII serial protocol, `\r` terminated.
/// Command format: `"<DD><CCC>\r"` where DD = 2-digit device ID, CCC = 3-digit command.
/// With data:      `"<DD><CCC><data>\r"`
/// Response:       `"<DD><CCC><data>\r"`
///
/// Device IDs used:
///   gMic_          = 0  (microscope body)
///   lamp_          = 10 (halogen lamp)
///   zDrive_        = 16 (z drive)
///   objNosepiece_  = 20 (objective nosepiece)
///   rLFA4_         = 8  (4-position RL filter turret)
///   rLFA8_         = 9  (8-position RL filter turret)
///
/// Key commands:
///   Get firmware version:  `"00025\r"` → `"00025<version>\r"`
///   Get microscope type:   `"00026\r"` → `"00026<type>\r"`
///   Check presence:        `"<DD>001\r"` → `"<DD>001<1_or_0>\r"`
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Hub};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct LeicaDMRHub {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    version: String,
    microscope_type: String,
}

impl LeicaDMRHub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            version: String::new(),
            microscope_type: String::new(),
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

    /// Get command: `"<DD><CCC>\r"` → data portion of response.
    pub fn get_command(&mut self, device_id: u8, command: u16) -> MmResult<String> {
        let cmd = format!("{:02}{:03}\r", device_id, command);
        let resp = self.send_recv(&cmd)?;
        let prefix = format!("{:02}{:03}", device_id, command);
        if !resp.starts_with(&prefix) {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(resp[prefix.len()..].trim().to_string())
    }

    /// Set command with integer data.
    pub fn set_command(&mut self, device_id: u8, command: u16, data: i32) -> MmResult<()> {
        let cmd = format!("{:02}{:03}{}\r", device_id, command, data);
        let resp = self.send_recv(&cmd)?;
        let prefix = format!("{:02}{:03}", device_id, command);
        if !resp.starts_with(&prefix) {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(())
    }

    pub fn version(&self) -> &str { &self.version }
    pub fn microscope_type(&self) -> &str { &self.microscope_type }
}

impl Default for LeicaDMRHub {
    fn default() -> Self { Self::new() }
}

impl Device for LeicaDMRHub {
    fn name(&self) -> &str { "LeicaDMR-Hub" }
    fn description(&self) -> &str { "Leica DMR upright microscope hub" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Get firmware version (device 0, command 25)
        let version = self.get_command(0, 25)?;
        self.version = version;
        // Get microscope type (device 0, command 26)
        let micro_type = self.get_command(0, 26).unwrap_or_else(|_| "DMRXE".to_string());
        self.microscope_type = micro_type.trim().to_string();
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "FirmwareVersion" => Ok(PropertyValue::String(self.version.clone())),
            "MicroscopeType"  => Ok(PropertyValue::String(self.microscope_type.clone())),
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

impl Hub for LeicaDMRHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        Ok(vec![
            "LeicaDMR-RLShutter".to_string(),
            "LeicaDMR-Lamp".to_string(),
            "LeicaDMR-RLModule".to_string(),
            "LeicaDMR-ObjNosepiece".to_string(),
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
            .expect("00025\r", "00025v3.0")
            .expect("00026\r", "00026DMRXA");
        let mut hub = LeicaDMRHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert!(hub.version().contains("v3.0"));
        assert!(hub.microscope_type().contains("DMRXA"));
    }

    #[test]
    fn no_transport_error() {
        let mut hub = LeicaDMRHub::new();
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn get_set_command() {
        let t = MockTransport::new()
            .expect("00025\r", "00025v2.0")
            .expect("00026\r", "00026DMRA")
            .expect("10010\r", "10010 75");  // lamp intensity query
        let mut hub = LeicaDMRHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        let lamp_int = hub.get_command(10, 10).unwrap();
        assert!(lamp_int.contains("75"));
    }
}
