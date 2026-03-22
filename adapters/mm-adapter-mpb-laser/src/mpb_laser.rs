use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

/// MPB Communications Inc. laser controller.
///
/// Implements the `Shutter` trait: open = laser diode on (`setldenable 1`),
/// closed = laser diode off (`setldenable 0`).
///
/// The device prompt is `>` and every command echoes back a response line.
/// Laser states: 0=off, 1=on, 2=fault.
pub struct MpbLaser {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    power_setpoint: f64,
    power_min: f64,
    power_max: f64,
}

impl MpbLaser {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("SwitchOnOff", PropertyValue::String("Off".into()), false).unwrap();
        props.define_property("LaserMode", PropertyValue::String("APC".into()), false).unwrap();
        props.define_property("PowerSetpoint", PropertyValue::Float(0.0), false).unwrap();
        props.define_property("CurrentSetpoint", PropertyValue::Integer(0), false).unwrap();
        props.define_property("State", PropertyValue::String("Off".into()), true).unwrap();
        props.define_property("KeyLockStatus", PropertyValue::String("Unlocked".into()), true).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            power_setpoint: 0.0,
            power_min: 0.0,
            power_max: 100.0,
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

    fn parse_laser_state(code: i64) -> &'static str {
        match code {
            0 => "Off",
            1 => "On",
            2 => "Fault",
            _ => "Unknown",
        }
    }
}

impl Default for MpbLaser {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for MpbLaser {
    fn name(&self) -> &str {
        "MPBLaser"
    }

    fn description(&self) -> &str {
        "Unofficial device adapter for lasers from MPB Communications Inc."
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Query power setpoint limits
        if let Ok(lim) = self.cmd("getpowersetptlim 0") {
            let parts: Vec<&str> = lim.split_whitespace().collect();
            if parts.len() >= 2 {
                self.power_min = parts[0].parse().unwrap_or(0.0);
                self.power_max = parts[1].parse().unwrap_or(100.0);
            }
        }
        self.props.set_property_limits("PowerSetpoint", self.power_min, self.power_max).ok();

        // Query current power setpoint
        if let Ok(p) = self.cmd("getpower 0") {
            self.power_setpoint = p.parse().unwrap_or(0.0);
            self.props.entry_mut("PowerSetpoint")
                .map(|e| e.value = PropertyValue::Float(self.power_setpoint));
        }

        // Query laser diode enable state
        if let Ok(en) = self.cmd("getldenable") {
            self.is_open = en.trim() == "1";
            let label = if self.is_open { "On" } else { "Off" };
            self.props.entry_mut("SwitchOnOff")
                .map(|e| e.value = PropertyValue::String(label.into()));
        }

        // Query laser mode (APC=1, ACC=0)
        if let Ok(mode) = self.cmd("getpowerenable") {
            let mode_str = if mode.trim() == "1" { "APC" } else { "ACC" };
            self.props.entry_mut("LaserMode")
                .map(|e| e.value = PropertyValue::String(mode_str.into()));
        }

        // Query laser state
        if let Ok(sta) = self.cmd("getlaserstate") {
            let code: i64 = sta.parse().unwrap_or(0);
            let state = Self::parse_laser_state(code);
            self.props.entry_mut("State")
                .map(|e| e.value = PropertyValue::String(state.into()));
        }

        // Query key lock (getinput 2: 1=unlocked, 0=locked)
        if let Ok(kl) = self.cmd("getinput 2") {
            let status = if kl.trim() == "1" { "Unlocked" } else { "Locked" };
            self.props.entry_mut("KeyLockStatus")
                .map(|e| e.value = PropertyValue::String(status.into()));
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.cmd("setldenable 0");
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "PowerSetpoint" => Ok(PropertyValue::Float(self.power_setpoint)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "PowerSetpoint" => {
                let p = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.cmd(&format!("setpower 0 {:.6}", p))?;
                }
                self.power_setpoint = p;
                self.props.entry_mut("PowerSetpoint")
                    .map(|e| e.value = PropertyValue::Float(p));
                Ok(())
            }
            "SwitchOnOff" => {
                let s = match &val {
                    PropertyValue::String(s) => s.clone(),
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                let open = s == "On";
                if self.initialized {
                    let cmd = if open { "setldenable 1" } else { "setldenable 0" };
                    self.cmd(cmd)?;
                    self.is_open = open;
                }
                self.props.set(name, PropertyValue::String(s))
            }
            "LaserMode" => {
                let s = match &val {
                    PropertyValue::String(s) => s.clone(),
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                if self.initialized {
                    let cmd = if s == "APC" { "powerenable 1" } else { "powerenable 0" };
                    self.cmd(cmd)?;
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

impl Shutter for MpbLaser {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open { "setldenable 1" } else { "setldenable 0" };
        self.cmd(cmd)?;
        self.is_open = open;
        let label = if open { "On" } else { "Off" };
        self.props.entry_mut("SwitchOnOff")
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
            .expect("getpowersetptlim 0", "0.0 100.0")
            .expect("getpower 0", "50.0")
            .expect("getldenable", "0")
            .expect("getpowerenable", "1")
            .expect("getlaserstate", "0")
            .expect("getinput 2", "1")
    }

    #[test]
    fn initialize_reads_fields() {
        let mut laser = MpbLaser::new().with_transport(Box::new(make_transport()));
        laser.initialize().unwrap();
        assert!(!laser.get_open().unwrap());
        assert_eq!(laser.power_setpoint, 50.0);
        assert_eq!(laser.power_max, 100.0);
        assert_eq!(
            laser.get_property("LaserMode").unwrap(),
            PropertyValue::String("APC".into())
        );
        assert_eq!(
            laser.get_property("KeyLockStatus").unwrap(),
            PropertyValue::String("Unlocked".into())
        );
    }

    #[test]
    fn open_close_laser() {
        let t = make_transport()
            .expect("setldenable 1", "1")
            .expect("setldenable 0", "0");
        let mut laser = MpbLaser::new().with_transport(Box::new(t));
        laser.initialize().unwrap();
        laser.set_open(true).unwrap();
        assert!(laser.get_open().unwrap());
        laser.set_open(false).unwrap();
        assert!(!laser.get_open().unwrap());
    }

    #[test]
    fn set_power_setpoint() {
        let t = make_transport().expect("setpower 0 75.000000", "75.0");
        let mut laser = MpbLaser::new().with_transport(Box::new(t));
        laser.initialize().unwrap();
        laser.set_property("PowerSetpoint", PropertyValue::Float(75.0)).unwrap();
        assert_eq!(laser.power_setpoint, 75.0);
    }

    #[test]
    fn no_transport_error() {
        let mut laser = MpbLaser::new();
        assert!(laser.initialize().is_err());
    }
}
