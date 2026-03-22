use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

/// Cobolt Official laser controller.
///
/// Implements the `Shutter` trait: open = laser on (`l1`), closed = laser off (`l0`).
/// This is the official Cobolt adapter that works with all Cobolt laser series.
pub struct CoboltOfficialLaser {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    power_setpoint_mw: f64,
}

impl CoboltOfficialLaser {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("None".into()), false).unwrap();
        props.define_property("SerialNumber", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("FirmwareVersion", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("UsageHours", PropertyValue::Float(0.0), true).unwrap();
        props.define_property("PowerSetpoint_mW", PropertyValue::Float(0.0), false).unwrap();
        props.set_property_limits("PowerSetpoint_mW", 0.0, 1000.0).unwrap();
        props.define_property("PowerReadback_mW", PropertyValue::Float(0.0), true).unwrap();
        props.define_property("LaserState", PropertyValue::String("Off".into()), false).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            power_setpoint_mw: 0.0,
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

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let cmd = command.to_string();
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }
}

impl Default for CoboltOfficialLaser {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CoboltOfficialLaser {
    fn name(&self) -> &str {
        "Cobolt Laser"
    }

    fn description(&self) -> &str {
        "Official device adapter for Cobolt lasers."
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Query serial number
        if let Ok(sn) = self.cmd("sn?") {
            self.props.entry_mut("SerialNumber")
                .map(|e| e.value = PropertyValue::String(sn));
        }

        // Query firmware version
        if let Ok(ver) = self.cmd("ver?") {
            self.props.entry_mut("FirmwareVersion")
                .map(|e| e.value = PropertyValue::String(ver));
        }

        // Query usage hours
        if let Ok(hrs) = self.cmd("hrs?") {
            let h = hrs.parse::<f64>().unwrap_or(0.0);
            self.props.entry_mut("UsageHours")
                .map(|e| e.value = PropertyValue::Float(h));
        }

        // Query current laser on/off state
        if let Ok(state) = self.cmd("l?") {
            self.is_open = state.trim() == "1";
            let label = if self.is_open { "On" } else { "Off" };
            self.props.entry_mut("LaserState")
                .map(|e| e.value = PropertyValue::String(label.into()));
        }

        // Query power setpoint
        if let Ok(sp) = self.cmd("glp?") {
            if let Ok(mw) = sp.parse::<f64>() {
                self.power_setpoint_mw = mw;
                self.props.entry_mut("PowerSetpoint_mW")
                    .map(|e| e.value = PropertyValue::Float(mw));
            }
        }

        // Query power readback
        if let Ok(p) = self.cmd("p?") {
            if let Ok(mw) = p.parse::<f64>() {
                self.props.entry_mut("PowerReadback_mW")
                    .map(|e| e.value = PropertyValue::Float(mw));
            }
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.cmd("l0");
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "PowerSetpoint_mW" => Ok(PropertyValue::Float(self.power_setpoint_mw)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "PowerSetpoint_mW" => {
                let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    let resp = self.cmd(&format!("slp {:.4}", mw))?;
                    if resp != "OK" {
                        return Err(MmError::SerialInvalidResponse);
                    }
                }
                self.power_setpoint_mw = mw;
                self.props.entry_mut("PowerSetpoint_mW")
                    .map(|e| e.value = PropertyValue::Float(mw));
                Ok(())
            }
            "LaserState" => {
                let s = match &val {
                    PropertyValue::String(s) => s.clone(),
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                let open = s == "On";
                if self.initialized {
                    let cmd = if open { "l1" } else { "l0" };
                    let resp = self.cmd(cmd)?;
                    if resp != "OK" {
                        return Err(MmError::SerialInvalidResponse);
                    }
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

impl Shutter for CoboltOfficialLaser {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open { "l1" } else { "l0" };
        let resp = self.cmd(cmd)?;
        if resp != "OK" {
            return Err(MmError::SerialInvalidResponse);
        }
        self.is_open = open;
        let label = if open { "On" } else { "Off" };
        self.props.entry_mut("LaserState")
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
            .expect("sn?", "ABC-12345")
            .expect("ver?", "2.0.1")
            .expect("hrs?", "100.0")
            .expect("l?", "0")
            .expect("glp?", "50.0")
            .expect("p?", "0.0")
    }

    #[test]
    fn initialize_reads_fields() {
        let mut laser = CoboltOfficialLaser::new().with_transport(Box::new(make_transport()));
        laser.initialize().unwrap();
        assert!(!laser.get_open().unwrap());
        assert_eq!(laser.power_setpoint_mw, 50.0);
        assert_eq!(
            laser.get_property("SerialNumber").unwrap(),
            PropertyValue::String("ABC-12345".into())
        );
    }

    #[test]
    fn open_close_laser() {
        let t = make_transport()
            .expect("l1", "OK")
            .expect("l0", "OK");
        let mut laser = CoboltOfficialLaser::new().with_transport(Box::new(t));
        laser.initialize().unwrap();
        laser.set_open(true).unwrap();
        assert!(laser.get_open().unwrap());
        laser.set_open(false).unwrap();
        assert!(!laser.get_open().unwrap());
    }

    #[test]
    fn set_power_setpoint() {
        let t = make_transport().expect("slp 75.0000", "OK");
        let mut laser = CoboltOfficialLaser::new().with_transport(Box::new(t));
        laser.initialize().unwrap();
        laser.set_property("PowerSetpoint_mW", PropertyValue::Float(75.0)).unwrap();
        assert_eq!(laser.power_setpoint_mw, 75.0);
    }

    #[test]
    fn no_transport_error() {
        let mut laser = CoboltOfficialLaser::new();
        assert!(laser.initialize().is_err());
    }
}
