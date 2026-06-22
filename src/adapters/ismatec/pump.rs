/// Ismatec MCP peristaltic pump controller.
///
/// Protocol (TX `\r`, RX `\r\n`):
///   Address (1–8) is prepended to every command.
///   Single-char commands return `*` on success; string commands return a line.
///
///   `<addr>(\r`           → firmware version string
///   `<addr>-\r`           → `*`  reset overload
///   `<addr>L\r`           → `*`  set mode: continuous RPM
///   `<addr>M\r`           → `*`  set mode: continuous flow rate
///   `<addr>J\r`           → `*`  set direction: clockwise
///   `<addr>K\r`           → `*`  set direction: counter-clockwise
///   `<addr>S<5-dig>\r`    → `*`  set speed RPM × 10 (e.g. `S00600` = 60.0 RPM)
///   `<addr>H\r`           → `*`  start pump
///   `<addr>I\r`           → `*`  stop pump
///   `<addr>E\r`           → string: pump running status
///   `<addr>+\r`           → string: tubing inner diameter (mm)
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::Device;
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct IsmatecPump {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    address: u8,
    n_rollers: i64,
    manual_control: bool,
    mode: String,
    speed_rpm: f64,
    clockwise: bool,
    running: bool,
    fractional_digits: i64,
}

impl IsmatecPump {
    pub fn new(address: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Address", PropertyValue::Integer(address as i64), false)
            .unwrap();
        props
            .define_property("NumberOfRollers", PropertyValue::Integer(3), false)
            .unwrap();
        props
            .set_allowed_values("NumberOfRollers", &["2", "3", "4", "6", "8", "12"])
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            address,
            n_rollers: 3,
            manual_control: false,
            mode: "Continuous-RPM".into(),
            speed_rpm: 0.0,
            clockwise: true,
            running: false,
            fractional_digits: 0,
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
        let c = format!("{}{}\r", self.address, command);
        self.call_transport(|t| {
            let r = t.send_recv(&c)?;
            Ok(r.trim().to_string())
        })
    }

    fn cmd_ack(&mut self, command: &str) -> MmResult<()> {
        let resp = self.cmd(command)?;
        if resp == "*" {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("MCP NAK: {}", resp)))
        }
    }

    /// Format speed for `S` command: 5 digits with 1 implied decimal (RPM × 10).
    fn format_speed(rpm: f64) -> String {
        let val = (rpm * 10.0).round() as u32;
        format!("S{:05}", val)
    }

    fn define_property_if_missing(
        &mut self,
        name: &str,
        value: PropertyValue,
        read_only: bool,
    ) -> MmResult<()> {
        if !self.props.has_property(name) {
            self.props.define_property(name, value, read_only)?;
        }
        Ok(())
    }

    fn parse_pump_id(response: &str) -> MmResult<(String, String, String)> {
        let mut parts = response.splitn(3, ' ');
        let model = parts
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| MmError::LocallyDefined("MCP malformed pump ID response".into()))?
            .to_string();
        let firmware = parts
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| MmError::LocallyDefined("MCP malformed pump ID response".into()))?
            .to_string();
        let head = parts
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| MmError::LocallyDefined("MCP malformed pump ID response".into()))?
            .to_string();
        Ok((model, firmware, head))
    }

    fn mode_command(mode: &str) -> MmResult<&'static str> {
        match mode {
            "Continuous-RPM" => Ok("L"),
            "Continuous-FlowRate" => Ok("M"),
            "Dispense-Time" => Ok("N"),
            "Dispense-Volume" => Ok("O"),
            "Dispense-Time+Pause" => Ok("P"),
            "Dispense-Volume+Pause" => Ok("Q"),
            "Dispense-TimedVolume" => Ok("G"),
            _ => Err(MmError::InvalidPropertyValue),
        }
    }
}

impl Default for IsmatecPump {
    fn default() -> Self {
        Self::new(1)
    }
}

