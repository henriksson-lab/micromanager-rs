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
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct LaserQuantumLaser {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    power_mw: f64,
    current_pct: f64,
    max_power_mw: f64,
}

impl LaserQuantumLaser {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("PowerSetpoint_mW", PropertyValue::Float(0.0), false).unwrap();
        props.define_property("Current_pct", PropertyValue::Float(0.0), false).unwrap();
        props.set_property_limits("Current_pct", 0.0, 100.0).unwrap();
        props.define_property("ControlMode", PropertyValue::String("POWER".into()), false).unwrap();
        props.set_allowed_values("ControlMode", &["POWER", "CURRENT"]).unwrap();
        props.define_property("Version", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("PsuTime_h", PropertyValue::Float(0.0), true).unwrap();
        props.define_property("LaserEnabledTime_h", PropertyValue::Float(0.0), true).unwrap();
        props.define_property("LaserOpTime_h", PropertyValue::Float(0.0), true).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            power_mw: 0.0,
            current_pct: 0.0,
            max_power_mw: 1000.0,
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

    /// Parse a numeric response that may have a trailing unit suffix (e.g. "125.3mW" → 125.3).
    fn parse_numeric(s: &str) -> f64 {
        // Strip non-numeric tail (%, mW, C, etc.)
        let trimmed = s.trim();
        let end = trimmed
            .find(|c: char| c != '.' && c != '-' && !c.is_ascii_digit())
            .unwrap_or(trimmed.len());
        trimmed[..end].parse().unwrap_or(0.0)
    }
}

impl Default for LaserQuantumLaser {
    fn default() -> Self { Self::new() }
}

impl Device for LaserQuantumLaser {
    fn name(&self) -> &str { "LaserQuantumLaser" }
    fn description(&self) -> &str { "LaserQuantum Gem/Ventus/Opus/Axiom laser" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        let ver = self.cmd("VERSION?")?;
        if !ver.contains("SMD12") {
            return Err(MmError::LocallyDefined(
                format!("Unexpected version string: {}", ver)
            ));
        }
        self.props.entry_mut("Version").map(|e| e.value = PropertyValue::String(ver));

        let status = self.cmd("STATUS?")?;
        self.is_open = status.trim().eq_ignore_ascii_case("enabled");

        let ctrl = self.cmd("CONTROL?")?;
        self.props.entry_mut("ControlMode")
            .map(|e| e.value = PropertyValue::String(ctrl.trim().to_uppercase()));

        // Timers (4 responses: 3 data lines + empty)
        if let Ok(line1) = self.cmd("TIMERS?") {
            let psu = Self::parse_numeric(&line1);
            self.props.entry_mut("PsuTime_h").map(|e| e.value = PropertyValue::Float(psu));
        }
        // Read remaining timer lines
        let _ = self.call_transport(|t| { let r = t.receive_line()?; Ok(r) });
        let _ = self.call_transport(|t| { let r = t.receive_line()?; Ok(r) });
        let _ = self.call_transport(|t| { let r = t.receive_line()?; Ok(r) });

        // Current power and current
        if let Ok(p) = self.cmd("POWER?") {
            self.power_mw = Self::parse_numeric(&p);
            self.props.entry_mut("PowerSetpoint_mW")
                .map(|e| e.value = PropertyValue::Float(self.power_mw));
        }
        if let Ok(c) = self.cmd("CURRENT?") {
            self.current_pct = Self::parse_numeric(&c);
            self.props.entry_mut("Current_pct")
                .map(|e| e.value = PropertyValue::Float(self.current_pct));
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
            "PowerSetpoint_mW" => Ok(PropertyValue::Float(self.power_mw)),
            "Current_pct" => Ok(PropertyValue::Float(self.current_pct)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "PowerSetpoint_mW" => {
                let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.cmd(&format!("POWER={:.4}", mw))?;
                }
                self.power_mw = mw;
                self.props.entry_mut("PowerSetpoint_mW")
                    .map(|e| e.value = PropertyValue::Float(mw));
                Ok(())
            }
            "Current_pct" => {
                let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.cmd(&format!("CURRENT={:.4}", pct))?;
                }
                self.current_pct = pct;
                self.props.entry_mut("Current_pct")
                    .map(|e| e.value = PropertyValue::Float(pct));
                Ok(())
            }
            "ControlMode" => {
                let mode = val.as_str().to_uppercase();
                if self.initialized {
                    self.cmd(&format!("CONTROL={}", mode))?;
                }
                self.props.set(name, PropertyValue::String(mode))
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Shutter }
    fn busy(&self) -> bool { false }
}

impl Shutter for LaserQuantumLaser {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open { "ON" } else { "OFF" };
        self.cmd(cmd)?;
        self.is_open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.is_open) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        self.set_open(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("VERSION?", "SMD12 v2.0")
            .expect("STATUS?", "DISABLED")
            .expect("CONTROL?", "POWER")
            // TIMERS?: 4 responses
            .expect("TIMERS?", "PSU Time = 100.5 Hours")
            .any("Laser Enabled Time = 50.2 Hours")
            .any("Laser Operation Time = 48.0 Hours")
            .any("")
            // POWER?, CURRENT?
            .expect("POWER?", "50.0mW")
            .expect("CURRENT?", "30.0%")
    }

    #[test]
    fn initialize() {
        let mut dev = LaserQuantumLaser::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(dev.power_mw, 50.0);
        assert_eq!(dev.current_pct, 30.0);
    }

    #[test]
    fn open_close() {
        let t = make_transport()
            .expect("ON", "OK")
            .expect("OFF", "OK");
        let mut dev = LaserQuantumLaser::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_power() {
        let t = make_transport().any("OK");
        let mut dev = LaserQuantumLaser::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("PowerSetpoint_mW", PropertyValue::Float(80.0)).unwrap();
        assert_eq!(dev.power_mw, 80.0);
    }

    #[test]
    fn parse_numeric_strips_units() {
        assert_eq!(LaserQuantumLaser::parse_numeric("125.3mW"), 125.3);
        assert_eq!(LaserQuantumLaser::parse_numeric("45.5%"), 45.5);
        assert_eq!(LaserQuantumLaser::parse_numeric("23.1C"), 23.1);
    }

    #[test]
    fn no_transport_error() {
        let mut dev = LaserQuantumLaser::new();
        assert!(dev.initialize().is_err());
    }
}
