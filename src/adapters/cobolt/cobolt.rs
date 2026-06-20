use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};
use std::thread;
use std::time::Duration;

/// Cobolt / HÜBNER Photonics laser controller.
///
/// Implements the `Shutter` trait: open = laser on, closed = laser off.
/// Also exposes power setpoint and readback as properties.
pub struct CoboltLaser {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    is_open: Cell<bool>,
    power_setpoint_mw: f64,
    power_maximum_mw: f64,
    current_ma: f64,
    current_maximum_ma: f64,
    current_modulation_minimum_ma: f64,
    current_modulation_maximum_ma: f64,
    control_mode: String,
    autostart_status: String,
    serial_response: RefCell<String>,
    serial_number: String,
    model: String,
    firmware_version: String,
    hours: String,
}

impl CoboltLaser {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("PowerSetpoint_mW", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("PowerSetpoint_mW", 0.0, 1000.0)
            .unwrap();
        props
            .define_property("PowerReadback_mW", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("SerialNumber", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property(
                "FirmwareVersion",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("UsageHours", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("KeyStatus", PropertyValue::String("Off".into()), true)
            .unwrap();
        props
            .define_property("FaultCode", PropertyValue::String("0".into()), true)
            .unwrap();
        props
            .define_property("Interlock", PropertyValue::String("0".into()), true)
            .unwrap();
        props
            .define_property("Laser", PropertyValue::String("Off".into()), false)
            .unwrap();
        props.set_allowed_values("Laser", &["Off", "On"]).unwrap();
        props
            .define_property("Hours", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Key On/Off", PropertyValue::String("Off".into()), true)
            .unwrap();
        props
            .define_property("Fault", PropertyValue::String("No Fault".into()), true)
            .unwrap();
        props
            .define_property("Serial Number", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Model", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property(
                "Firmware Version",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Serial Command",
                PropertyValue::String(String::new()),
                false,
            )
            .unwrap();
        props
            .define_property(
                "Serial Command Response",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("Autostart", PropertyValue::String("Disabled".into()), false)
            .unwrap();
        props
            .set_allowed_values("Autostart", &["Enabled", "Disabled"])
            .unwrap();
        props
            .define_property(
                "Autostart Status",
                PropertyValue::String("Disabled".into()),
                true,
            )
            .unwrap();
        props
            .define_property("Power", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("Power Status", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("Power Setpoint", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("Power Maximum", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("Current", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("Current Status", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property(
                "Current Setpoint",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("Current Maximum", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property(
                "Current Modulation Minimum",
                PropertyValue::Float(0.0),
                false,
            )
            .unwrap();
        props
            .define_property(
                "Current Modulation Maximum",
                PropertyValue::Float(0.0),
                false,
            )
            .unwrap();
        props
            .define_property(
                "Control Mode",
                PropertyValue::String("Constant Power".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values(
                "Control Mode",
                &["Constant Power", "Constant Current", "Modulation"],
            )
            .unwrap();
        props
            .define_property("Modulation", PropertyValue::String("Disabled".into()), true)
            .unwrap();
        props
            .define_property(
                "Modulation Analog",
                PropertyValue::String("Disabled".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("Modulation Analog", &["Enabled", "Disabled"])
            .unwrap();
        props
            .define_property(
                "Modulation Digital ",
                PropertyValue::String("Disabled".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("Modulation Digital ", &["Enabled", "Disabled"])
            .unwrap();
        props
            .define_property(
                "Operating Status",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("Wavelength", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Laser Type", PropertyValue::String(String::new()), true)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: Cell::new(false),
            power_setpoint_mw: 0.0,
            power_maximum_mw: 0.0,
            current_ma: 0.0,
            current_maximum_ma: 0.0,
            current_modulation_minimum_ma: 0.0,
            current_modulation_maximum_ma: 0.0,
            control_mode: "Constant Power".into(),
            autostart_status: "Disabled".into(),
            serial_response: RefCell::new(String::new()),
            serial_number: String::new(),
            model: String::new(),
            firmware_version: String::new(),
            hours: String::new(),
        }
    }

    /// Inject a transport (serial port or mock).
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

    /// Send a command and return the trimmed response line.
    fn cmd(&self, command: &str) -> MmResult<String> {
        let cmd = command.to_string();
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }

    #[allow(dead_code)]
    fn refresh_power_readback(&self) -> MmResult<f64> {
        let resp = self.cmd("p?")?;
        resp.parse::<f64>()
            .map_err(|_| MmError::SerialInvalidResponse)
    }

    fn send_power_setpoint(&mut self, mw: f64) -> MmResult<()> {
        let cmd = format!("p {:.4}", mw / 1000.0);
        let resp = self.cmd(&cmd)?;
        if resp != "OK" {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(())
    }

    fn parse_watts_to_mw(&self, command: &str) -> MmResult<f64> {
        Ok(self
            .cmd(command)?
            .parse::<f64>()
            .map_err(|_| MmError::SerialInvalidResponse)?
            * 1000.0)
    }

    fn parse_float_command(&self, command: &str) -> MmResult<f64> {
        self.cmd(command)?
            .parse::<f64>()
            .map_err(|_| MmError::SerialInvalidResponse)
    }

    fn bool_status(response: &str, yes: &str, no: &str) -> MmResult<String> {
        match response.chars().next() {
            Some('0') => Ok(no.into()),
            Some('1') => Ok(yes.into()),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn refresh_autostart_status(&mut self) -> MmResult<String> {
        let status = Self::bool_status(&self.cmd("@cobas?")?, "Enabled", "Disabled")?;
        self.autostart_status = status.clone();
        Ok(status)
    }

    fn set_control_mode_name(&mut self, mode: &str) -> MmResult<()> {
        let cmd = match mode {
            "Constant Power" => "cp",
            "Constant Current" => "ci",
            "Modulation" => "em",
            _ => return Err(MmError::InvalidPropertyValue),
        };
        if self.initialized {
            let _ = self.cmd(cmd)?;
        }
        self.control_mode = mode.into();
        self.props
            .entry_mut("Control Mode")
            .map(|e| e.value = PropertyValue::String(mode.into()));
        self.props.entry_mut("Modulation").map(|e| {
            e.value = PropertyValue::String(if mode == "Modulation" {
                "Enabled".into()
            } else {
                "Disabled".into()
            })
        });
        Ok(())
    }
}

impl Default for CoboltLaser {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CoboltLaser {
    fn name(&self) -> &str {
        "Cobolt"
    }

    fn description(&self) -> &str {
        "Cobolt Controller by Karl Bellvé with contribution from Alexis Maizel"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Query identification fields
        let sn = self.cmd("sn?")?;
        self.serial_number = sn.clone();
        self.props
            .entry_mut("SerialNumber")
            .map(|e| e.value = PropertyValue::String(sn));
        self.props
            .entry_mut("Serial Number")
            .map(|e| e.value = PropertyValue::String(self.serial_number.clone()));

        if let Ok(model) = self.cmd("glm?") {
            self.model = model.clone();
            self.props
                .entry_mut("Model")
                .map(|e| e.value = PropertyValue::String(model.clone()));
            self.props
                .entry_mut("Laser Type")
                .map(|e| e.value = PropertyValue::String(model.chars().take(3).collect()));
        }

        let ver = self.cmd("ver?")?;
        self.firmware_version = ver.clone();
        self.props
            .entry_mut("FirmwareVersion")
            .map(|e| e.value = PropertyValue::String(ver));
        self.props
            .entry_mut("Firmware Version")
            .map(|e| e.value = PropertyValue::String(self.firmware_version.clone()));

        let hrs = self.cmd("hrs?")?;
        self.hours = hrs.clone();
        self.props
            .entry_mut("UsageHours")
            .map(|e| e.value = PropertyValue::String(hrs));
        self.props
            .entry_mut("Hours")
            .map(|e| e.value = PropertyValue::String(self.hours.clone()));

        // Query initial state
        let state_resp = self.cmd("l?")?;
        self.is_open.set(state_resp.trim() == "1");
        self.props.entry_mut("Laser").map(|e| {
            e.value = PropertyValue::String(if self.is_open.get() { "On" } else { "Off" }.into())
        });

        // Query power setpoint. C++ reads p? in W and exposes mW.
        let sp = self.cmd("p?")?;
        if let Ok(watts) = sp.parse::<f64>() {
            let mw = watts * 1000.0;
            self.power_setpoint_mw = mw;
            self.props
                .entry_mut("PowerSetpoint_mW")
                .map(|e| e.value = PropertyValue::Float(mw));
            self.props
                .entry_mut("Power")
                .map(|e| e.value = PropertyValue::Float(mw));
            self.props
                .entry_mut("Power Setpoint")
                .map(|e| e.value = PropertyValue::Float(mw));
        }

        if let Ok(max_power) = self.parse_float_command("gmlp?") {
            self.power_maximum_mw = max_power;
            self.props
                .entry_mut("Power Maximum")
                .map(|e| e.value = PropertyValue::Float(max_power));
            let _ = self.props.set_property_limits("Power", 0.0, max_power);
        }
        if let Ok(max_current) = self.parse_float_command("gmlc?") {
            self.current_maximum_ma = max_current;
            self.props
                .entry_mut("Current Maximum")
                .map(|e| e.value = PropertyValue::Float(max_current));
            let _ = self.props.set_property_limits("Current", 0.0, max_current);
            let _ = self
                .props
                .set_property_limits("Current Modulation Minimum", 0.0, max_current);
            let _ = self
                .props
                .set_property_limits("Current Modulation Maximum", 0.0, max_current);
        }
        let _ = self.refresh_autostart_status();

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "PowerSetpoint_mW" if self.initialized => {
                Ok(PropertyValue::Float(self.parse_watts_to_mw("p?")?))
            }
            "PowerSetpoint_mW" => Ok(PropertyValue::Float(self.power_setpoint_mw)),
            "PowerReadback_mW" if self.initialized => {
                Ok(PropertyValue::Float(self.parse_watts_to_mw("pa?")?))
            }
            "Power" => Ok(PropertyValue::Float(self.power_setpoint_mw)),
            "Power Setpoint" if self.initialized => {
                Ok(PropertyValue::Float(self.parse_watts_to_mw("p?")?))
            }
            "Power Setpoint" => Ok(PropertyValue::Float(self.power_setpoint_mw)),
            "Power Status" if self.initialized => {
                Ok(PropertyValue::Float(self.parse_watts_to_mw("pa?")?))
            }
            "Power Maximum" if self.initialized => {
                Ok(PropertyValue::Float(self.parse_float_command("gmlp?")?))
            }
            "UsageHours" if self.initialized => Ok(PropertyValue::String(self.cmd("hrs?")?)),
            "Hours" if self.initialized => Ok(PropertyValue::String(self.cmd("hrs?")?)),
            "KeyStatus" if self.initialized => {
                let answer = self.cmd("@cobasks?")?;
                Ok(PropertyValue::String(Self::bool_status(
                    &answer, "On", "Off",
                )?))
            }
            "Key On/Off" if self.initialized => Ok(PropertyValue::String(Self::bool_status(
                &self.cmd("@cobasks?")?,
                "On",
                "Off",
            )?)),
            "Interlock" if self.initialized => {
                let answer = self.cmd("ilk?")?;
                let interlock = match answer.chars().next() {
                    Some('0') => "Closed",
                    Some('1') => "Open",
                    _ => return Err(MmError::SerialInvalidResponse),
                };
                Ok(PropertyValue::String(interlock.into()))
            }
            "Fault" if self.initialized => self.get_property("FaultCode"),
            "FaultCode" if self.initialized => {
                let answer = self.cmd("f?")?;
                let fault = match answer.chars().next() {
                    Some('0') => "No Fault",
                    Some('1') => "Temperature Fault",
                    Some('3') => "Open Interlock",
                    Some('4') => "Constant Power Fault",
                    _ => return Err(MmError::SerialInvalidResponse),
                };
                Ok(PropertyValue::String(fault.into()))
            }
            "Serial Command Response" => {
                Ok(PropertyValue::String(self.serial_response.borrow().clone()))
            }
            "Autostart Status" | "Autostart" if self.initialized => Ok(PropertyValue::String(
                Self::bool_status(&self.cmd("@cobas?")?, "Enabled", "Disabled")?,
            )),
            "Current Status" if self.initialized => Ok(PropertyValue::String(self.cmd("i?")?)),
            "Current Setpoint" if self.initialized => Ok(PropertyValue::String(self.cmd("glc?")?)),
            "Current Maximum" if self.initialized => {
                Ok(PropertyValue::Float(self.parse_float_command("gmlc?")?))
            }
            "Current Modulation Minimum" if self.initialized => {
                Ok(PropertyValue::Float(self.parse_float_command("glth?")?))
            }
            "Current Modulation Maximum" if self.initialized => {
                Ok(PropertyValue::Float(self.parse_float_command("gmc?")?))
            }
            "Operating Status" if self.initialized => Ok(PropertyValue::String(
                match self.cmd("gom?")?.chars().next() {
                    Some('0') => "Off".into(),
                    Some('1') => "Waiting for temperature".into(),
                    Some('2') => "Continuous".into(),
                    Some('3') => "On/Off Modulation".into(),
                    Some('4') => "Modulation".into(),
                    Some('5') => "Fault".into(),
                    Some('6') => "Aborted: Complete Laser Start Required".into(),
                    _ => return Err(MmError::SerialInvalidResponse),
                },
            )),
            "Laser" if self.initialized => Ok(PropertyValue::String(if self.get_open()? {
                "On".into()
            } else {
                "Off".into()
            })),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "PowerSetpoint_mW" | "Power" => {
                let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.send_power_setpoint(mw)?;
                }
                self.power_setpoint_mw = mw;
                self.props
                    .entry_mut("PowerSetpoint_mW")
                    .map(|e| e.value = PropertyValue::Float(mw));
                self.props
                    .entry_mut("Power")
                    .map(|e| e.value = PropertyValue::Float(mw));
                Ok(())
            }
            "Serial Command" => {
                let command = val.as_str();
                if command.is_empty() {
                    return self.props.set(name, val);
                }
                let response = self.cmd(command)?;
                *self.serial_response.borrow_mut() = response.clone();
                self.props
                    .entry_mut("Serial Command Response")
                    .map(|e| e.value = PropertyValue::String(response));
                self.props.set(name, val)
            }
            "Autostart" => {
                let status = val.as_str();
                let command = match status {
                    "Enabled" => "@cobas 1",
                    "Disabled" => "@cobas 0",
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                if self.initialized {
                    let _ = self.cmd(command)?;
                }
                self.autostart_status = status.into();
                self.props
                    .entry_mut("Autostart Status")
                    .map(|e| e.value = PropertyValue::String(status.into()));
                self.props.set(name, PropertyValue::String(status.into()))
            }
            "Control Mode" => self.set_control_mode_name(val.as_str()),
            "Current" => {
                let current = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.set_control_mode_name("Constant Current")?;
                    let _ = self.cmd(&format!("slc {}", current))?;
                }
                self.current_ma = current;
                self.props.set(name, PropertyValue::Float(current))
            }
            "Current Modulation Minimum" => {
                let current = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    let _ = self.cmd(&format!("slth {}", current))?;
                }
                self.current_modulation_minimum_ma = current;
                self.props.set(name, PropertyValue::Float(current))
            }
            "Current Modulation Maximum" => {
                let current = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    let _ = self.cmd(&format!("smc {}", current))?;
                }
                self.current_modulation_maximum_ma = current;
                self.props.set(name, PropertyValue::Float(current))
            }
            "Modulation Analog" => {
                let enabled = val.as_str() == "Enabled";
                if val.as_str() != "Enabled" && val.as_str() != "Disabled" {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized {
                    if enabled {
                        let _ = self.cmd("eswm 0")?;
                    }
                    let _ = self.cmd(if enabled { "sames 1" } else { "sames 0" })?;
                }
                self.props.set(name, val)
            }
            "Modulation Digital " => {
                let enabled = val.as_str() == "Enabled";
                if val.as_str() != "Enabled" && val.as_str() != "Disabled" {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized {
                    if enabled {
                        let _ = self.cmd("eswm 0")?;
                    }
                    let _ = self.cmd(if enabled { "sdmes 1" } else { "sdmes 0" })?;
                }
                self.props.set(name, val)
            }
            "Laser" => self.set_open(val.as_str() == "On"),
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

impl Shutter for CoboltLaser {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let autostart = if self.initialized {
            self.refresh_autostart_status()?
        } else {
            self.autostart_status.clone()
        };
        if autostart == "Disabled" {
            let cmd = if open { "l1" } else { "l0" };
            let resp = self.cmd(cmd)?;
            if resp != "OK" {
                return Err(MmError::SerialInvalidResponse);
            }
        } else {
            if self.control_mode != "Constant Current" {
                self.set_control_mode_name("Constant Current")?;
            }
            let current = if open {
                self.parse_float_command("gmc?")
                    .unwrap_or(self.current_modulation_maximum_ma)
            } else {
                self.parse_float_command("glth?")
                    .unwrap_or(self.current_modulation_minimum_ma)
            };
            let target = if open { current } else { 0.0 };
            let _ = self.cmd(&format!("slc {}", target))?;
        }
        self.is_open.set(open);
        self.props
            .entry_mut("Laser")
            .map(|e| e.value = PropertyValue::String(if open { "On" } else { "Off" }.into()));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        if self.initialized {
            match self.cmd("l?")?.trim() {
                "0" => Ok(false),
                "1" => Ok(true),
                _ => Err(MmError::SerialInvalidResponse),
            }
        } else {
            Ok(self.is_open.get())
        }
    }

    fn fire(&mut self, delta_t: f64) -> MmResult<()> {
        self.set_open(true)?;
        thread::sleep(Duration::from_millis((delta_t + 0.5).max(0.0) as u64));
        self.set_open(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn init_script() -> MockTransport {
        MockTransport::new()
            .expect("sn?", "X")
            .expect("glm?", "06-01-488")
            .expect("ver?", "1.0")
            .expect("hrs?", "0")
            .expect("l?", "0")
            .expect("p?", "0.0")
            .expect("gmlp?", "100")
            .expect("gmlc?", "200")
            .expect("@cobas?", "0")
    }

    #[test]
    fn initialize_reads_fields() {
        let transport = MockTransport::new()
            .expect("sn?", "12345")
            .expect("glm?", "06-01-488")
            .expect("ver?", "1.0.0")
            .expect("hrs?", "42.5")
            .expect("l?", "0")
            .expect("p?", "0.050")
            .expect("gmlp?", "100")
            .expect("gmlc?", "200")
            .expect("@cobas?", "0")
            .expect("l?", "0")
            .expect("hrs?", "42.5");
        let mut laser = CoboltLaser::new().with_transport(Box::new(transport));
        laser.initialize().unwrap();
        assert!(!laser.get_open().unwrap());
        assert_eq!(laser.power_setpoint_mw, 50.0);
        assert_eq!(
            laser.get_property("SerialNumber").unwrap(),
            PropertyValue::String("12345".into())
        );
        assert_eq!(
            laser.get_property("UsageHours").unwrap(),
            PropertyValue::String("42.5".into())
        );
    }

    #[test]
    fn open_close_laser() {
        let transport = init_script()
            // set_open(true)
            .expect("@cobas?", "0")
            .expect("l1", "OK")
            .expect("l?", "1")
            // set_open(false)
            .expect("@cobas?", "0")
            .expect("l0", "OK")
            .expect("l?", "0");

        let mut laser = CoboltLaser::new().with_transport(Box::new(transport));
        laser.initialize().unwrap();
        laser.set_open(true).unwrap();
        assert!(laser.get_open().unwrap());
        laser.set_open(false).unwrap();
        assert!(!laser.get_open().unwrap());
    }

    #[test]
    fn set_power_setpoint() {
        let transport = init_script()
            // set_property("PowerSetpoint_mW", 100.0)
            .expect("p 0.1000", "OK");

        let mut laser = CoboltLaser::new().with_transport(Box::new(transport));
        laser.initialize().unwrap();
        laser
            .set_property("PowerSetpoint_mW", PropertyValue::Float(100.0))
            .unwrap();
        assert_eq!(laser.power_setpoint_mw, 100.0);
    }

    #[test]
    fn no_transport_returns_not_connected() {
        let mut laser = CoboltLaser::new();
        assert!(laser.initialize().is_err());
    }

    #[test]
    fn fire_closes_laser_after_pulse() {
        let transport = init_script()
            .expect("@cobas?", "0")
            .expect("l1", "OK")
            .expect("@cobas?", "0")
            .expect("l0", "OK")
            .expect("l?", "0");

        let mut laser = CoboltLaser::new().with_transport(Box::new(transport));
        laser.initialize().unwrap();
        laser.fire(0.0).unwrap();
        assert!(!laser.get_open().unwrap());
    }

    #[test]
    fn get_properties_refresh_live_serial_values() {
        let transport = MockTransport::new()
            .expect("sn?", "X")
            .expect("glm?", "06-01-488")
            .expect("ver?", "1.0")
            .expect("hrs?", "0")
            .expect("l?", "0")
            .expect("p?", "0.0")
            .expect("gmlp?", "100")
            .expect("gmlc?", "200")
            .expect("@cobas?", "0")
            .expect("p?", "0.125")
            .expect("pa?", "0.120")
            .expect("hrs?", "3.5")
            .expect("@cobasks?", "1")
            .expect("ilk?", "0")
            .expect("f?", "4");

        let mut laser = CoboltLaser::new().with_transport(Box::new(transport));
        laser.initialize().unwrap();
        assert_eq!(
            laser.get_property("PowerSetpoint_mW").unwrap(),
            PropertyValue::Float(125.0)
        );
        assert_eq!(
            laser.get_property("PowerReadback_mW").unwrap(),
            PropertyValue::Float(120.0)
        );
        assert_eq!(
            laser.get_property("UsageHours").unwrap(),
            PropertyValue::String("3.5".into())
        );
        assert_eq!(
            laser.get_property("KeyStatus").unwrap(),
            PropertyValue::String("On".into())
        );
        assert_eq!(
            laser.get_property("Interlock").unwrap(),
            PropertyValue::String("Closed".into())
        );
        assert_eq!(
            laser.get_property("FaultCode").unwrap(),
            PropertyValue::String("Constant Power Fault".into())
        );
    }

    #[test]
    fn serial_command_property_stores_response() {
        let transport = init_script().expect("gom?", "2");
        let mut laser = CoboltLaser::new().with_transport(Box::new(transport));
        laser.initialize().unwrap();
        laser
            .set_property("Serial Command", PropertyValue::String("gom?".into()))
            .unwrap();
        assert_eq!(
            laser.get_property("Serial Command Response").unwrap(),
            PropertyValue::String("2".into())
        );
    }

    #[test]
    fn autostart_set_open_uses_current_mode_and_modulation_currents() {
        let transport = MockTransport::new()
            .expect("sn?", "X")
            .expect("glm?", "06-01-488")
            .expect("ver?", "1.0")
            .expect("hrs?", "0")
            .expect("l?", "0")
            .expect("p?", "0.0")
            .expect("gmlp?", "100")
            .expect("gmlc?", "200")
            .expect("@cobas?", "1")
            .expect("@cobas?", "1")
            .expect("ci", "OK")
            .expect("gmc?", "175")
            .expect("slc 175", "OK")
            .expect("@cobas?", "1")
            .expect("glth?", "25")
            .expect("slc 0", "OK");

        let mut laser = CoboltLaser::new().with_transport(Box::new(transport));
        laser.initialize().unwrap();
        laser.set_open(true).unwrap();
        laser.set_open(false).unwrap();
    }

    #[test]
    fn shutdown_only_clears_initialized_like_upstream() {
        let transport = init_script();
        let mut laser = CoboltLaser::new().with_transport(Box::new(transport));
        laser.initialize().unwrap();
        laser.is_open.set(true);
        laser.shutdown().unwrap();
        assert!(laser.get_open().unwrap());
    }
}
