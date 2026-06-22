/// LaserQuantum Gem/Ventus/Opus/Axiom laser controller.
///
/// Text-based protocol, `\r` line termination.
///
/// Commands:
///   `VERSION?\r`     → version string (must contain "SMD12")
///   `STATUS?\r`      → "ENABLED" or "DISABLED"
///   `ON\r`           → turn laser on
///   `OFF\r`          → turn laser off
///   `CONTROL?\r`     → "POWER" or "CURRENT"
///   `CONTROL=POWER\r`/ `CONTROL=CURRENT\r`
///   `POWER?\r`       → e.g. "125.3mW"  (strip "mW")
///   `POWER=125.0\r`  → set power in mW
///   `CURRENT?\r`     → e.g. "45.5%"    (strip "%")
///   `CURRENT=45.0\r` → set current in %
///   `TIMERS?\r`      → 3 lines: "PSU Time = X Hours" / "Laser Enabled Time = X Hours" /
///                               "Laser Operation Time = X Hours" + empty line
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct LaserQuantumLaser {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    power_mw: f64,
    current_pct: f64,
    max_power_mw: f64,
    control_mode: String,
    current_control_enabled: bool,
    laser_temp_c: f64,
    psu_temp_c: f64,
}

impl LaserQuantumLaser {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Maximum power (mW)", PropertyValue::Float(500.0), false)
            .unwrap();
        props
            .define_property("Power (mW)", PropertyValue::Float(0.0), false)
            .unwrap();
        props.set_property_limits("Power (mW)", 0.0, 500.0).unwrap();
        props
            .define_property("Current (%)", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .set_property_limits("Current (%)", 0.0, 100.0)
            .unwrap();
        props
            .define_property("Control mode", PropertyValue::String("Power".into()), false)
            .unwrap();
        props
            .set_allowed_values("Control mode", &["Power", "Current"])
            .unwrap();
        props
            .define_property("Version", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property(
                "Laser Operation",
                PropertyValue::String("Off".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("Laser Operation", &["Off", "On"])
            .unwrap();
        props
            .define_property(
                "Current control",
                PropertyValue::String("Enabled".into()),
                true,
            )
            .unwrap();
        props
            .define_property("Temperature laser (C)", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("Temperature PSU (C)", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("Time PSU (h)", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("Time enabled (h)", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("Time operation (h)", PropertyValue::Float(0.0), true)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            power_mw: 0.0,
            current_pct: 0.0,
            max_power_mw: 500.0,
            control_mode: "Power".into(),
            current_control_enabled: true,
            laser_temp_c: 0.0,
            psu_temp_c: 0.0,
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
        let cmd = format!("{}\r", command);
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            let resp = resp.trim().to_string();
            if resp.to_ascii_uppercase().contains("ERROR") {
                return Err(MmError::LocallyDefined(format!(
                    "LaserQuantum returned error response: {resp}"
                )));
            }
            Ok(resp)
        })
    }

    /// Parse a numeric response that may have a trailing unit suffix (e.g. "125.3mW" → 125.3).
    fn parse_numeric(s: &str) -> f64 {
        // Strip non-numeric prefix/tail (timer labels, %, mW, C, etc.)
        let trimmed = s.trim();
        let start = trimmed
            .find(|c: char| c == '.' || c == '-' || c.is_ascii_digit())
            .unwrap_or(trimmed.len());
        let numeric = &trimmed[start..];
        let end = numeric
            .find(|c: char| c != '.' && c != '-' && !c.is_ascii_digit())
            .unwrap_or(numeric.len());
        numeric[..end].parse().unwrap_or(0.0)
    }

    fn set_control_mode_cache(&mut self, mode: &str) {
        self.control_mode = mode.into();
        self.props
            .entry_mut("Control mode")
            .map(|e| e.value = PropertyValue::String(mode.into()));

        let power_mode = mode == "Power";
        if let Some(entry) = self.props.entry_mut("Power (mW)") {
            entry.read_only = !power_mode;
        }
        if let Some(entry) = self.props.entry_mut("Current (%)") {
            entry.read_only = power_mode;
        }
    }

    fn set_current_control_enabled(&mut self, enabled: bool) {
        self.current_control_enabled = enabled;
        let label = if enabled { "Enabled" } else { "Disabled" };
        self.props
            .entry_mut("Current control")
            .map(|e| e.value = PropertyValue::String(label.into()));
        if !enabled {
            self.control_mode = "Power".into();
            self.props.entry_mut("Control mode").map(|e| {
                e.value = PropertyValue::String("Power".into());
                e.read_only = true;
            });
            if let Some(entry) = self.props.entry_mut("Current (%)") {
                entry.read_only = true;
            }
            if let Some(entry) = self.props.entry_mut("Power (mW)") {
                entry.read_only = false;
            }
        }
    }

    fn query_status(&mut self) -> MmResult<bool> {
        let status = self.cmd("STATUS?")?;
        let status = status.to_ascii_uppercase();
        if status.contains("ENABLED") {
            Ok(true)
        } else if status.contains("DISABLED") {
            Ok(false)
        } else {
            Err(MmError::SerialInvalidResponse)
        }
    }

    fn query_control_mode(&mut self) -> MmResult<&'static str> {
        let ctrl = self.cmd("CONTROL?")?;
        let ctrl = ctrl.to_ascii_uppercase();
        if ctrl.contains("POWER") {
            Ok("Power")
        } else if ctrl.contains("CURRENT") {
            Ok("Current")
        } else {
            Err(MmError::SerialInvalidResponse)
        }
    }

    fn supports_current_control(&mut self, initial_mode: &str) -> MmResult<bool> {
        if initial_mode == "Power" {
            self.cmd("CONTROL=CURRENT")?;
        }

        let current = match self.cmd("CURRENT?") {
            Ok(resp) => Self::parse_numeric(&resp),
            Err(err) => {
                if initial_mode == "Power" {
                    let _ = self.cmd("CONTROL=POWER");
                }
                return Err(err);
            }
        };

        match self.cmd(&format!("CURRENT={:.4}", current)) {
            Ok(_) => {
                if initial_mode == "Power" {
                    self.cmd("CONTROL=POWER")?;
                }
                Ok(true)
            }
            Err(MmError::LocallyDefined(message)) if message.contains("ERROR 67") => {
                if initial_mode == "Power" {
                    self.cmd("CONTROL=POWER")?;
                }
                Ok(false)
            }
            Err(err) => {
                if initial_mode == "Power" {
                    let _ = self.cmd("CONTROL=POWER");
                }
                Err(err)
            }
        }
    }
}

impl Default for LaserQuantumLaser {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for LaserQuantumLaser {
    fn name(&self) -> &str {
        "Laser"
    }
    fn description(&self) -> &str {
        "LaserQuantum laser"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        let ver = self.cmd("VERSION?")?;
        if !ver.contains("SMD12") {
            return Err(MmError::LocallyDefined(format!(
                "Unexpected version string: {}",
                ver
            )));
        }
        self.props
            .entry_mut("Version")
            .map(|e| e.value = PropertyValue::String(ver));

        self.is_open = self.query_status()?;
        let operation = if self.is_open { "On" } else { "Off" };
        self.props
            .entry_mut("Laser Operation")
            .map(|e| e.value = PropertyValue::String(operation.into()));

        let mode = self.query_control_mode()?;
        let current_control_enabled = self.supports_current_control(mode)?;
        self.set_control_mode_cache(mode);
        self.set_current_control_enabled(current_control_enabled);

        // Timers (4 responses: 3 data lines + empty)
        if let Ok(line1) = self.cmd("TIMERS?") {
            let psu = Self::parse_numeric(&line1);
            self.props
                .entry_mut("Time PSU (h)")
                .map(|e| e.value = PropertyValue::Float(psu));
            if let Ok(line2) = self.call_transport(|t| t.receive_line()) {
                let laser_enabled = Self::parse_numeric(&line2);
                self.props
                    .entry_mut("Time enabled (h)")
                    .map(|e| e.value = PropertyValue::Float(laser_enabled));
            }
            if let Ok(line3) = self.call_transport(|t| t.receive_line()) {
                let laser_op = Self::parse_numeric(&line3);
                self.props
                    .entry_mut("Time operation (h)")
                    .map(|e| e.value = PropertyValue::Float(laser_op));
            }
            let _ = self.call_transport(|t| t.receive_line());
        }

        // Current power and current
        if let Ok(p) = self.cmd("POWER?") {
            self.power_mw = Self::parse_numeric(&p);
            self.props
                .entry_mut("Power (mW)")
                .map(|e| e.value = PropertyValue::Float(self.power_mw));
        }
        if let Ok(c) = self.cmd("CURRENT?") {
            self.current_pct = Self::parse_numeric(&c);
            self.props
                .entry_mut("Current (%)")
                .map(|e| e.value = PropertyValue::Integer(self.current_pct as i64));
        }
        if let Ok(t) = self.cmd("LASTEMP?") {
            self.laser_temp_c = Self::parse_numeric(&t);
            self.props
                .entry_mut("Temperature laser (C)")
                .map(|e| e.value = PropertyValue::Float(self.laser_temp_c));
        }
        if let Ok(t) = self.cmd("PSUTEMP?") {
            self.psu_temp_c = Self::parse_numeric(&t);
            self.props
                .entry_mut("Temperature PSU (C)")
                .map(|e| e.value = PropertyValue::Float(self.psu_temp_c));
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.cmd("OFF");
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Power (mW)" => Ok(PropertyValue::Float(self.power_mw)),
            "Current (%)" => Ok(PropertyValue::Integer(self.current_pct as i64)),
            "Control mode" => Ok(PropertyValue::String(self.control_mode.clone())),
            "Temperature laser (C)" => Ok(PropertyValue::Float(self.laser_temp_c)),
            "Temperature PSU (C)" => Ok(PropertyValue::Float(self.psu_temp_c)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "Power (mW)" => {
                let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0.0..=self.max_power_mw).contains(&mw) {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized {
                    let live_mode = self.query_control_mode()?;
                    self.set_control_mode_cache(live_mode);
                    if live_mode != "Power" {
                        return Err(MmError::CanNotSetProperty);
                    }
                    self.cmd(&format!("POWER={:.4}", mw))?;
                } else if self.control_mode != "Power" {
                    return Err(MmError::CanNotSetProperty);
                }
                self.power_mw = mw;
                self.props
                    .entry_mut("Power (mW)")
                    .map(|e| e.value = PropertyValue::Float(mw));
                Ok(())
            }
            "Maximum power (mW)" => {
                let max_power = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if max_power < 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.max_power_mw = max_power;
                self.props
                    .set_property_limits("Power (mW)", 0.0, max_power)?;
                self.props.set(name, PropertyValue::Float(max_power))
            }
            "Current (%)" => {
                if !self.current_control_enabled {
                    return Err(MmError::CanNotSetProperty);
                }
                let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0.0..=100.0).contains(&pct) {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized {
                    let live_mode = self.query_control_mode()?;
                    self.set_control_mode_cache(live_mode);
                    if live_mode != "Current" {
                        return Err(MmError::CanNotSetProperty);
                    }
                    self.cmd(&format!("CURRENT={:.4}", pct))?;
                } else if self.control_mode != "Current" {
                    return Err(MmError::CanNotSetProperty);
                }
                self.current_pct = pct;
                self.props
                    .entry_mut("Current (%)")
                    .map(|e| e.value = PropertyValue::Integer(pct as i64));
                Ok(())
            }
            "Control mode" => {
                let mode = val.as_str().to_string();
                let cmd_mode = match mode.as_str() {
                    "Power" => "POWER",
                    "Current" if self.current_control_enabled => "CURRENT",
                    "Current" => return Err(MmError::CanNotSetProperty),
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                if self.initialized {
                    self.cmd(&format!("CONTROL={}", cmd_mode))?;
                }
                self.props.set(name, PropertyValue::String(mode.clone()))?;
                self.set_control_mode_cache(&mode);
                Ok(())
            }
            "Laser Operation" => {
                let operation = val.as_str().to_string();
                let open = match operation.as_str() {
                    "On" => true,
                    "Off" => false,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                if self.initialized {
                    let cmd = if open { "ON" } else { "OFF" };
                    self.cmd(cmd)?;
                }
                self.is_open = open;
                self.props.set(name, PropertyValue::String(operation))
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

impl Shutter for LaserQuantumLaser {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open { "ON" } else { "OFF" };
        self.cmd(cmd)?;
        self.is_open = open;
        let operation = if open { "On" } else { "Off" };
        self.props
            .entry_mut("Laser Operation")
            .map(|e| e.value = PropertyValue::String(operation.into()));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.is_open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        self.set_open(true)
    }
}

impl Generic for LaserQuantumLaser {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("VERSION?\r", "SMD12 v2.0")
            .expect("STATUS?\r", "DISABLED")
            .expect("CONTROL?\r", "POWER")
            .expect("CONTROL=CURRENT\r", "OK")
            .expect("CURRENT?\r", "30.0%")
            .expect("CURRENT=30.0000\r", "OK")
            .expect("CONTROL=POWER\r", "OK")
            // TIMERS?: 4 responses
            .expect("TIMERS?\r", "PSU Time = 100.5 Hours")
            .any("Laser Enabled Time = 50.2 Hours")
            .any("Laser Operation Time = 48.0 Hours")
            .any("")
            // POWER?, CURRENT?
            .expect("POWER?\r", "50.0mW")
            .expect("CURRENT?\r", "30.0%")
            .expect("LASTEMP?\r", "24.5C")
            .expect("PSUTEMP?\r", "31.0C")
    }

    #[test]
    fn initialize() {
        let mut dev = LaserQuantumLaser::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(dev.device_type(), DeviceType::Generic);
        assert_eq!(dev.power_mw, 50.0);
        assert_eq!(dev.current_pct, 30.0);
        assert_eq!(dev.name(), "Laser");
        assert_eq!(dev.description(), "LaserQuantum laser");
        assert_eq!(
            dev.get_property("Time PSU (h)").unwrap(),
            PropertyValue::Float(100.5)
        );
        assert_eq!(
            dev.get_property("Time enabled (h)").unwrap(),
            PropertyValue::Float(50.2)
        );
        assert_eq!(
            dev.get_property("Time operation (h)").unwrap(),
            PropertyValue::Float(48.0)
        );
        assert!(dev.has_property("Maximum power (mW)"));
        assert!(dev.has_property("Laser Operation"));
        assert!(dev.has_property("Current control"));
        assert_eq!(
            dev.get_property("Temperature laser (C)").unwrap(),
            PropertyValue::Float(24.5)
        );
        assert_eq!(
            dev.get_property("Temperature PSU (C)").unwrap(),
            PropertyValue::Float(31.0)
        );
        assert!(!dev.has_property("PowerSetpoint_mW"));
        assert!(!dev.has_property("Current_pct"));
        assert!(!dev.has_property("ControlMode"));
        assert_eq!(
            dev.get_property("Control mode").unwrap(),
            PropertyValue::String("Power".into())
        );
        assert!(!dev.is_property_read_only("Power (mW)"));
        assert!(dev.is_property_read_only("Current (%)"));
    }

    #[test]
    fn open_close() {
        let t = make_transport().expect("ON\r", "OK").expect("OFF\r", "OK");
        let mut dev = LaserQuantumLaser::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_power() {
        let t = make_transport()
            .expect("CONTROL?\r", "POWER")
            .expect("POWER=80.0000\r", "OK");
        let mut dev = LaserQuantumLaser::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Power (mW)", PropertyValue::Float(80.0))
            .unwrap();
        assert_eq!(dev.power_mw, 80.0);
    }

    #[test]
    fn set_power_rechecks_live_control_mode_before_command() {
        let t = make_transport().expect("CONTROL?\r", "CURRENT");
        let mut dev = LaserQuantumLaser::new().with_transport(Box::new(t));
        dev.initialize().unwrap();

        assert_eq!(
            dev.set_property("Power (mW)", PropertyValue::Float(80.0)),
            Err(MmError::CanNotSetProperty)
        );
        assert_eq!(dev.power_mw, 50.0);
        assert_eq!(
            dev.get_property("Control mode").unwrap(),
            PropertyValue::String("Current".into())
        );
    }

    #[test]
    fn set_current_rechecks_live_control_mode_before_command() {
        let t = make_transport()
            .expect("CONTROL=CURRENT\r", "OK")
            .expect("CONTROL?\r", "CURRENT")
            .expect("CURRENT=40.0000\r", "OK");
        let mut dev = LaserQuantumLaser::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Control mode", PropertyValue::String("Current".into()))
            .unwrap();
        dev.set_property("Current (%)", PropertyValue::Float(40.0))
            .unwrap();

        assert_eq!(dev.current_pct, 40.0);
    }

    #[test]
    fn control_mode_gates_power_and_current_setters() {
        let mut dev = LaserQuantumLaser::new();
        assert_eq!(
            dev.set_property("Current (%)", PropertyValue::Float(50.0)),
            Err(MmError::CanNotSetProperty)
        );
        dev.set_property("Control mode", PropertyValue::String("Current".into()))
            .unwrap();
        assert_eq!(
            dev.get_property("Control mode").unwrap(),
            PropertyValue::String("Current".into())
        );
        assert!(dev.is_property_read_only("Power (mW)"));
        assert!(!dev.is_property_read_only("Current (%)"));
        assert_eq!(
            dev.set_property("Power (mW)", PropertyValue::Float(50.0)),
            Err(MmError::CanNotSetProperty)
        );
        dev.set_property("Current (%)", PropertyValue::Float(50.0))
            .unwrap();
    }

    #[test]
    fn command_error_response_does_not_update_cached_setpoint() {
        let t = make_transport()
            .expect("CONTROL?\r", "POWER")
            .expect("POWER=80.0000\r", "ERROR 1");
        let mut dev = LaserQuantumLaser::new().with_transport(Box::new(t));
        dev.initialize().unwrap();

        assert!(dev
            .set_property("Power (mW)", PropertyValue::Float(80.0))
            .is_err());
        assert_eq!(dev.power_mw, 50.0);
    }

    #[test]
    fn maximum_power_limits_power_property() {
        let mut dev = LaserQuantumLaser::new();
        let entry = dev.props.entry("Power (mW)").unwrap();
        assert!(entry.has_limits);
        assert_eq!(entry.lower_limit, 0.0);
        assert_eq!(entry.upper_limit, 500.0);

        dev.set_property("Maximum power (mW)", PropertyValue::Float(75.0))
            .unwrap();
        let entry = dev.props.entry("Power (mW)").unwrap();
        assert_eq!(entry.upper_limit, 75.0);

        assert_eq!(
            dev.set_property("Power (mW)", PropertyValue::Float(80.0)),
            Err(MmError::InvalidPropertyValue)
        );
    }

    #[test]
    fn current_property_rejects_out_of_range_values() {
        let mut dev = LaserQuantumLaser::new();
        dev.set_control_mode_cache("Current");
        assert_eq!(
            dev.set_property("Current (%)", PropertyValue::Float(101.0)),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            dev.set_property("Current (%)", PropertyValue::Float(-1.0)),
            Err(MmError::InvalidPropertyValue)
        );
        dev.set_property("Current (%)", PropertyValue::Float(100.0))
            .unwrap();
    }

    #[test]
    fn parse_numeric_strips_units() {
        assert_eq!(LaserQuantumLaser::parse_numeric("125.3mW"), 125.3);
        assert_eq!(LaserQuantumLaser::parse_numeric("45.5%"), 45.5);
        assert_eq!(LaserQuantumLaser::parse_numeric("23.1C"), 23.1);
        assert_eq!(
            LaserQuantumLaser::parse_numeric("PSU Time = 100.5 Hours"),
            100.5
        );
    }

    #[test]
    fn no_transport_error() {
        let mut dev = LaserQuantumLaser::new();
        assert!(dev.initialize().is_err());
    }

    #[test]
    fn initialized_port_change_is_rejected() {
        let mut dev = LaserQuantumLaser::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert_eq!(
            dev.set_property("Port", PropertyValue::String("COM2".into())),
            Err(MmError::InvalidPropertyValue)
        );
    }

    #[test]
    fn unsupported_current_control_disables_current_mode() {
        let t = MockTransport::new()
            .expect("VERSION?\r", "SMD12 v2.0")
            .expect("STATUS?\r", "DISABLED")
            .expect("CONTROL?\r", "POWER")
            .expect("CONTROL=CURRENT\r", "OK")
            .expect("CURRENT?\r", "30.0%")
            .expect("CURRENT=30.0000\r", "ERROR 67")
            .expect("CONTROL=POWER\r", "OK")
            .expect("TIMERS?\r", "PSU Time = 100.5 Hours")
            .any("Laser Enabled Time = 50.2 Hours")
            .any("Laser Operation Time = 48.0 Hours")
            .any("")
            .expect("POWER?\r", "50.0mW")
            .expect("CURRENT?\r", "30.0%")
            .expect("LASTEMP?\r", "24.5C")
            .expect("PSUTEMP?\r", "31.0C");

        let mut dev = LaserQuantumLaser::new().with_transport(Box::new(t));
        dev.initialize().unwrap();

        assert_eq!(
            dev.get_property("Current control").unwrap(),
            PropertyValue::String("Disabled".into())
        );
        assert!(dev.is_property_read_only("Control mode"));
        assert!(dev.is_property_read_only("Current (%)"));
        assert_eq!(
            dev.set_property("Control mode", PropertyValue::String("Current".into())),
            Err(MmError::CanNotSetProperty)
        );
        assert_eq!(
            dev.set_property("Current (%)", PropertyValue::Float(50.0)),
            Err(MmError::CanNotSetProperty)
        );
    }
}
