/// Universal MM Hub Serial — configurable serial hub.
///
/// This hub loads device descriptions from the connected serial device on initialization.
/// The identification protocol uses:
///   `"UMMH\n"` → `"OK <device_count>\n"`  (handshake)
///   `"LIST\n"` → `"<name> <type>\n"` repeated, then `"END\n"`
///
/// Sub-devices are then registered dynamically.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Hub};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

/// Description of a sub-device discovered by the hub.
#[derive(Debug, Clone)]
pub struct SubDeviceInfo {
    pub name: String,
    pub device_type: String,
}

pub struct UniversalHub {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    sub_devices: Vec<SubDeviceInfo>,
}

impl UniversalHub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            sub_devices: Vec::new(),
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

    fn handshake(&mut self) -> MmResult<usize> {
        let resp = self.send_recv("UMMH\n")?;
        // Expected: "OK <count>"
        let parts: Vec<&str> = resp.split_whitespace().collect();
        if parts.is_empty() || parts[0] != "OK" {
            return Err(MmError::SerialInvalidResponse);
        }
        let count = if parts.len() > 1 {
            parts[1].parse::<usize>().unwrap_or(0)
        } else {
            0
        };
        Ok(count)
    }

    fn list_devices(&mut self) -> MmResult<Vec<SubDeviceInfo>> {
        // Send "LIST\n" — the response is the first device line (or "END")
        let first_line = self.send_recv("LIST\n")?;
        let mut devices = Vec::new();

        // Process all lines including the first returned by send_recv
        let mut line = first_line;
        loop {
            if line == "END" {
                break;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                devices.push(SubDeviceInfo {
                    name: parts[0].to_string(),
                    device_type: parts[1].to_string(),
                });
            }
            // Read next line
            line = self.call_transport(|t| Ok(t.receive_line()?.trim().to_string()))?;
        }
        Ok(devices)
    }

    pub fn sub_devices(&self) -> &[SubDeviceInfo] { &self.sub_devices }
}

impl Default for UniversalHub {
    fn default() -> Self { Self::new() }
}

impl Device for UniversalHub {
    fn name(&self) -> &str { "UniversalMMHubSerial" }
    fn description(&self) -> &str { "Universal hardware hub (serial)" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let _count = self.handshake()?;
        let devices = self.list_devices()?;
        self.sub_devices = devices;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
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
    fn device_type(&self) -> DeviceType { DeviceType::Hub }
    fn busy(&self) -> bool { false }
}

impl Hub for UniversalHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        Ok(self.sub_devices.iter().map(|d| d.name.clone()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn hub_initialize_with_devices() {
        // LIST\n returns "Shutter-1 Shutter" (first line via send_recv)
        // then receive_line returns "Filter-1 State", then "END"
        let t = MockTransport::new()
            .expect("UMMH\n", "OK 2")
            .expect("LIST\n", "Shutter-1 Shutter")
            .any("Filter-1 State")
            .any("END");
        let mut hub = UniversalHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert_eq!(hub.sub_devices().len(), 2);
        assert_eq!(hub.sub_devices()[0].name, "Shutter-1");
        assert_eq!(hub.sub_devices()[1].name, "Filter-1");
    }

    #[test]
    fn hub_empty_device_list() {
        // LIST\n returns "END" immediately (no devices)
        let t = MockTransport::new()
            .expect("UMMH\n", "OK 0")
            .expect("LIST\n", "END");
        let mut hub = UniversalHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert_eq!(hub.sub_devices().len(), 0);
    }

    #[test]
    fn hub_invalid_handshake() {
        let t = MockTransport::new()
            .expect("UMMH\n", "ERROR");
        let mut hub = UniversalHub::new().with_transport(Box::new(t));
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn no_transport_error() {
        let mut hub = UniversalHub::new();
        assert!(hub.initialize().is_err());
    }
}
