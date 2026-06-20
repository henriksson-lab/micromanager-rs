use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

/// MPB Communications Inc. laser controller.
///
/// Upstream registers this adapter as a Generic device. Laser diode on/off is
/// controlled by the `"Switch On/Off"` property.
///
/// The device prompt is `>` and every command echoes back a response line.
/// Laser states: 0=off, 1=on, 2=fault.
pub struct MpbLaser {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    power_setpoint: f64,
    current_setpoint: i64,
    power_min: f64,
    power_max: f64,
    current_min: f64,
    current_max: f64,
    laser_mode: String,
    ld_enable: String,
}

impl MpbLaser {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_property("Switch On/Off", PropertyValue::String("Off".into()), false)
            .unwrap();
        props
            .set_allowed_values("Switch On/Off", &["Off", "On"])
            .unwrap();
        props
            .define_property(
                "Set Laser Mode",
                PropertyValue::String("Constant Power".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("Set Laser Mode", &["Constant Power", "Constant Current"])
            .unwrap();
        props
            .define_property("Power Setpoint", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("Current Setpoint", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("State", PropertyValue::String("Off".into()), true)
            .unwrap();
        props
            .define_property("Key Lock Status", PropertyValue::String("Off".into()), true)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            power_setpoint: 0.0,
            current_setpoint: 0,
            power_min: 0.0,
            power_max: 100.0,
            current_min: 0.0,
            current_max: 0.0,
            laser_mode: "Constant Power".into(),
            ld_enable: "Off".into(),
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
            6 => "Key Lock",
            7 => "Interlock",
            8 => "Fault",
            20 => "Startup",
            31 => "Manual Turning On",
            41 => "Manual On",
            42 => "Auto On",
            _ => "Unknown",
        }
    }

    fn mode_from_device(value: &str) -> &'static str {
        if value.trim() == "0" {
            "Constant Current"
        } else {
            "Constant Power"
        }
    }

    fn mode_command(value: &str) -> MmResult<&'static str> {
        match value {
            "Constant Current" => Ok("powerenable 0"),
            "Constant Power" => Ok("powerenable 1"),
            _ => Err(MmError::InvalidPropertyValue),
        }
    }

    fn switch_command(value: &str) -> MmResult<&'static str> {
        match value {
            "Off" => Ok("setldenable 0"),
            "On" => Ok("setldenable 1"),
            _ => Err(MmError::InvalidPropertyValue),
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

        let lim = self.cmd("getpowersetptlim 0")?;
        let parts: Vec<&str> = lim.split_whitespace().collect();
        if parts.len() >= 2 {
            self.power_min = parts[0].parse().unwrap_or(0.0);
            self.power_max = parts[1].parse().unwrap_or(100.0);
        }
        self.props
            .set_property_limits("Power Setpoint", self.power_min, self.power_max)
            .ok();

        self.current_min = self.cmd("getldlim 1")?
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        self.current_max = self.cmd("getacccurmax")?.parse().unwrap_or(0.0);
        self.props
            .set_property_limits("Current Setpoint", self.current_min, self.current_max)
            .ok();

        self.ld_enable = if self.cmd("getldenable")?.trim() == "1" {
            "On".into()
        } else {
            "Off".into()
        };
        self.props
            .entry_mut("Switch On/Off")
            .map(|e| e.value = PropertyValue::String(self.ld_enable.clone()));

        self.laser_mode = Self::mode_from_device(&self.cmd("getpowerenable")?).into();
        self.props
            .entry_mut("Set Laser Mode")
            .map(|e| e.value = PropertyValue::String(self.laser_mode.clone()));

        self.power_setpoint = self.cmd("getpower 0")?.parse().unwrap_or(0.0);
        self.props
            .entry_mut("Power Setpoint")
            .map(|e| e.value = PropertyValue::Float(self.power_setpoint));

        let code: i64 = self.cmd("getlaserstate")?.parse().unwrap_or(0);
        let state = Self::parse_laser_state(code);
        self.props
            .entry_mut("State")
            .map(|e| e.value = PropertyValue::String(state.into()));

        let status = if self.cmd("getinput 2")?.trim() == "1" {
            "Off"
        } else {
            "On"
        };
        self.props
            .entry_mut("Key Lock Status")
            .map(|e| e.value = PropertyValue::String(status.into()));

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Power Setpoint" => Ok(PropertyValue::Float(self.power_setpoint)),
            "Current Setpoint" => Ok(PropertyValue::Integer(self.current_setpoint)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Power Setpoint" => {
                let p = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.cmd(&format!("setpower 0 {:.6}", p))?;
                }
                self.power_setpoint = p;
                self.props
                    .entry_mut("Power Setpoint")
                    .map(|e| e.value = PropertyValue::Float(p));
                Ok(())
            }
            "Current Setpoint" => {
                let current = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.current_setpoint = current;
                self.props
                    .entry_mut("Current Setpoint")
                    .map(|e| e.value = PropertyValue::Integer(current));
                Ok(())
            }
            "Switch On/Off" => {
                let s = val.as_str().to_string();
                let cmd = Self::switch_command(&s)?;
                if self.initialized && s != self.ld_enable {
                    self.cmd(cmd)?;
                }
                self.ld_enable = s.clone();
                self.props.set(name, PropertyValue::String(s))
            }
            "Set Laser Mode" => {
                let s = val.as_str().to_string();
                let cmd = Self::mode_command(&s)?;
                if self.initialized && s != self.laser_mode {
                    self.cmd(cmd)?;
                }
                self.laser_mode = s.clone();
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
        DeviceType::Generic
    }

