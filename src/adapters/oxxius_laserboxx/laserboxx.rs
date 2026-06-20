use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
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
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    power_setpoint_mw: f64,
    current_setpoint_pct: f64,
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
            is_open: false,
            power_setpoint_mw: 0.0,
            current_setpoint_pct: 0.0,
            nominal_power_mw: 100.0,
            model: String::new(),
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

    /// Parse nominal power from model string e.g. "LBX-473-100-CSB" → 100.0 mW.
    fn parse_nominal_power(model: &str) -> f64 {
        model
            .split('-')
            .nth(2)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(100.0)
    }

    fn status_string(code: i64) -> &'static str {
        match code {
            1 => "Warm-up phase",
            2 => "Stand-by for emission",
            3 => "Emission is on",
            4 => "Alarm raised",
            5 => "Internal error raised",
            6 => "Sleep mode",
            7 => "Searching for SLM point",
            _ => "Unknown",
        }
    }

    fn alarm_string(code: i64) -> &'static str {
        match code {
            0 => "No alarm",
            1 => "Out-of-bounds current",
            2 => "Out-of-bounds power",
            3 => "Out-of-bounds supply voltage",
            4 => "Out-of-bounds inner temperature",
            5 => "Out-of-bounds laser head temperature",
            7 => "Interlock circuit open",
            8 => "Manual reset",
            _ => "Unknown alarm",
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

        if let Ok(sn) = self.cmd("hid?") {
            self.props
                .entry_mut("Serial number")
                .map(|e| e.value = PropertyValue::String(sn));
        }
        if let Ok(sv) = self.cmd("?sv") {
            self.props
                .entry_mut("Software version")
                .map(|e| e.value = PropertyValue::String(sv));
        }
        if let Ok(hh) = self.cmd("?hh") {
            let hours = hh.parse::<f64>().unwrap_or(0.0);
            self.props
                .entry_mut("Emission time (hours)")
                .map(|e| e.value = PropertyValue::Float(hours));
        }
        if let Ok(bt) = self.cmd("?bt") {
            let temp = bt.parse::<f64>().unwrap_or(0.0);
            self.props
                .entry_mut("Base temperature")
                .map(|e| e.value = PropertyValue::Float(temp));
        }
        if let Ok(f) = self.cmd("?f") {
            let code = f.parse::<i64>().unwrap_or(0);
            let alarm = Self::alarm_string(code);
            self.props
                .entry_mut("Alarm")
                .map(|e| e.value = PropertyValue::String(alarm.into()));
        }
        if let Ok(i) = self.cmd("?int") {
            let s = if i.trim() == "1" { "Closed" } else { "Open" };
            self.props
                .entry_mut("OnInterlock")
                .map(|e| e.value = PropertyValue::String(s.into()));
        }

        // Query initial status
        if let Ok(sta) = self.cmd("?sta") {
            let code = sta.parse::<i64>().unwrap_or(0);
            self.is_open = matches!(code, 1 | 3 | 7);
            let status = Self::status_string(code);
            self.props
                .entry_mut("Laser status")
                .map(|e| e.value = PropertyValue::String(status.into()));
            let emission = if self.is_open { "On" } else { "Off" };
            self.props
                .entry_mut("Emission")
                .map(|e| e.value = PropertyValue::String(emission.into()));
        }

        // Query power readback
        if let Ok(p) = self.cmd("?p") {
            let mw = p.parse::<f64>().unwrap_or(0.0);
            self.props
                .entry_mut("Monitored power (mW)")
                .map(|e| e.value = PropertyValue::Float(mw));
        }
        if self.model != "LMX" {
            if let Ok(c) = self.cmd("?c") {
                let ma = c.parse::<f64>().unwrap_or(0.0);
                self.props
                    .entry_mut("Monitored current (mA)")
                    .map(|e| e.value = PropertyValue::Float(ma));
            }
        }
        if self.model == "LBX" {
            if let Ok(acc) = self.cmd("?acc") {
                let mode = if acc.trim() == "1" { "ACC" } else { "APC" };
                self.props
                    .entry_mut("Control mode")
                    .map(|e| e.value = PropertyValue::String(mode.into()));
            }
            if let Ok(sc) = self.cmd("?sc") {
                let pct = sc.parse::<f64>().unwrap_or(0.0);
                self.current_setpoint_pct = pct;
                self.props
                    .entry_mut("Current set point (%)")
                    .map(|e| e.value = PropertyValue::Float(pct));
            }
            if let Ok(am) = self.cmd("?am") {
                let mode = if am.trim() == "1" { "On" } else { "Off" };
                self.props
                    .entry_mut("Analog modulation")
                    .map(|e| e.value = PropertyValue::String(mode.into()));
            }
            if let Ok(ttl) = self.cmd("?ttl") {
                let mode = if ttl.trim() == "1" { "On" } else { "Off" };
                self.props
                    .entry_mut("Digital modulation")
                    .map(|e| e.value = PropertyValue::String(mode.into()));
            }
        }
        if self.model != "LMX" {
            if let Ok(t) = self.cmd("?t") {
                let mode = if t.trim() == "0" { "Sleep" } else { "Ready" };
                self.props
                    .entry_mut("Sleep mode")
                    .map(|e| e.value = PropertyValue::String(mode.into()));
            }
        } else {
            self.props
                .entry_mut("Sleep mode")
                .map(|e| e.value = PropertyValue::String("Ready".into()));
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.cmd("dl 0");
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Power set point (%)" => Ok(PropertyValue::Float(self.power_setpoint_mw)),
            "Current set point (%)" => Ok(PropertyValue::Float(self.current_setpoint_pct)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
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
                    self.is_open = open;
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
                        self.is_open = false;
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
                    self.power_setpoint_mw = pct;
                } else {
                    self.power_setpoint_mw = pct;
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
                self.current_setpoint_pct = pct;
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
        self.is_open = open;
        let emission = if open { "On" } else { "Off" };
        self.props
            .entry_mut("Emission")
            .map(|e| e.value = PropertyValue::String(emission.into()));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.is_open)
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

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("inf?", "LBX-473-100-CSB")
            .expect("hid?", "OXX-SN-001")
            .expect("?sv", "v2.3.1")
            .expect("?hh", "123.5")
            .expect("?bt", "31.2")
            .expect("?f", "0")
            .expect("?int", "1")
            .expect("?sta", "2")
            .expect("?p", "0.0")
            .expect("?c", "12.5")
            .expect("?acc", "0")
            .expect("?sc", "7.5")
            .expect("?am", "0")
            .expect("?ttl", "0")
            .expect("?t", "1")
    }

    #[test]
    fn initialize_reads_fields() {
        let mut dev = LaserBoxx::new().with_transport(Box::new(make_transport()));
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
        let t = make_transport().expect("dl 1", "").expect("dl 0", "");
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
        assert_eq!(dev.power_setpoint_mw, 50.0);
    }

    #[test]
    fn lcx_power_setpoint_uses_ip_command() {
        let t = MockTransport::new()
            .expect("inf?", "LCX-561-200-CPP")
            .expect("hid?", "OXX-SN-002")
            .expect("?sv", "v2.3.1")
            .expect("?hh", "10")
            .expect("?bt", "30")
            .expect("?f", "0")
            .expect("?int", "1")
            .expect("?sta", "2")
            .expect("?p", "0.0")
            .expect("?c", "12.5")
            .expect("?t", "1")
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
}
