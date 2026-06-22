use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};

/// MPB Communications Inc. laser controller.
///
/// Upstream registers this adapter as a Generic device. Laser diode on/off is
/// controlled by the `"Switch On/Off"` property.
///
/// The device prompt is `>` and every command echoes back a response line.
/// Laser states: 0=off, 1=on, 2=fault.
pub struct MpbLaser {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    power_setpoint: Cell<f64>,
    current_setpoint: Cell<i64>,
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
            power_setpoint: Cell::new(0.0),
            current_setpoint: Cell::new(0),
            power_min: 0.0,
            power_max: 100.0,
            current_min: 0.0,
            current_max: 0.0,
            laser_mode: "Constant Power".into(),
            ld_enable: "Off".into(),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(RefCell::new(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_ref() {
            Some(t) => f(t.borrow_mut().as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
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

    fn format_power_setpoint(setpoint: f64) -> String {
        if setpoint == 0.0 {
            return "0".into();
        }

        let abs = setpoint.abs();
        if !(1.0e-4..1.0e6).contains(&abs) {
            return Self::format_cpp_scientific(setpoint);
        }

        let decimals = (6_i32 - abs.log10().floor() as i32 - 1).max(0) as usize;
        let mut s = format!("{:.*}", decimals, setpoint);
        if s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        if abs >= 1.0 && s.chars().filter(|c| c.is_ascii_digit()).count() > 6 {
            return Self::format_cpp_scientific(setpoint);
        }
        s
    }

    fn format_cpp_scientific(setpoint: f64) -> String {
        let s = format!("{:.5e}", setpoint);
        let (mut mantissa, exponent) = s.split_once('e').unwrap_or((s.as_str(), "0"));
        while mantissa.ends_with('0') {
            mantissa = &mantissa[..mantissa.len() - 1];
        }
        if mantissa.ends_with('.') {
            mantissa = &mantissa[..mantissa.len() - 1];
        }
        let exp: i32 = exponent.parse().unwrap_or(0);
        format!("{mantissa}e{exp:+03}")
    }

    fn rounded_power_setpoint(setpoint: f64) -> f64 {
        Self::format_power_setpoint(setpoint)
            .parse()
            .unwrap_or(setpoint)
    }

    fn query_laser_mode(&self) -> MmResult<&'static str> {
        Ok(Self::mode_from_device(&self.cmd("getpowerenable")?))
    }

    fn query_ld_enable(&self) -> MmResult<&'static str> {
        match self.cmd("getldenable")?.trim() {
            "0" => Ok("Off"),
            "1" => Ok("On"),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn query_key_lock(&self) -> MmResult<&'static str> {
        match self.cmd("getinput 2")?.trim() {
            "1" => Ok("Off"),
            "0" => Ok("On"),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn query_state(&self) -> MmResult<&'static str> {
        let code = self
            .cmd("getlaserstate")?
            .parse::<i64>()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        Ok(Self::parse_laser_state(code))
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

        self.current_min = self
            .cmd("getldlim 1")?
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

        self.power_setpoint
            .set(self.cmd("getpower 0")?.parse().unwrap_or(0.0));
        self.props
            .entry_mut("Power Setpoint")
            .map(|e| e.value = PropertyValue::Float(self.power_setpoint.get()));

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
            "Switch On/Off" if self.initialized => {
                Ok(PropertyValue::String(self.query_ld_enable()?.into()))
            }
            "Set Laser Mode" if self.initialized => {
                Ok(PropertyValue::String(self.query_laser_mode()?.into()))
            }
            "Power Setpoint" if self.initialized => {
                let value = self
                    .cmd("getpower 0")?
                    .parse::<f64>()
                    .map_err(|_| MmError::SerialInvalidResponse)?;
                self.power_setpoint.set(value);
                Ok(PropertyValue::Float(value))
            }
            "Power Setpoint" => Ok(PropertyValue::Float(self.power_setpoint.get())),
            "Current Setpoint" => Ok(PropertyValue::Integer(self.current_setpoint.get())),
            "State" if self.initialized => Ok(PropertyValue::String(self.query_state()?.into())),
            "Key Lock Status" if self.initialized => {
                Ok(PropertyValue::String(self.query_key_lock()?.into()))
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "Power Setpoint" => {
                let p = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                let formatted = Self::format_power_setpoint(p);
                if self.initialized {
                    if self.query_laser_mode()? != "Constant Power" {
                        return Ok(());
                    }
                    self.cmd(&format!("setpower 0 {formatted}"))?;
                }
                let rounded = Self::rounded_power_setpoint(p);
                self.power_setpoint.set(rounded);
                self.props
                    .entry_mut("Power Setpoint")
                    .map(|e| e.value = PropertyValue::Float(rounded));
                Ok(())
            }
            "Current Setpoint" => {
                let current = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized && self.query_laser_mode()? != "Constant Current" {
                    return Ok(());
                }
                self.current_setpoint.set(current);
                self.props
                    .entry_mut("Current Setpoint")
                    .map(|e| e.value = PropertyValue::Integer(current));
                Ok(())
            }
            "Switch On/Off" => {
                let s = val.as_str().to_string();
                let cmd = Self::switch_command(&s)?;
                if self.initialized {
                    let live_state = self.query_ld_enable()?;
                    if s != live_state {
                        self.cmd(cmd)?;
                    }
                }
                self.ld_enable = s.clone();
                self.props.set(name, PropertyValue::String(s))
            }
            "Set Laser Mode" => {
                let s = val.as_str().to_string();
                let cmd = Self::mode_command(&s)?;
                if self.initialized {
                    let live_mode = self.query_laser_mode()?;
                    if s != live_mode {
                        self.cmd(cmd)?;
                    }
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
        if self.initialized {
            if name == "Power Setpoint" {
                return self
                    .query_laser_mode()
                    .map(|mode| mode != "Constant Power")
                    .unwrap_or(true);
            }
            if name == "Current Setpoint" {
                return self
                    .query_laser_mode()
                    .map(|mode| mode != "Constant Current")
                    .unwrap_or(true);
            }
        }
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
        let t = make_transport()
            .expect("getpowerenable", "1")
            .expect("getinput 2", "1");
        let mut laser = MpbLaser::new().with_transport(Box::new(t));
        laser.initialize().unwrap();
        assert_eq!(laser.device_type(), DeviceType::Generic);
        assert_eq!(laser.power_setpoint.get(), 50.0);
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
            .expect("getldenable", "0")
            .expect("setldenable 1", "1")
            .expect("getldenable", "1")
            .expect("getldenable", "1")
            .expect("setldenable 0", "0")
            .expect("getldenable", "0");
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
    fn switch_setter_compares_against_live_state() {
        let t = make_transport()
            .expect("getldenable", "1")
            .expect("setldenable 0", "0");
        let mut laser = MpbLaser::new().with_transport(Box::new(t));
        laser.initialize().unwrap();

        laser
            .set_property("Switch On/Off", PropertyValue::String("Off".into()))
            .unwrap();
        assert_eq!(laser.ld_enable, "Off");
    }

    #[test]
    fn mode_setter_compares_against_live_mode() {
        let t = make_transport()
            .expect("getpowerenable", "0")
            .expect("powerenable 1", "1");
        let mut laser = MpbLaser::new().with_transport(Box::new(t));
        laser.initialize().unwrap();

        laser
            .set_property(
                "Set Laser Mode",
                PropertyValue::String("Constant Power".into()),
            )
            .unwrap();
        assert_eq!(laser.laser_mode, "Constant Power");
    }

    #[test]
    fn set_power_setpoint() {
        let t = make_transport()
            .expect("getpowerenable", "1")
            .expect("setpower 0 75", "75.0");
        let mut laser = MpbLaser::new().with_transport(Box::new(t));
        laser.initialize().unwrap();
        laser
            .set_property("Power Setpoint", PropertyValue::Float(75.0))
            .unwrap();
        assert_eq!(laser.power_setpoint.get(), 75.0);
    }

    #[test]
    fn power_setpoint_command_format_matches_cpp_precision() {
        assert_eq!(MpbLaser::format_power_setpoint(75.0), "75");
        assert_eq!(MpbLaser::format_power_setpoint(12.345678), "12.3457");
        assert_eq!(MpbLaser::format_power_setpoint(0.1234567), "0.123457");
        assert_eq!(MpbLaser::format_power_setpoint(1_000_000.0), "1e+06");
        assert_eq!(MpbLaser::format_power_setpoint(0.00001), "1e-05");
        assert_eq!(MpbLaser::rounded_power_setpoint(12.345678), 12.3457);
    }

    #[test]
    fn before_get_properties_refresh_from_device() {
        let t = make_transport()
            .expect("getldenable", "1")
            .expect("getpowerenable", "0")
            .expect("getpower 0", "12.5")
            .expect("getlaserstate", "42")
            .expect("getinput 2", "0");
        let mut laser = MpbLaser::new().with_transport(Box::new(t));
        laser.initialize().unwrap();
        assert_eq!(
            laser.get_property("Switch On/Off").unwrap(),
            PropertyValue::String("On".into())
        );
        assert_eq!(
            laser.get_property("Set Laser Mode").unwrap(),
            PropertyValue::String("Constant Current".into())
        );
        assert_eq!(
            laser.get_property("Power Setpoint").unwrap(),
            PropertyValue::Float(12.5)
        );
        assert_eq!(
            laser.get_property("State").unwrap(),
            PropertyValue::String("Auto On".into())
        );
        assert_eq!(
            laser.get_property("Key Lock Status").unwrap(),
            PropertyValue::String("On".into())
        );
    }

    #[test]
    fn setpoint_read_only_tracks_live_mode() {
        let t = make_transport()
            .expect("getpowerenable", "1")
            .expect("getpowerenable", "1")
            .expect("getpowerenable", "0")
            .expect("getpowerenable", "0");
        let mut laser = MpbLaser::new().with_transport(Box::new(t));
        laser.initialize().unwrap();
        assert!(!laser.is_property_read_only("Power Setpoint"));
        assert!(laser.is_property_read_only("Current Setpoint"));
        assert!(laser.is_property_read_only("Power Setpoint"));
        assert!(!laser.is_property_read_only("Current Setpoint"));
    }

    #[test]
    fn setpoint_setters_obey_live_read_only_mode() {
        let t = make_transport()
            .expect("getpowerenable", "0")
            .expect("getpowerenable", "1");
        let mut laser = MpbLaser::new().with_transport(Box::new(t));
        laser.initialize().unwrap();

        laser
            .set_property("Power Setpoint", PropertyValue::Float(75.0))
            .unwrap();
        laser
            .set_property("Current Setpoint", PropertyValue::Integer(123))
            .unwrap();

        assert_eq!(laser.power_setpoint.get(), 50.0);
        assert_eq!(laser.current_setpoint.get(), 0);
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

    #[test]
    fn initialized_port_change_is_rejected() {
        let t = make_transport();
        let mut laser = MpbLaser::new().with_transport(Box::new(t));
        laser.initialize().unwrap();

        let err = laser
            .set_property("Port", PropertyValue::String("COM2".into()))
            .unwrap_err();
        assert_eq!(err, MmError::InvalidPropertyValue);
        assert_eq!(
            laser.get_property("Port").unwrap(),
            PropertyValue::String("Undefined".into())
        );
    }
}
