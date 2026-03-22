use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

/// Toptica iBeam Smart CW laser controller.
///
/// Implements the `Shutter` trait: open = laser on (`la on`), closed = laser off (`la off`).
///
/// The iBeam Smart uses a multi-line protocol where each command returns multiple lines
/// terminated by `[OK]`. The adapter simplifies this by using `send_recv` which gets the
/// first response line (the mock transport supplies the relevant line directly).
pub struct IBeamSmartCW {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    power_mw: f64,
    max_power_mw: f64,
}

impl IBeamSmartCW {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("SerialID", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("FirmwareVersion", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("MaxPower_mW", PropertyValue::Float(0.0), true).unwrap();
        props.define_property("ClipStatus", PropertyValue::String("Unknown".into()), true).unwrap();
        props.define_property("LaserOperation", PropertyValue::String("Off".into()), false).unwrap();
        props.define_property("Power_mW", PropertyValue::Float(0.0), false).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            power_mw: 0.0,
            max_power_mw: 125.0,
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

    /// Send a command and return the trimmed response.
    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let cmd = command.to_string();
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }
}

impl Default for IBeamSmartCW {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for IBeamSmartCW {
    fn name(&self) -> &str {
        "iBeamSmartCW"
    }

    fn description(&self) -> &str {
        "Toptica iBeam smart laser in CW mode"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Disable prompt so we get clean responses
        self.cmd("prom off")?;

        // Get serial number (response contains "iBEAM-xxxx")
        if let Ok(serial) = self.cmd("id") {
            self.props.entry_mut("SerialID")
                .map(|e| e.value = PropertyValue::String(serial));
        }

        // Get firmware version (response contains "iB..." version string)
        if let Ok(ver) = self.cmd("ver") {
            self.props.entry_mut("FirmwareVersion")
                .map(|e| e.value = PropertyValue::String(ver));
        }

        // Get clip status
        if let Ok(clip) = self.cmd("sta clip") {
            self.props.entry_mut("ClipStatus")
                .map(|e| e.value = PropertyValue::String(clip));
        }

        // Get laser on/off status
        if let Ok(la) = self.cmd("sta la") {
            self.is_open = la.contains("ON");
            let label = if self.is_open { "On" } else { "Off" };
            self.props.entry_mut("LaserOperation")
                .map(|e| e.value = PropertyValue::String(label.into()));
        }

        // Get power level (response: "CH2, PWR: <f> mW")
        if let Ok(pow_resp) = self.cmd("sh level pow") {
            // Parse "CH2, PWR: 10.0 mW" pattern
            if let Some(pwr_pos) = pow_resp.find("PWR:") {
                let rest = &pow_resp[pwr_pos + 4..];
                let mw: f64 = rest.split_whitespace().next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                self.power_mw = mw;
                self.props.entry_mut("Power_mW")
                    .map(|e| e.value = PropertyValue::Float(mw));
            }
        }

        self.props.set_property_limits("Power_mW", 0.0, self.max_power_mw).ok();

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.cmd("la off");
            let _ = self.cmd("prom on");
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Power_mW" => Ok(PropertyValue::Float(self.power_mw)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Power_mW" => {
                let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.cmd(&format!("set pow {:.2}", mw))?;
                }
                self.power_mw = mw;
                self.props.entry_mut("Power_mW")
                    .map(|e| e.value = PropertyValue::Float(mw));
                Ok(())
            }
            "LaserOperation" => {
                let s = match &val {
                    PropertyValue::String(s) => s.clone(),
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                let open = s == "On";
                if self.initialized {
                    let cmd = if open { "la on" } else { "la off" };
                    self.cmd(cmd)?;
                    self.is_open = open;
                }
                self.props.set(name, PropertyValue::String(s))
            }
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
        DeviceType::Shutter
    }

    fn busy(&self) -> bool {
        false
    }
}

impl Shutter for IBeamSmartCW {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open { "la on" } else { "la off" };
        self.cmd(cmd)?;
        self.is_open = open;
        let label = if open { "On" } else { "Off" };
        self.props.entry_mut("LaserOperation")
            .map(|e| e.value = PropertyValue::String(label.into()));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.is_open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        self.set_open(true)?;
        self.set_open(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("prom off", "[OK]")
            .expect("id", "iBEAM-1234")
            .expect("ver", "iB-V2.0.1 [OK]")
            .expect("sta clip", "PASS")
            .expect("sta la", "OFF")
            .expect("sh level pow", "CH2, PWR: 0.0 mW")
    }

    #[test]
    fn initialize_reads_fields() {
        let mut dev = IBeamSmartCW::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(
            dev.get_property("SerialID").unwrap(),
            PropertyValue::String("iBEAM-1234".into())
        );
        assert_eq!(
            dev.get_property("ClipStatus").unwrap(),
            PropertyValue::String("PASS".into())
        );
    }

    #[test]
    fn open_close_laser() {
        let t = make_transport()
            .expect("la on", "[OK]")
            .expect("la off", "[OK]");
        let mut dev = IBeamSmartCW::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_power() {
        let t = make_transport().expect("set pow 50.00", "[OK]");
        let mut dev = IBeamSmartCW::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Power_mW", PropertyValue::Float(50.0)).unwrap();
        assert_eq!(dev.power_mw, 50.0);
    }

    #[test]
    fn no_transport_error() {
        let mut dev = IBeamSmartCW::new();
        assert!(dev.initialize().is_err());
    }
}
