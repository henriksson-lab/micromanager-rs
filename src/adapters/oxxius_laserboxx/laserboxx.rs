use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};
use std::thread;
use std::time::Duration;

/// Oxxius LaserBoxx laser controller (LBX/LCX/LMX models).
///
/// Implements the `Shutter` trait: open = emission on (`dl 1`), closed = emission off (`dl 0`).
///
/// Status codes from `?sta`:
///   1 = warm-up, 2 = stand-by, 3 = emission on, 4 = alarm,
///   5 = internal error, 6 = sleep, 7 = searching SLM point.
pub struct LaserBoxx {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    is_open: Cell<bool>,
    power_setpoint_mw: Cell<f64>,
    current_setpoint_pct: Cell<f64>,
    nominal_power_mw: f64,
    model: String,
}

impl LaserBoxx {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Model", PropertyValue::String("L.X-000".into()), true)
            .unwrap();
        props
            .define_property("Serial number", PropertyValue::String("0".into()), true)
            .unwrap();
        props
            .define_property("Software version", PropertyValue::String("0".into()), true)
            .unwrap();
        props
            .define_property("Laser status", PropertyValue::String("Off".into()), true)
            .unwrap();
        props
            .define_property("Emission", PropertyValue::String("Off".into()), false)
            .unwrap();
        props
            .set_allowed_values("Emission", &["Off", "On"])
            .unwrap();
        props
            .define_property("Alarm", PropertyValue::String("No Alarm".into()), true)
            .unwrap();
        props
            .define_property("OnInterlock", PropertyValue::String("Open".into()), true)
            .unwrap();
        props
            .define_property("Control mode", PropertyValue::String("APC".into()), false)
            .unwrap();
        props
            .set_allowed_values("Control mode", &["APC", "ACC"])
            .unwrap();
        props
            .define_property("Monitored power (mW)", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("Power set point (%)", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("Power set point (%)", 0.0, 110.0)
            .unwrap();
        props
            .define_property("Monitored current (mA)", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("Current set point (%)", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("Current set point (%)", 0.0, 125.0)
            .unwrap();
        props
            .define_property("Sleep mode", PropertyValue::String("Off".into()), false)
            .unwrap();
        props
            .set_allowed_values("Sleep mode", &["Sleep", "Ready"])
            .unwrap();
        props
            .define_property(
                "Analog modulation",
                PropertyValue::String("Off".into()),
                false,
            )
            .unwrap();
        props
            .define_property(
                "Digital modulation",
                PropertyValue::String("Off".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("Analog modulation", &["Off", "On"])
            .unwrap();
        props
            .set_allowed_values("Digital modulation", &["Off", "On"])
            .unwrap();
        props
            .define_property("Emission time (hours)", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("Base temperature", PropertyValue::Float(0.0), true)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: Cell::new(false),
            power_setpoint_mw: Cell::new(0.0),
            current_setpoint_pct: Cell::new(0.0),
            nominal_power_mw: 100.0,
            model: String::new(),
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

    /// Parse nominal power from model string e.g. "LBX-473-100-CSB" → 100.0 mW.
    fn parse_nominal_power(model: &str) -> f64 {
        model
            .split('-')
            .nth(2)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(100.0)
    }

    fn status_string(code: i64) -> MmResult<&'static str> {
        match code {
            1 => Ok("Warm-up phase"),
            2 => Ok("Stand-by for emission"),
            3 => Ok("Emission is on"),
            4 => Ok("Alarm raised"),
            5 => Ok("Internal error raised"),
            6 => Ok("Sleep mode"),
            7 => Ok("Searching for SLM point"),
            _ => Err(MmError::UnknownPosition),
        }
    }

    fn alarm_string(code: i64) -> MmResult<&'static str> {
        match code {
            0 => Ok("No alarm"),
            1 => Ok("Out-of-bounds current"),
            2 => Ok("Out-of-bounds power"),
            3 => Ok("Out-of-bounds supply voltage"),
            4 => Ok("Out-of-bounds inner temperature"),
            5 => Ok("Out-of-bounds laser head temperature"),
            7 => Ok("Interlock circuit open"),
            8 => Ok("Manual reset"),
            _ => Err(MmError::UnknownPosition),
        }
    }

    fn model_property_value(info: &str) -> String {
        let mut hyphens = info.match_indices('-');
        hyphens.next();
        match hyphens.next() {
            Some((idx, _)) => info[..idx].to_string(),
            None => info.to_string(),
        }
    }

    fn status_code(&self) -> MmResult<i64> {
        self.cmd("?sta")?
            .parse::<i64>()
            .map_err(|_| MmError::SerialInvalidResponse)
    }

    fn status_is_open(code: i64) -> bool {
        matches!(code, 1 | 3 | 7)
    }

    fn parse_f64_response(response: String) -> MmResult<f64> {
        response
            .trim_end_matches('%')
            .parse::<f64>()
            .map_err(|_| MmError::SerialInvalidResponse)
    }

    fn refresh_open_from_status(&self) -> MmResult<bool> {
        let open = Self::status_is_open(self.status_code()?);
        self.is_open.set(open);
        Ok(open)
    }

    fn refresh_status_properties(&mut self) -> MmResult<()> {
        let serial_number = self.cmd("hid?")?;
        self.props
            .entry_mut("Serial number")
            .map(|e| e.value = PropertyValue::String(serial_number));

        let software_version = self.cmd("?sv")?;
        self.props
            .entry_mut("Software version")
            .map(|e| e.value = PropertyValue::String(software_version));

        let model = Self::model_property_value(&self.cmd("inf?")?);
        self.props
            .entry_mut("Model")
            .map(|e| e.value = PropertyValue::String(model));

        let status = self.status_code()?;
        let status_string = Self::status_string(status)?;
        self.props
            .entry_mut("Laser status")
            .map(|e| e.value = PropertyValue::String(status_string.into()));

        let status = self.status_code()?;
        self.is_open.set(Self::status_is_open(status));
        let emission = if self.is_open.get() { "On" } else { "Off" };
        self.props
            .entry_mut("Emission")
            .map(|e| e.value = PropertyValue::String(emission.into()));

        let alarm_code = self
            .cmd("?f")?
            .parse::<i64>()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        let alarm = Self::alarm_string(alarm_code)?;
        self.props
            .entry_mut("Alarm")
            .map(|e| e.value = PropertyValue::String(alarm.into()));

        let interlock = if self.cmd("?int")?.trim() == "1" {
            "Closed"
        } else {
            "Open"
        };
        self.props
            .entry_mut("OnInterlock")
            .map(|e| e.value = PropertyValue::String(interlock.into()));

        let control_mode = if self.model == "LBX" && self.cmd("?acc")?.trim() == "1" {
            "ACC"
        } else {
            "APC"
        };
        self.props
            .entry_mut("Control mode")
            .map(|e| e.value = PropertyValue::String(control_mode.into()));

        let monitored_power = self.cmd("?p")?.parse::<f64>().unwrap_or(0.0);
        self.props
            .entry_mut("Monitored power (mW)")
            .map(|e| e.value = PropertyValue::Float(monitored_power));

        let power_pct = if self.model == "LBX" {
            100.0 * self.cmd("?sp")?.parse::<f64>().unwrap_or(0.0) / self.nominal_power_mw
        } else {
            Self::parse_f64_response(self.cmd("ip")?)?
        };
        self.power_setpoint_mw.set(power_pct);
        self.props
            .entry_mut("Power set point (%)")
            .map(|e| e.value = PropertyValue::Float(power_pct));

        if self.model != "LMX" {
            let monitored_current = self.cmd("?c")?.parse::<f64>().unwrap_or(0.0);
            self.props
                .entry_mut("Monitored current (mA)")
                .map(|e| e.value = PropertyValue::Float(monitored_current));
        }

        let current_pct = if self.model == "LBX" {
            self.cmd("?sc")?.parse::<f64>().unwrap_or(0.0)
        } else {
            0.0
        };
        self.current_setpoint_pct.set(current_pct);
        self.props
            .entry_mut("Current set point (%)")
            .map(|e| e.value = PropertyValue::Float(current_pct));

        let sleep_mode = if self.model != "LMX" && self.cmd("?t")?.trim() == "0" {
            "Sleep"
        } else {
            "Ready"
        };
        self.props
            .entry_mut("Sleep mode")
            .map(|e| e.value = PropertyValue::String(sleep_mode.into()));

        let analog_mode = if self.model == "LBX" && self.cmd("?am")?.trim() == "1" {
            "On"
        } else {
            "Off"
        };
        self.props
            .entry_mut("Analog modulation")
            .map(|e| e.value = PropertyValue::String(analog_mode.into()));

        let digital_mode = if self.model == "LBX" && self.cmd("?ttl")?.trim() == "1" {
            "On"
        } else {
            "Off"
        };
        self.props
            .entry_mut("Digital modulation")
            .map(|e| e.value = PropertyValue::String(digital_mode.into()));

        let hours = self.cmd("?hh")?.parse::<f64>().unwrap_or(0.0);
        self.props
            .entry_mut("Emission time (hours)")
            .map(|e| e.value = PropertyValue::Float(hours));

        let base_temp = self.cmd("?bt")?.parse::<f64>().unwrap_or(0.0);
        self.props
            .entry_mut("Base temperature")
            .map(|e| e.value = PropertyValue::Float(base_temp));

        Ok(())
    }
}

impl Default for LaserBoxx {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for LaserBoxx {
    fn name(&self) -> &str {
        "Oxxius LaserBoxx LBX or LMX or LCX"
    }

    fn description(&self) -> &str {
        "Oxxius LaserBoxx laser source"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Query model info to determine type and nominal power
        let info = self.cmd("inf?")?;
        self.nominal_power_mw = Self::parse_nominal_power(&info);
        self.model = info.split('-').next().unwrap_or("").to_string();
        self.props.entry_mut("Model").map(|e| {
            e.value = PropertyValue::String(Self::model_property_value(&info));
        });

        self.refresh_status_properties()?;

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.cmd("dl 0");
            self.is_open.set(false);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Serial number" if self.initialized => Ok(PropertyValue::String(self.cmd("hid?")?)),
            "Software version" if self.initialized => Ok(PropertyValue::String(self.cmd("?sv")?)),
            "Model" if self.initialized => Ok(PropertyValue::String(Self::model_property_value(
                &self.cmd("inf?")?,
            ))),
            "Laser status" if self.initialized => Ok(PropertyValue::String(
                Self::status_string(self.status_code()?)?.into(),
            )),
            "Emission" if self.initialized => {
                let open = self.refresh_open_from_status()?;
                Ok(PropertyValue::String(
                    if open { "On" } else { "Off" }.into(),
                ))
            }
            "Alarm" if self.initialized => {
                let code = self
                    .cmd("?f")?
                    .parse::<i64>()
                    .map_err(|_| MmError::SerialInvalidResponse)?;
                Ok(PropertyValue::String(Self::alarm_string(code)?.into()))
            }
            "OnInterlock" if self.initialized => {
                let s = if self.cmd("?int")?.trim() == "1" {
                    "Closed"
                } else {
                    "Open"
                };
                Ok(PropertyValue::String(s.into()))
            }
            "Control mode" if self.initialized => {
                let mode = if self.model == "LBX" && self.cmd("?acc")?.trim() == "1" {
                    "ACC"
                } else {
                    "APC"
                };
                Ok(PropertyValue::String(mode.into()))
            }
            "Monitored power (mW)" if self.initialized => Ok(PropertyValue::Float(
                Self::parse_f64_response(self.cmd("?p")?)?,
            )),
            "Power set point (%)" if self.initialized => {
                let pct = if self.model == "LBX" {
                    100.0 * Self::parse_f64_response(self.cmd("?sp")?)? / self.nominal_power_mw
                } else {
                    Self::parse_f64_response(self.cmd("ip")?)?
                };
                self.power_setpoint_mw.set(pct);
                Ok(PropertyValue::Float(pct))
            }
            "Power set point (%)" => Ok(PropertyValue::Float(self.power_setpoint_mw.get())),
            "Monitored current (mA)" if self.initialized && self.model != "LMX" => Ok(
                PropertyValue::Float(Self::parse_f64_response(self.cmd("?c")?)?),
            ),
            "Current set point (%)" if self.initialized => {
                let pct = if self.model == "LBX" {
                    Self::parse_f64_response(self.cmd("?sc")?)?
                } else {
                    0.0
                };
                self.current_setpoint_pct.set(pct);
                Ok(PropertyValue::Float(pct))
            }
            "Current set point (%)" => Ok(PropertyValue::Float(self.current_setpoint_pct.get())),
            "Sleep mode" if self.initialized => {
                let mode = if self.model != "LMX" && self.cmd("?t")?.trim() == "0" {
                    "Sleep"
                } else {
                    "Ready"
                };
                Ok(PropertyValue::String(mode.into()))
            }
            "Analog modulation" if self.initialized => {
                let mode = if self.model == "LBX" && self.cmd("?am")?.trim() == "1" {
                    "On"
                } else {
                    "Off"
                };
                Ok(PropertyValue::String(mode.into()))
            }
            "Digital modulation" if self.initialized => {
                let mode = if self.model == "LBX" && self.cmd("?ttl")?.trim() == "1" {
                    "On"
                } else {
                    "Off"
                };
                Ok(PropertyValue::String(mode.into()))
            }
            "Emission time (hours)" if self.initialized => Ok(PropertyValue::Float(
                Self::parse_f64_response(self.cmd("?hh")?)?,
            )),
            "Base temperature" if self.initialized => Ok(PropertyValue::Float(
                Self::parse_f64_response(self.cmd("?bt")?)?,
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "Emission" => {
                let s = match &val {
                    PropertyValue::String(s) => s.clone(),
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                if s != "On" && s != "Off" {
                    return Err(MmError::InvalidPropertyValue);
                }
                let open = s == "On";
                if self.initialized {
                    let cmd = if open { "dl 1" } else { "dl 0" };
                    self.cmd(cmd)?;
                    self.is_open.set(open);
                    thread::sleep(Duration::from_millis(500));
                }
                self.props.set(name, PropertyValue::String(s))
            }
            "Control mode" => {
                let mode = val.as_str().to_string();
                if mode != "APC" && mode != "ACC" {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized && self.model == "LBX" {
                    let old = self
                        .props
                        .get("Control mode")
                        .ok()
                        .and_then(|v| match v {
                            PropertyValue::String(s) => Some(s.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    if old != mode {
                        self.cmd("dl 0")?;
                        self.is_open.set(false);
                        let query = if mode == "ACC" { "1" } else { "0" };
                        self.cmd(&format!("acc {}", query))?;
                    }
                }
                self.props.set(name, PropertyValue::String(mode))
            }
            "Power set point (%)" => {
                let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0.0..=110.0).contains(&pct) {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized {
                    let cmd = if self.model == "LBX" {
                        let mw = self.nominal_power_mw * pct / 100.0;
                        format!("p {:.4}", mw)
                    } else {
                        format!("ip {}", pct)
                    };
                    self.cmd(&cmd)?;
                    self.power_setpoint_mw.set(pct);
                } else {
                    self.power_setpoint_mw.set(pct);
                }
                self.props
                    .entry_mut("Power set point (%)")
                    .map(|e| e.value = PropertyValue::Float(pct));
                Ok(())
            }
            "Current set point (%)" => {
                let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0.0..=125.0).contains(&pct) {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized && self.model == "LBX" {
                    self.cmd(&format!("c {}", pct))?;
                }
                self.current_setpoint_pct.set(pct);
                self.props
                    .entry_mut("Current set point (%)")
                    .map(|e| e.value = PropertyValue::Float(pct));
                Ok(())
            }
            "Sleep mode" => {
                let mode = val.as_str().to_string();
                if mode != "Sleep" && mode != "Ready" {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized && self.model != "LMX" {
                    let query = if mode == "Sleep" { "0" } else { "1" };
                    self.cmd(&format!("t {}", query))?;
                }
                self.props.set(name, PropertyValue::String(mode))
            }
            "Analog modulation" => {
                let mode = val.as_str().to_string();
                if mode != "On" && mode != "Off" {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized && self.model == "LBX" {
                    let query = if mode == "On" { "1" } else { "0" };
                    self.cmd(&format!("am {}", query))?;
                }
                self.props.set(name, PropertyValue::String(mode))
            }
            "Digital modulation" => {
                let mode = val.as_str().to_string();
                if mode != "On" && mode != "Off" {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized && self.model == "LBX" {
                    let query = if mode == "On" { "1" } else { "0" };
                    self.cmd(&format!("ttl {}", query))?;
                }
                self.props.set(name, PropertyValue::String(mode))
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

impl Shutter for LaserBoxx {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open { "dl 1" } else { "dl 0" };
        self.cmd(cmd)?;
        self.is_open.set(open);
        let emission = if open { "On" } else { "Off" };
        self.props
            .entry_mut("Emission")
            .map(|e| e.value = PropertyValue::String(emission.into()));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        if self.initialized {
            self.refresh_open_from_status()
        } else {
            Ok(self.is_open.get())
        }
    }

    fn fire(&mut self, delta_t: f64) -> MmResult<()> {
        self.set_open(true)?;
        thread::sleep(Duration::from_millis(delta_t.max(0.0) as u64));
        self.set_open(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("inf?", "LBX-473-100-CSB")
            .expect("hid?", "OXX-SN-001")
            .expect("?sv", "v2.3.1")
            .expect("inf?", "LBX-473-100-CSB")
            .expect("?sta", "2")
            .expect("?sta", "2")
            .expect("?f", "0")
            .expect("?int", "1")
            .expect("?acc", "0")
            .expect("?p", "0.0")
            .expect("?sp", "0.0")
            .expect("?c", "12.5")
            .expect("?sc", "7.5")
            .expect("?t", "1")
            .expect("?am", "0")
            .expect("?ttl", "0")
            .expect("?hh", "123.5")
            .expect("?bt", "31.2")
    }

    #[test]
    fn initialize_reads_fields() {
        let t = make_transport()
            .expect("?sta", "2")
            .expect("hid?", "OXX-SN-001")
            .expect("?int", "1")
            .expect("?f", "0")
            .expect("?bt", "31.2")
            .expect("?sc", "7.5");
        let mut dev = LaserBoxx::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(dev.nominal_power_mw, 100.0);
        assert_eq!(
            dev.get_property("Serial number").unwrap(),
            PropertyValue::String("OXX-SN-001".into())
        );
        assert_eq!(
            dev.get_property("OnInterlock").unwrap(),
            PropertyValue::String("Closed".into())
        );
        assert_eq!(
            dev.get_property("Alarm").unwrap(),
            PropertyValue::String("No alarm".into())
        );
        assert_eq!(
            dev.get_property("Base temperature").unwrap(),
            PropertyValue::Float(31.2)
        );
        assert_eq!(
            dev.get_property("Current set point (%)").unwrap(),
            PropertyValue::Float(7.5)
        );
    }

    #[test]
    fn open_close_emission() {
        let t = make_transport()
            .expect("dl 1", "")
            .expect("?sta", "3")
            .expect("dl 0", "")
            .expect("?sta", "2");
        let mut dev = LaserBoxx::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_power_setpoint() {
        let t = make_transport().any("OK");
        let mut dev = LaserBoxx::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Power set point (%)", PropertyValue::Float(50.0))
            .unwrap();
        assert_eq!(dev.power_setpoint_mw.get(), 50.0);
    }

    #[test]
    fn lcx_power_setpoint_uses_ip_command() {
        let t = MockTransport::new()
            .expect("inf?", "LCX-561-200-CPP")
            .expect("hid?", "OXX-SN-002")
            .expect("?sv", "v2.3.1")
            .expect("inf?", "LCX-561-200-CPP")
            .expect("?sta", "2")
            .expect("?sta", "2")
            .expect("?f", "0")
            .expect("?int", "1")
            .expect("?p", "0.0")
            .expect("ip", "0.0")
            .expect("?c", "12.5")
            .expect("?t", "1")
            .expect("?hh", "10")
            .expect("?bt", "30")
            .expect("ip 50", "OK");
        let mut dev = LaserBoxx::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Power set point (%)", PropertyValue::Float(50.0))
            .unwrap();
    }

    #[test]
    fn set_lbx_action_properties() {
        let t = make_transport()
            .expect("dl 0", "")
            .expect("acc 1", "")
            .expect("c 25", "")
            .expect("t 0", "")
            .expect("am 1", "")
            .expect("ttl 1", "");
        let mut dev = LaserBoxx::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Control mode", PropertyValue::String("ACC".into()))
            .unwrap();
        dev.set_property("Current set point (%)", PropertyValue::Float(25.0))
            .unwrap();
        dev.set_property("Sleep mode", PropertyValue::String("Sleep".into()))
            .unwrap();
        dev.set_property("Analog modulation", PropertyValue::String("On".into()))
            .unwrap();
        dev.set_property("Digital modulation", PropertyValue::String("On".into()))
            .unwrap();
    }

    #[test]
    fn parse_nominal_power() {
        assert_eq!(LaserBoxx::parse_nominal_power("LBX-473-100-CSB"), 100.0);
        assert_eq!(LaserBoxx::parse_nominal_power("LCX-561-200-CPP"), 200.0);
        assert_eq!(LaserBoxx::parse_nominal_power("LMX-638-50-B"), 50.0);
    }

    #[test]
    fn model_property_uses_model_and_wavelength() {
        assert_eq!(
            LaserBoxx::model_property_value("LBX-473-100-CSB"),
            "LBX-473"
        );
    }

    #[test]
    fn no_transport_error() {
        let mut dev = LaserBoxx::new();
        assert!(dev.initialize().is_err());
    }

    #[test]
    fn port_cannot_change_after_initialize() {
        let t = make_transport();
        let mut dev = LaserBoxx::new().with_transport(Box::new(t));
        dev.set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        dev.initialize().unwrap();
        assert_eq!(
            dev.set_property("Port", PropertyValue::String("COM2".into())),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            dev.get_property("Port").unwrap(),
            PropertyValue::String("COM1".into())
        );
    }

    #[test]
    fn unknown_status_and_alarm_codes_error() {
        let t = MockTransport::new()
            .expect("inf?", "LBX-473-100-CSB")
            .expect("hid?", "OXX-SN-001")
            .expect("?sv", "v2.3.1")
            .expect("inf?", "LBX-473-100-CSB")
            .expect("?sta", "99");
        let mut dev = LaserBoxx::new().with_transport(Box::new(t));
        assert_eq!(dev.initialize().unwrap_err(), MmError::UnknownPosition);

        let t = MockTransport::new()
            .expect("inf?", "LBX-473-100-CSB")
            .expect("hid?", "OXX-SN-001")
            .expect("?sv", "v2.3.1")
            .expect("inf?", "LBX-473-100-CSB")
            .expect("?sta", "2")
            .expect("?sta", "2")
            .expect("?f", "9");
        let mut dev = LaserBoxx::new().with_transport(Box::new(t));
        assert_eq!(dev.initialize().unwrap_err(), MmError::UnknownPosition);
    }

    #[test]
    fn shutdown_ignores_emission_off_error() {
        let t = make_transport();
        let mut dev = LaserBoxx::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.shutdown().unwrap();
        assert!(!dev.initialized);
    }
}
