/// X-Cite 120PC Exacte xenon arc lamp illuminator.
///
/// 2-character ASCII command protocol (no terminator on most responses).
///   `tt\r`        → connect
///   `aa\r`        → clear alarm
///   `vv\r`        → software version string
///   `uu\r`        → unit status bitmask string
///   `ii\r`        → current intensity level (char '0'–'4')
///   `mm\r`        → open shutter
///   `zz\r`        → close shutter
///   `bb\r`        → turn lamp on
///   `ss\r`        → turn lamp off
///   `iN\r`        → set intensity to level N (0=0%, 1=12%, 2=25%, 3=50%, 4=100%)
///   `ll\r`        → lock front panel
///   `nn\r`        → unlock front panel
///
/// Unit status bitmask (from `uu`):
///   bit 0: alarm active
///   bit 1: lamp on
///   bit 2: shutter open
///   bit 4: lamp ready
///   bit 5: panel locked
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;
use std::thread;
use std::time::{Duration, Instant};

const INTENSITIES: [&str; 5] = ["0", "12", "25", "50", "100"];

pub struct XCite120PC {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    shutter_open: bool,
    lamp_on: bool,
    intensity_level: u8,
    shutter_dwell_time_ms: f64,
    last_shutter_time: Option<Instant>,
    time_shutter_closed: Option<Instant>,
}

