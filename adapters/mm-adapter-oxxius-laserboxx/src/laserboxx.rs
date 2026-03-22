use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

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
    nominal_power_mw: f64,
    model: String,
}

impl LaserBoxx {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Model", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("SerialNumber", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("SoftwareVersion", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("LaserStatus", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("Emission", PropertyValue::String("Off".into()), false).unwrap();
        props.define_property("Alarm", PropertyValue::String("No alarm".into()), true).unwrap();
        props.define_property("Interlock", PropertyValue::String("Unknown".into()), true).unwrap();
        props.define_property("ControlMode", PropertyValue::String("APC".into()), false).unwrap();
        props.define_property("PowerMonitored_mW", PropertyValue::Float(0.0), true).unwrap();
        props.define_property("PowerSetpoint_pct", PropertyValue::Float(0.0), false).unwrap();
        props.set_property_limits("PowerSetpoint_pct", 0.0, 110.0).unwrap();
        props.define_property("UsageHours", PropertyValue::Float(0.0), true).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            power_setpoint_mw: 0.0,
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
        model.split('-')
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
        self.props.entry_mut("Model").map(|e| e.value = PropertyValue::String(info));

        if let Ok(sn) = self.cmd("hid?") {
            self.props.entry_mut("SerialNumber").map(|e| e.value = PropertyValue::String(sn));
        }
        if let Ok(sv) = self.cmd("?sv") {
            self.props.entry_mut("SoftwareVersion").map(|e| e.value = PropertyValue::String(sv));
        }
        if let Ok(hh) = self.cmd("?hh") {
            let hours = hh.parse::<f64>().unwrap_or(0.0);
            self.props.entry_mut("UsageHours").map(|e| e.value = PropertyValue::Float(hours));
        }
        if let Ok(f) = self.cmd("?f") {
            let code = f.parse::<i64>().unwrap_or(0);
            let alarm = Self::alarm_string(code);
            self.props.entry_mut("Alarm").map(|e| e.value = PropertyValue::String(alarm.into()));
        }
        if let Ok(i) = self.cmd("?int") {
            let s = if i.trim() == "1" { "Closed" } else { "Open" };
            self.props.entry_mut("Interlock").map(|e| e.value = PropertyValue::String(s.into()));
        }

        // Query initial status
        if let Ok(sta) = self.cmd("?sta") {
            let code = sta.parse::<i64>().unwrap_or(0);
            self.is_open = code == 3;
            let status = Self::status_string(code);
            self.props.entry_mut("LaserStatus").map(|e| e.value = PropertyValue::String(status.into()));
            let emission = if self.is_open { "On" } else { "Off" };
            self.props.entry_mut("Emission").map(|e| e.value = PropertyValue::String(emission.into()));
        }

        // Query power readback
        if let Ok(p) = self.cmd("?p") {
            let mw = p.parse::<f64>().unwrap_or(0.0);
            self.props.entry_mut("PowerMonitored_mW").map(|e| e.value = PropertyValue::Float(mw));
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
            "PowerSetpoint_pct" => Ok(PropertyValue::Float(self.power_setpoint_mw)),
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
                let open = s == "On";
                if self.initialized {
                    let cmd = if open { "dl 1" } else { "dl 0" };
                    self.cmd(cmd)?;
                    self.is_open = open;
                }
                self.props.set(name, PropertyValue::String(s))
            }
            "PowerSetpoint_pct" => {
                let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    let mw = self.nominal_power_mw * pct / 100.0;
                    let cmd = format!("p {:.4}", mw);
                    self.cmd(&cmd)?;
                    self.power_setpoint_mw = pct;
                } else {
                    self.power_setpoint_mw = pct;
                }
                self.props.entry_mut("PowerSetpoint_pct")
                    .map(|e| e.value = PropertyValue::Float(pct));
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
        self.props.entry_mut("Emission")
            .map(|e| e.value = PropertyValue::String(emission.into()));
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
            .expect("inf?", "LBX-473-100-CSB")
            .expect("hid?", "OXX-SN-001")
            .expect("?sv", "v2.3.1")
            .expect("?hh", "123.5")
            .expect("?f", "0")
            .expect("?int", "1")
            .expect("?sta", "2")
            .expect("?p", "0.0")
    }

    #[test]
    fn initialize_reads_fields() {
        let mut dev = LaserBoxx::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(dev.nominal_power_mw, 100.0);
        assert_eq!(
            dev.get_property("SerialNumber").unwrap(),
            PropertyValue::String("OXX-SN-001".into())
        );
        assert_eq!(
            dev.get_property("Interlock").unwrap(),
            PropertyValue::String("Closed".into())
        );
        assert_eq!(
            dev.get_property("Alarm").unwrap(),
            PropertyValue::String("No alarm".into())
        );
    }

    #[test]
    fn open_close_emission() {
        let t = make_transport()
            .expect("dl 1", "")
            .expect("dl 0", "");
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
        dev.set_property("PowerSetpoint_pct", PropertyValue::Float(50.0)).unwrap();
        assert_eq!(dev.power_setpoint_mw, 50.0);
    }

    #[test]
    fn parse_nominal_power() {
        assert_eq!(LaserBoxx::parse_nominal_power("LBX-473-100-CSB"), 100.0);
        assert_eq!(LaserBoxx::parse_nominal_power("LCX-561-200-CPP"), 200.0);
        assert_eq!(LaserBoxx::parse_nominal_power("LMX-638-50-B"), 50.0);
    }

    #[test]
    fn no_transport_error() {
        let mut dev = LaserBoxx::new();
        assert!(dev.initialize().is_err());
    }
}