impl Device for IsmatecPump {
    fn name(&self) -> &str {
        "IsmatecMCP"
    }
    fn description(&self) -> &str {
        "Ismatec MCP peristaltic pump"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Reset overload
        self.cmd_ack("-")?;
        // Make sure the pump is stopped before synchronizing cached state.
        self.cmd_ack("I")?;
        self.running = false;
        // We cannot query these state bits, so match upstream by forcing them.
        self.cmd_ack(if self.clockwise { "J" } else { "K" })?;
        self.cmd_ack(if self.manual_control { "A" } else { "B" })?;
        self.cmd_ack(Self::mode_command(&self.mode)?)?;

        let digits = self.cmd("[")?;
        self.fractional_digits = digits.parse::<i64>().map_err(|_| {
            MmError::LocallyDefined("MCP malformed fractional digit response".into())
        })?;

        let id = self.cmd("#")?;
        let (model, bundled_firmware, head_id) = Self::parse_pump_id(&id)?;
        self.define_property_if_missing("PumpModel", PropertyValue::String(model), true)?;
        self.define_property_if_missing("PumpHeadID", PropertyValue::String(head_id), true)?;

        self.define_property_if_missing("Pumping", PropertyValue::String("Off".into()), false)?;
        self.props.set_allowed_values("Pumping", &["Off", "On"])?;
        self.define_property_if_missing(
            "ManualControl",
            PropertyValue::String(
                if self.manual_control {
                    "Enabled"
                } else {
                    "Disabled"
                }
                .into(),
            ),
            false,
        )?;
        self.props
            .set_allowed_values("ManualControl", &["Disabled", "Enabled"])?;
        self.define_property_if_missing("Mode", PropertyValue::String(self.mode.clone()), false)?;
        self.props.set_allowed_values(
            "Mode",
            &[
                "Continuous-RPM",
                "Continuous-FlowRate",
                "Dispense-Time",
                "Dispense-Volume",
                "Dispense-Time+Pause",
                "Dispense-Volume+Pause",
                "Dispense-TimedVolume",
            ],
        )?;

        // Get firmware version
        let ver = self.cmd("(")?;
        self.define_property_if_missing(
            "FirmwareVersion",
            PropertyValue::String(String::new()),
            true,
        )?;
        self.props.entry_mut("FirmwareVersion").map(|e| {
            e.value = PropertyValue::String(if ver.is_empty() {
                bundled_firmware
            } else {
                ver
            })
        });

        let head = self.cmd(")")?;
        self.props
            .entry_mut("PumpHeadID")
            .map(|e| e.value = PropertyValue::String(head));
        self.define_property_if_missing(
            "Direction",
            PropertyValue::String(
                if self.clockwise {
                    "Clockwise"
                } else {
                    "Counterclockwise"
                }
                .into(),
            ),
            false,
        )?;
        self.props
            .set_allowed_values("Direction", &["Clockwise", "Counterclockwise"])?;
        self.define_property_if_missing("Speed_rpm", PropertyValue::Float(self.speed_rpm), false)?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.cmd_ack("I"); // stop
            self.running = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if !self.props.has_property(name) {
            return self.props.get(name).cloned();
        }
        match name {
            "NumberOfRollers" => Ok(PropertyValue::Integer(self.n_rollers)),
            "Pumping" => Ok(PropertyValue::String(
                if self.running { "On" } else { "Off" }.into(),
            )),
            "ManualControl" => Ok(PropertyValue::String(
                if self.manual_control {
                    "Enabled"
                } else {
                    "Disabled"
                }
                .into(),
            )),
            "Mode" => Ok(PropertyValue::String(self.mode.clone())),
            "Speed_rpm" => Ok(PropertyValue::Float(self.speed_rpm)),
            "Direction" => Ok(PropertyValue::String(
                if self.clockwise {
                    "Clockwise"
                } else {
                    "Counterclockwise"
                }
                .into(),
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if !self.props.has_property(name) {
            return self.props.set(name, val);
        }
        match name {
            "NumberOfRollers" => {
                let rollers = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(rollers))?;
                self.n_rollers = rollers;
                Ok(())
            }
            "Pumping" => {
                let s = val.as_str().to_string();
                match s.as_str() {
                    "On" => self.start()?,
                    "Off" => self.stop()?,
                    _ => return Err(MmError::InvalidPropertyValue),
                }
                self.props
                    .entry_mut("Pumping")
                    .map(|e| e.value = PropertyValue::String(s));
                Ok(())
            }
            "ManualControl" => {
                let s = val.as_str().to_string();
                let enabled = match s.as_str() {
                    "Enabled" => true,
                    "Disabled" => false,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                if self.initialized {
                    self.cmd_ack(if enabled { "A" } else { "B" })?;
                }
                self.manual_control = enabled;
                self.props
                    .entry_mut("ManualControl")
                    .map(|e| e.value = PropertyValue::String(s));
                Ok(())
            }
            "Mode" => {
                let s = val.as_str().to_string();
                let cmd = Self::mode_command(&s)?;
                if self.initialized {
                    self.cmd_ack(cmd)?;
                }
                self.mode = s.clone();
                self.props
                    .entry_mut("Mode")
                    .map(|e| e.value = PropertyValue::String(s));
                Ok(())
            }
            "Speed_rpm" => {
                let rpm = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if rpm <= 0.0 || rpm > 9999.9 {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized {
                    self.cmd_ack(&Self::format_speed(rpm))?;
                }
                self.speed_rpm = rpm;
                self.props
                    .entry_mut("Speed_rpm")
                    .map(|e| e.value = PropertyValue::Float(rpm));
                Ok(())
            }
            "Direction" => {
                let s = val.as_str().to_string();
                let clockwise = match s.as_str() {
                    "Clockwise" => true,
                    "Counterclockwise" => false,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                if self.initialized {
                    self.cmd_ack(if clockwise { "J" } else { "K" })?;
                }
                self.clockwise = clockwise;
                self.props
                    .entry_mut("Direction")
                    .map(|e| e.value = PropertyValue::String(s));
                Ok(())
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

/// Expose start/stop via set_property("Running", ...) for scripting convenience.
impl IsmatecPump {
    pub fn start(&mut self) -> MmResult<()> {
        self.cmd_ack("H")?;
        self.running = true;
        Ok(())
    }

    pub fn stop(&mut self) -> MmResult<()> {
        self.cmd_ack("I")?;
        self.running = false;
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.running
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect("1-\r", "*") // reset overload
            .expect("1I\r", "*") // stop
            .expect("1J\r", "*") // clockwise
            .expect("1B\r", "*") // manual control disabled
            .expect("1L\r", "*") // continuous RPM mode
            .expect("1[\r", "0") // fractional digits
            .expect("1#\r", "MCP 1.4 380") // model, bundled firmware, head ID
            .expect("1(\r", "MCP Standard v1.4") // firmware
            .expect("1)\r", "380") // head ID
    }

    #[test]
    fn initialize() {
        let mut p = IsmatecPump::new(1).with_transport(Box::new(make_init_transport()));
        assert!(p.has_property("NumberOfRollers"));
        assert!(!p.has_property("FirmwareVersion"));
        assert!(!p.has_property("Direction"));
        assert!(!p.has_property("Speed_rpm"));
        p.initialize().unwrap();
        assert!(!p.is_running());
        assert!(p.has_property("FirmwareVersion"));
        assert!(p.has_property("Direction"));
        assert!(p.has_property("Speed_rpm"));
        assert!(p.has_property("Pumping"));
        assert!(p.has_property("ManualControl"));
        assert!(p.has_property("Mode"));
        assert_eq!(
            p.get_property("PumpModel").unwrap(),
            PropertyValue::String("MCP".into())
        );
    }

    #[test]
    fn start_stop() {
        let t = make_init_transport()
            .expect("1H\r", "*")
            .expect("1I\r", "*");
        let mut p = IsmatecPump::new(1).with_transport(Box::new(t));
        p.initialize().unwrap();
        p.start().unwrap();
        assert!(p.is_running());
        p.stop().unwrap();
        assert!(!p.is_running());
    }

    #[test]
    fn set_speed() {
        let t = make_init_transport().expect("1S00600\r", "*");
        let mut p = IsmatecPump::new(1).with_transport(Box::new(t));
        p.initialize().unwrap();
        p.set_property("Speed_rpm", PropertyValue::Float(60.0))
            .unwrap();
        assert_eq!(p.speed_rpm, 60.0);
    }

    #[test]
    fn set_ccw() {
        let t = make_init_transport().expect("1K\r", "*");
        let mut p = IsmatecPump::new(1).with_transport(Box::new(t));
        p.initialize().unwrap();
        p.set_property(
            "Direction",
            PropertyValue::String("Counterclockwise".into()),
        )
        .unwrap();
        assert!(!p.clockwise);
    }

    #[test]
    fn set_pumping_property_uses_start_stop_commands() {
        let t = make_init_transport()
            .expect("1H\r", "*")
            .expect("1I\r", "*");
        let mut p = IsmatecPump::new(1).with_transport(Box::new(t));
        p.initialize().unwrap();
        p.set_property("Pumping", PropertyValue::String("On".into()))
            .unwrap();
        assert!(p.is_running());
        p.set_property("Pumping", PropertyValue::String("Off".into()))
            .unwrap();
        assert!(!p.is_running());
    }

    #[test]
    fn initialized_manual_control_and_mode_write_upstream_commands() {
        let t = make_init_transport()
            .expect("1A\r", "*")
            .expect("1M\r", "*");
        let mut p = IsmatecPump::new(1).with_transport(Box::new(t));
        p.initialize().unwrap();
        p.set_property("ManualControl", PropertyValue::String("Enabled".into()))
            .unwrap();
        p.set_property("Mode", PropertyValue::String("Continuous-FlowRate".into()))
            .unwrap();
    }

    #[test]
    fn malformed_pump_id_response_fails_initialize() {
        let t = MockTransport::new()
            .expect("1-\r", "*")
            .expect("1I\r", "*")
            .expect("1J\r", "*")
            .expect("1B\r", "*")
            .expect("1L\r", "*")
            .expect("1[\r", "0")
            .expect("1#\r", "MCP-only");
        let mut p = IsmatecPump::new(1).with_transport(Box::new(t));
        assert!(p.initialize().is_err());
    }

    #[test]
    fn format_speed() {
        assert_eq!(IsmatecPump::format_speed(60.0), "S00600");
        assert_eq!(IsmatecPump::format_speed(0.0), "S00000");
        assert_eq!(IsmatecPump::format_speed(100.0), "S01000");
        assert_eq!(IsmatecPump::format_speed(6.5), "S00065");
    }

    #[test]
    fn nak_response_fails() {
        let t = make_init_transport().expect("1H\r", "?"); // not *
        let mut p = IsmatecPump::new(1).with_transport(Box::new(t));
        p.initialize().unwrap();
        assert!(p.start().is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(IsmatecPump::new(1).initialize().is_err());
    }
}