impl XCite120PC {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Lamp-Intensity", PropertyValue::String("100".into()), false)
            .unwrap();
        props
            .set_allowed_values("Lamp-Intensity", &INTENSITIES)
            .unwrap();
        props
            .define_property("LampIntensity", PropertyValue::String("100".into()), false)
            .unwrap();
        props
            .set_allowed_values("LampIntensity", &INTENSITIES)
            .unwrap();
        props
            .define_property(
                "Shutter-State",
                PropertyValue::String("Closed".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("Shutter-State", &["Closed", "Open"])
            .unwrap();
        props
            .define_property("Shutter-Dwell-Time", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("Shutter-Dwell-Time", 0.0, 5000.0)
            .unwrap();
        props
            .define_property("Exposure-Time [s]", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("Exposure-Time [s]", 0.2, 999.9)
            .unwrap();
        props
            .define_property("Trigger", PropertyValue::String("Off".into()), false)
            .unwrap();
        props.set_allowed_values("Trigger", &["On", "Off"]).unwrap();
        props
            .define_property(
                "Front-Panel-Lock",
                PropertyValue::String("False".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("Front-Panel-Lock", &["True", "False"])
            .unwrap();
        props
            .define_property(
                "LockFrontPanel",
                PropertyValue::String("False".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("LockFrontPanel", &["True", "False"])
            .unwrap();
        props
            .define_property("Lamp-State", PropertyValue::String("Off".into()), false)
            .unwrap();
        props
            .set_allowed_values("Lamp-State", &["On", "Off"])
            .unwrap();
        props
            .define_property("Alarm-Clear", PropertyValue::String("Clear".into()), false)
            .unwrap();
        props.set_allowed_values("Alarm-Clear", &["Clear"]).unwrap();
        props
            .define_property(
                "Software-Version",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "ShutterSoftwareVersion",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("Lamp-Hours", PropertyValue::String("Unknown".into()), true)
            .unwrap();
        props
            .define_property(
                "Unit-Status-Alarm-State",
                PropertyValue::String("Unknown".into()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Unit-Status-Lamp-State",
                PropertyValue::String("Unknown".into()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Unit-Status-Shutter-State",
                PropertyValue::String("Unknown".into()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Unit-Status-Home",
                PropertyValue::String("Unknown".into()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Unit-Status-Lamp-Ready",
                PropertyValue::String("Unknown".into()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Unit-Status-Front-Panel",
                PropertyValue::String("Unknown".into()),
                true,
            )
            .unwrap();
        // Compatibility aliases from the previous Rust adapter surface.
        props
            .define_property("Intensity_pct", PropertyValue::String("100".into()), false)
            .unwrap();
        props
            .set_allowed_values("Intensity_pct", &INTENSITIES)
            .unwrap();
        props
            .define_property("LampState", PropertyValue::String("Off".into()), false)
            .unwrap();
        props
            .set_allowed_values("LampState", &["On", "Off"])
            .unwrap();
        props
            .define_property("PanelLock", PropertyValue::String("False".into()), false)
            .unwrap();
        props
            .set_allowed_values("PanelLock", &["True", "False", "On", "Off"])
            .unwrap();
        props
            .define_property("Version", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("LampHours", PropertyValue::String("Unknown".into()), true)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            shutter_open: false,
            lamp_on: false,
            intensity_level: 4,
            shutter_dwell_time_ms: 0.0,
            last_shutter_time: None,
            time_shutter_closed: None,
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
        let cmd = format!("{}\r", command);
        self.call_transport(|t| {
            t.purge()?;
            let resp = t.send_recv(&cmd)?;
            let resp = resp.trim().to_string();
            if resp == "e" {
                Err(MmError::SerialCommandFailed)
            } else {
                Ok(resp)
            }
        })
    }

    fn cmd_no_response(&self, command: &str) -> MmResult<()> {
        let resp = self.cmd(command)?;
        if resp.is_empty() || resp == "OK" {
            Ok(())
        } else {
            Err(MmError::NotConnected)
        }
    }

    fn parse_status(status_str: &str) -> (bool, bool, bool, bool, bool, bool) {
        // Status is a bitmask returned as an integer string
        let bits: u32 = status_str.trim().parse().unwrap_or(0);
        let alarm = (bits & 0x01) != 0;
        let lamp_on = (bits & 0x02) != 0;
        let shutter = (bits & 0x04) != 0;
        let home_fault = (bits & 0x08) != 0;
        let lamp_ready = (bits & 0x10) != 0;
        let locked = (bits & 0x20) != 0;
        (alarm, lamp_on, shutter, home_fault, lamp_ready, locked)
    }

    fn status_value(status: &str, bit_name: &str) -> MmResult<PropertyValue> {
        let (alarm, lamp, shutter, home_fault, ready, locked) = Self::parse_status(status);
        let value = match bit_name {
            "Unit-Status-Alarm-State" => {
                if alarm {
                    "ON"
                } else {
                    "OFF"
                }
            }
            "Unit-Status-Lamp-State" => {
                if lamp {
                    "ON"
                } else {
                    "OFF"
                }
            }
            "Unit-Status-Shutter-State" => {
                if shutter {
                    "OPEN"
                } else {
                    "CLOSED"
                }
            }
            "Unit-Status-Home" => {
                if home_fault {
                    "FAULT"
                } else {
                    "PASS"
                }
            }
            "Unit-Status-Lamp-Ready" => {
                if ready {
                    "READY"
                } else {
                    "NOT READY"
                }
            }
            "Unit-Status-Front-Panel" => {
                if locked {
                    "LOCKED"
                } else {
                    "NOT LOCKED"
                }
            }
            _ => return Err(MmError::UnknownLabel(bit_name.to_string())),
        };
        Ok(PropertyValue::String(value.into()))
    }

    fn set_prop_force(&mut self, name: &str, value: PropertyValue) {
        if let Some(entry) = self.props.entry_mut(name) {
            entry.value = value;
        }
    }

    fn set_status_props(&mut self, status: &str) {
        let (_alarm, lamp, shutter, _home, _ready, locked) = Self::parse_status(status);
        self.lamp_on = lamp;
        self.shutter_open = shutter;
        self.set_prop_force(
            "Lamp-State",
            PropertyValue::String(if lamp { "On" } else { "Off" }.into()),
        );
        self.set_prop_force(
            "LampState",
            PropertyValue::String(if lamp { "On" } else { "Off" }.into()),
        );
        self.set_prop_force(
            "Shutter-State",
            PropertyValue::String(if shutter { "Open" } else { "Closed" }.into()),
        );
        self.set_prop_force(
            "Front-Panel-Lock",
            PropertyValue::String(if locked { "True" } else { "False" }.into()),
        );
        self.set_prop_force(
            "LockFrontPanel",
            PropertyValue::String(if locked { "True" } else { "False" }.into()),
        );
        self.set_prop_force(
            "PanelLock",
            PropertyValue::String(if locked { "True" } else { "False" }.into()),
        );
    }
}

impl Default for XCite120PC {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for XCite120PC {
    fn name(&self) -> &str {
        "XCite120PC"
    }
    fn description(&self) -> &str {
        "X-Cite 120PC Exacte xenon arc lamp"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        self.cmd_no_response("tt")?; // connect
        self.cmd_no_response("aa")?; // clear alarm

        let ver = self.cmd("vv")?;
        self.set_prop_force("Software-Version", PropertyValue::String(ver.clone()));
        self.set_prop_force("ShutterSoftwareVersion", PropertyValue::String(ver.clone()));
        self.set_prop_force("Version", PropertyValue::String(ver));

        let hours = self.cmd("hh")?;
        self.set_prop_force("Lamp-Hours", PropertyValue::String(hours.clone()));
        self.set_prop_force("LampHours", PropertyValue::String(hours));

        let status = self.cmd("uu")?;
        self.set_status_props(&status);

        let level_str = self.cmd("ii")?;
        self.intensity_level = level_str.trim().parse::<u8>().unwrap_or(4).min(4);
        let pct = INTENSITIES[self.intensity_level as usize];
        self.set_prop_force("Lamp-Intensity", PropertyValue::String(pct.into()));
        self.set_prop_force("LampIntensity", PropertyValue::String(pct.into()));
        self.set_prop_force("Intensity_pct", PropertyValue::String(pct.into()));

        let now = Instant::now();
        self.time_shutter_closed = Some(now);
        self.last_shutter_time = Some(now);
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if self.initialized {
            match name {
                "Lamp-Hours" | "LampHours" => return Ok(PropertyValue::String(self.cmd("hh")?)),
                "Unit-Status-Alarm-State"
                | "Unit-Status-Lamp-State"
                | "Unit-Status-Shutter-State"
                | "Unit-Status-Home"
                | "Unit-Status-Lamp-Ready"
                | "Unit-Status-Front-Panel" => {
                    let status = self.cmd("uu")?;
                    return Self::status_value(&status, name);
                }
                _ => {}
            }
        }
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Lamp-Intensity" | "LampIntensity" | "Intensity_pct" => {
                let pct_str = val.as_str().to_string();
                let level = INTENSITIES
                    .iter()
                    .position(|&s| s == pct_str)
                    .ok_or(MmError::InvalidPropertyValue)? as u8;
                if self.initialized {
                    self.cmd_no_response(&format!("i{}", (b'0' + level) as char))?;
                }
                self.intensity_level = level;
                self.props
                    .set("Lamp-Intensity", PropertyValue::String(pct_str.clone()))?;
                self.props
                    .set("LampIntensity", PropertyValue::String(pct_str.clone()))?;
                self.props
                    .set("Intensity_pct", PropertyValue::String(pct_str))
            }
            "Lamp-State" | "LampState" => {
                let state = val.as_str().to_string();
                if self.initialized {
                    let cmd = if state == "On" { "bb" } else { "ss" };
                    self.cmd_no_response(cmd)?;
                }
                self.lamp_on = state == "On";
                self.props
                    .set("Lamp-State", PropertyValue::String(state.clone()))?;
                self.props.set("LampState", PropertyValue::String(state))
            }
            "Front-Panel-Lock" | "LockFrontPanel" | "PanelLock" => {
                let mut lock = val.as_str().to_string();
                if name == "PanelLock" {
                    lock = if lock == "On" {
                        "True".into()
                    } else if lock == "Off" {
                        "False".into()
                    } else {
                        lock
                    };
                }
                if self.initialized {
                    let cmd = if lock == "True" { "ll" } else { "nn" };
                    self.cmd_no_response(cmd)?;
                }
                self.props
                    .set("Front-Panel-Lock", PropertyValue::String(lock.clone()))?;
                self.props
                    .set("LockFrontPanel", PropertyValue::String(lock.clone()))?;
                self.props.set("PanelLock", PropertyValue::String(lock))
            }
            "Shutter-State" => {
                let state = val.as_str().to_string();
                self.set_open(state == "Open")?;
                self.props
                    .set("Shutter-State", PropertyValue::String(state))
            }
            "Shutter-Dwell-Time" => {
                let dwell = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0.0..=5000.0).contains(&dwell) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.shutter_dwell_time_ms = dwell;
                self.props.set(name, PropertyValue::Float(dwell))
            }
            "Exposure-Time [s]" => {
                let exposure = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0.2..=999.9).contains(&exposure) {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized {
                    self.cmd_no_response(&format!("c{:04}", (exposure * 10.0) as i32))?;
                }
                self.props.set(name, PropertyValue::Float(exposure))
            }
            "Trigger" => {
                let trigger = val.as_str().to_string();
                if trigger != "On" && trigger != "Off" {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized && trigger == "On" {
                    self.cmd_no_response("oo")?;
                }
                self.props.set(name, PropertyValue::String(trigger))
            }
            "Alarm-Clear" => {
                if self.initialized {
                    self.cmd_no_response("aa")?;
                }
                self.props.set(name, PropertyValue::String("Clear".into()))
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
        self.last_shutter_time
            .map(|last| last.elapsed().as_secs_f64() * 1000.0 < self.shutter_dwell_time_ms)
            .unwrap_or(false)
    }
}

impl Shutter for XCite120PC {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        if open && self.shutter_dwell_time_ms > 0.0 {
            if let Some(closed_at) = self.time_shutter_closed {
                let elapsed = closed_at.elapsed();
                let dwell = Duration::from_secs_f64(self.shutter_dwell_time_ms / 1000.0);
                if elapsed < dwell {
                    thread::sleep(dwell - elapsed);
                }
            }
        }
        let cmd = if open { "mm" } else { "zz" };
        self.cmd_no_response(cmd)?;
        self.shutter_open = open;
        if !open {
            self.time_shutter_closed = Some(Instant::now());
        }
        self.last_shutter_time = Some(Instant::now());
        self.set_prop_force(
            "Shutter-State",
            PropertyValue::String(if open { "Open" } else { "Closed" }.into()),
        );
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.shutter_open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any("OK") // tt
            .any("OK") // aa
            .any("v1.23") // vv
            .any("123") // hh
            .any("0") // uu: all bits clear → shutter closed, lamp off
            .any("4") // ii: level 4 = 100%
    }

    #[test]
    fn initialize() {
        let mut dev = XCite120PC::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(dev.intensity_level, 4);
    }

    #[test]
    fn open_close_shutter() {
        let t = make_transport().any("OK").any("OK");
        let mut dev = XCite120PC::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_intensity() {
        let t = make_transport().expect("i2\r", "");
        let mut dev = XCite120PC::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Lamp-Intensity", PropertyValue::String("25".into()))
            .unwrap();
        assert_eq!(dev.intensity_level, 2);
        assert_eq!(
            dev.get_property("Intensity_pct").unwrap(),
            PropertyValue::String("25".into())
        );
        assert_eq!(
            dev.get_property("LampIntensity").unwrap(),
            PropertyValue::String("25".into())
        );
    }

    #[test]
    fn fire_is_unsupported() {
        let mut dev = XCite120PC::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert_eq!(dev.fire(1.0), Err(MmError::UnsupportedCommand));
    }

    #[test]
    fn lamp_on_off() {
        let t = make_transport().any("OK").any("OK");
        let mut dev = XCite120PC::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Lamp-State", PropertyValue::String("On".into()))
            .unwrap();
        assert!(dev.lamp_on);
        dev.set_property("Lamp-State", PropertyValue::String("Off".into()))
            .unwrap();
        assert!(!dev.lamp_on);
    }

    #[test]
    fn exact_status_properties_refresh_from_device() {
        let t = make_transport()
            .expect("uu\r", "63")
            .expect("uu\r", "0")
            .expect("hh\r", "456");
        let mut dev = XCite120PC::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert_eq!(
            dev.get_property("Unit-Status-Shutter-State").unwrap(),
            PropertyValue::String("OPEN".into())
        );
        assert_eq!(
            dev.get_property("Unit-Status-Lamp-Ready").unwrap(),
            PropertyValue::String("NOT READY".into())
        );
        assert_eq!(
            dev.get_property("Lamp-Hours").unwrap(),
            PropertyValue::String("456".into())
        );
    }

    #[test]
    fn upstream_property_aliases_drive_exact_commands() {
        let t = make_transport()
            .expect("i3\r", "")
            .expect("ll\r", "")
            .expect("c0003\r", "")
            .expect("oo\r", "");
        let mut dev = XCite120PC::new().with_transport(Box::new(t));
        dev.initialize().unwrap();

        dev.set_property("LampIntensity", PropertyValue::String("50".into()))
            .unwrap();
        dev.set_property("LockFrontPanel", PropertyValue::String("True".into()))
            .unwrap();
        dev.set_property("Exposure-Time [s]", PropertyValue::Float(0.3))
            .unwrap();
        dev.set_property("Trigger", PropertyValue::String("On".into()))
            .unwrap();

        assert_eq!(
            dev.get_property("Lamp-Intensity").unwrap(),
            PropertyValue::String("50".into())
        );
        assert_eq!(
            dev.get_property("Front-Panel-Lock").unwrap(),
            PropertyValue::String("True".into())
        );
    }

    #[test]
    fn command_error_response_is_rejected() {
        let t = make_transport().expect("bb\r", "e");
        let mut dev = XCite120PC::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert_eq!(
            dev.set_property("Lamp-State", PropertyValue::String("On".into())),
            Err(MmError::SerialCommandFailed)
        );
    }

    #[test]
    fn unexpected_no_response_payload_is_rejected() {
        let t = make_transport().expect("mm\r", "not-ok");
        let mut dev = XCite120PC::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert_eq!(dev.set_open(true), Err(MmError::NotConnected));
    }

    #[test]
    fn no_transport_error() {
        assert!(XCite120PC::new().initialize().is_err());
    }
}