    fn busy(&self) -> bool {
        false
    }
}

impl Generic for MpbLaser {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("getpowersetptlim 0", "0.0 100.0")
            .expect("getldlim 1", "0 500")
            .expect("getacccurmax", "500")
            .expect("getldenable", "0")
            .expect("getpowerenable", "1")
            .expect("getpower 0", "50.0")
            .expect("getlaserstate", "0")
            .expect("getinput 2", "1")
    }

    #[test]
    fn initialize_reads_fields() {
        let mut laser = MpbLaser::new().with_transport(Box::new(make_transport()));
        laser.initialize().unwrap();
        assert_eq!(laser.device_type(), DeviceType::Generic);
        assert_eq!(laser.power_setpoint, 50.0);
        assert_eq!(laser.power_max, 100.0);
        assert_eq!(
            laser.get_property("Set Laser Mode").unwrap(),
            PropertyValue::String("Constant Power".into())
        );
        assert_eq!(
            laser.get_property("Key Lock Status").unwrap(),
            PropertyValue::String("Off".into())
        );
    }

    #[test]
    fn switch_property_controls_laser() {
        let t = make_transport()
            .expect("setldenable 1", "1")
            .expect("setldenable 0", "0");
        let mut laser = MpbLaser::new().with_transport(Box::new(t));
        laser.initialize().unwrap();
        laser
            .set_property("Switch On/Off", PropertyValue::String("On".into()))
            .unwrap();
        assert_eq!(
            laser.get_property("Switch On/Off").unwrap(),
            PropertyValue::String("On".into())
        );
        laser
            .set_property("Switch On/Off", PropertyValue::String("Off".into()))
            .unwrap();
        assert_eq!(
            laser.get_property("Switch On/Off").unwrap(),
            PropertyValue::String("Off".into())
        );
    }

    #[test]
    fn set_power_setpoint() {
        let t = make_transport().expect("setpower 0 75.000000", "75.0");
        let mut laser = MpbLaser::new().with_transport(Box::new(t));
        laser.initialize().unwrap();
        laser
            .set_property("Power Setpoint", PropertyValue::Float(75.0))
            .unwrap();
        assert_eq!(laser.power_setpoint, 75.0);
    }

    #[test]
    fn laser_state_labels_match_upstream_codes() {
        assert_eq!(MpbLaser::parse_laser_state(0), "Off");
        assert_eq!(MpbLaser::parse_laser_state(6), "Key Lock");
        assert_eq!(MpbLaser::parse_laser_state(7), "Interlock");
        assert_eq!(MpbLaser::parse_laser_state(8), "Fault");
        assert_eq!(MpbLaser::parse_laser_state(20), "Startup");
        assert_eq!(MpbLaser::parse_laser_state(31), "Manual Turning On");
        assert_eq!(MpbLaser::parse_laser_state(41), "Manual On");
        assert_eq!(MpbLaser::parse_laser_state(42), "Auto On");
    }

    #[test]
    fn no_transport_error() {
        let mut laser = MpbLaser::new();
        assert!(laser.initialize().is_err());
    }
}
