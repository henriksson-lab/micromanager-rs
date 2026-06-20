use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

/// Coherent Cube laser controller.
///
/// Open = laser on (L=1), Closed = laser off (L=0).
pub struct CoherentCube {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    power_setpoint_mw: f64,
    min_power_mw: f64,
    max_power_mw: f64,
}

impl CoherentCube {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("PowerSetpoint", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("PowerReadback", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("State", &["0", "1"]).unwrap();
        props
            .define_property(
                "ExternalLaserPowerControl",
                PropertyValue::Integer(0),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("ExternalLaserPowerControl", &["0", "1"])
            .unwrap();
        props
            .define_property("CWMode", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("CWMode", &["0", "1"]).unwrap();
        props
            .define_property("HeadID", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property(
                "Head Usage Hours",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("Wavelength", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("Minimum Laser Power", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("Maximum Laser Power", PropertyValue::Float(0.0), true)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            power_setpoint_mw: 0.0,
            min_power_mw: 0.0,
            max_power_mw: 100.0,
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

    /// Send `?TOKEN` and parse the `TOKEN=value` response.
    fn query(&mut self, token: &str) -> MmResult<String> {
        let cmd = format!("?{}", token);
        let tok = token.to_string();
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Self::parse_response(&tok, &resp)
        })
    }

    /// Send `TOKEN=value` and parse the echoed `TOKEN=achieved` response.
    fn set_token(&mut self, token: &str, value: &str) -> MmResult<String> {
        let cmd = format!("{}={}", token, value);
        let tok = token.to_string();
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Self::parse_set_response(&tok, &resp)
        })
    }

    fn parse_response(token: &str, resp: &str) -> MmResult<String> {
        let resp = resp.trim();
        // Expected format: "TOKEN=value"
        if let Some(eq) = resp.find('=') {
            let key = &resp[..eq];
            let val = &resp[eq + 1..];
            if key == token {
                return Ok(val.to_string());
            }
        }
        // Some responses may just be a bare value (e.g. acknowledgement lines)
        Ok(resp.to_string())
    }

    fn parse_set_response(token: &str, resp: &str) -> MmResult<String> {
        let resp = resp.trim();
        let Some(eq) = resp.find('=') else {
            return Err(MmError::SerialInvalidResponse);
        };
        let key = &resp[..eq];
        if key != token {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(resp[eq + 1..].to_string())
    }

    fn set_power_setpoint(&mut self, requested_mw: f64) -> MmResult<f64> {
        let achieved = self
            .set_token("P", &format!("{:.4}", requested_mw))?
            .parse::<f64>()
            .unwrap_or(0.0);

        if requested_mw != 0.0 {
            let fraction_error = ((achieved - requested_mw) / requested_mw).abs();
            if fraction_error > 0.05 && fraction_error < 0.10 {
                return Ok(achieved);
            }
        }

        Ok(requested_mw)
    }

    /// Read and discard the greeting banner (empty lines).
    fn read_greeting(&mut self) -> MmResult<()> {
        loop {
            let line = self.call_transport(|t| t.receive_line())?;
            if line.trim().is_empty() {
                break;
            }
        }
        Ok(())
    }
}

impl Default for CoherentCube {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CoherentCube {
    fn name(&self) -> &str {
        "CoherentCube"
    }

    fn description(&self) -> &str {
        "CoherentCube Laser"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Read greeting banner
        self.read_greeting()?;

        // Disable echo (E=0)
        let _ = self.set_token("E", "0");

        // Disable command prompt (>=0)
        let _ = self.set_token(">", "0");

        // Disable CDRH delay
        let _ = self.set_token("CDRH", "0");

        // Enable TEC servo
        let _ = self.set_token("T", "1");

        // Disable external power control
        let _ = self.set_token("EXT", "0");
        self.props
            .entry_mut("ExternalLaserPowerControl")
            .map(|e| e.value = PropertyValue::Integer(0));

        // Query power limits
        if let Ok(val) = self.query("MINLP") {
            self.min_power_mw = val.parse().unwrap_or(0.0);
            self.props
                .entry_mut("Minimum Laser Power")
                .map(|e| e.value = PropertyValue::Float(self.min_power_mw));
        }
        if let Ok(val) = self.query("MAXLP") {
            self.max_power_mw = val.parse().unwrap_or(100.0);
            self.props
                .entry_mut("Maximum Laser Power")
                .map(|e| e.value = PropertyValue::Float(self.max_power_mw));
        }
        self.props
            .set_property_limits("PowerSetpoint", self.min_power_mw, self.max_power_mw)?;

        // Query read-only ID fields
        if let Ok(hid) = self.query("HID") {
            self.props
                .entry_mut("HeadID")
                .map(|e| e.value = PropertyValue::String(hid));
        }
        if let Ok(hh) = self.query("HH") {
            self.props
                .entry_mut("Head Usage Hours")
                .map(|e| e.value = PropertyValue::String(hh));
        }
        if let Ok(wave) = self.query("WAVE") {
            let nm = wave.parse::<f64>().unwrap_or(0.0);
            self.props
                .entry_mut("Wavelength")
                .map(|e| e.value = PropertyValue::Float(nm));
        }

        // Query current state
        if let Ok(l) = self.query("L") {
            self.is_open = l.trim() == "1";
            self.props
                .entry_mut("State")
                .map(|e| e.value = PropertyValue::Integer(if self.is_open { 1 } else { 0 }));
        }
        if let Ok(sp) = self.query("SP") {
            self.power_setpoint_mw = sp.parse().unwrap_or(0.0);
            self.props
                .entry_mut("PowerSetpoint")
                .map(|e| e.value = PropertyValue::Float(self.power_setpoint_mw));
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "PowerSetpoint" => Ok(PropertyValue::Float(self.power_setpoint_mw)),
            "State" => Ok(PropertyValue::Integer(if self.is_open { 1 } else { 0 })),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "PowerSetpoint" => {
                let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                let stored_mw = if self.initialized {
                    self.set_power_setpoint(mw)?
                } else {
                    mw
                };
                self.power_setpoint_mw = stored_mw;
                self.props
                    .entry_mut("PowerSetpoint")
                    .map(|e| e.value = PropertyValue::Float(stored_mw));
                Ok(())
            }
            "State" => {
                let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                match state {
                    0 | 1 => self.set_open(state == 1),
                    _ => Err(MmError::InvalidPropertyValue),
                }
            }
            "CWMode" => {
                let mode = val.to_string();
                if self.initialized {
                    self.set_token("CW", &mode)?;
                }
                self.props.set(name, val)
            }
            "ExternalLaserPowerControl" => {
                let control = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if control != 0 && control != 1 {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized {
                    self.set_token("EXT", &control.to_string())?;
                }
                self.props.set(name, PropertyValue::Integer(control))
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

impl Shutter for CoherentCube {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let val = if open { "1" } else { "0" };
        self.set_token("L", val)?;
        self.is_open = open;
        self.props
            .entry_mut("State")
            .map(|e| e.value = PropertyValue::Integer(if open { 1 } else { 0 }));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.is_open)
    }

    fn fire(&mut self, delta_t: f64) -> MmResult<()> {
        self.set_open(true)?;
        std::thread::sleep(std::time::Duration::from_millis(
            delta_t.max(0.0).round() as u64
        ));
        self.set_open(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            // read_greeting: one non-empty line then empty
            .any("CoherentCube v1.0")
            .any("")
            // E=0, >=0, CDRH=0, T=1, EXT=0
            .any("E=0")
            .any(">=0")
            .any("CDRH=0")
            .any("T=1")
            .any("EXT=0")
            // ?MINLP, ?MAXLP
            .any("MINLP=0.0")
            .any("MAXLP=100.0")
            // ?HID, ?HH, ?WAVE
            .any("HID=SN-001")
            .any("HH=100.5")
            .any("WAVE=488.0")
            // ?L, ?SP
            .any("L=0")
            .any("SP=10.0")
    }

    #[test]
    fn initialize() {
        let mut dev = CoherentCube::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(dev.power_setpoint_mw, 10.0);
        assert_eq!(dev.max_power_mw, 100.0);
        assert!(dev.has_property("State"));
        assert_eq!(
            dev.get_property("State").unwrap(),
            PropertyValue::Integer(0)
        );
    }

    #[test]
    fn open_close() {
        let transport = make_transport()
            .any("L=1") // set_open(true) → L=1=response
            .any("L=0"); // set_open(false)
        let mut dev = CoherentCube::new().with_transport(Box::new(transport));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        assert_eq!(
            dev.get_property("State").unwrap(),
            PropertyValue::Integer(1)
        );
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(
            dev.get_property("State").unwrap(),
            PropertyValue::Integer(0)
        );
    }

    #[test]
    fn set_power() {
        let transport = make_transport().any("P=50.0000"); // set_property PowerSetpoint
        let mut dev = CoherentCube::new().with_transport(Box::new(transport));
        dev.initialize().unwrap();
        dev.set_property("PowerSetpoint", PropertyValue::Float(50.0))
            .unwrap();
        assert_eq!(dev.power_setpoint_mw, 50.0);
    }

    #[test]
    fn set_power_keeps_achieved_value_when_echo_is_close_but_different() {
        let transport = make_transport().any("P=46.0000");
        let mut dev = CoherentCube::new().with_transport(Box::new(transport));
        dev.initialize().unwrap();
        dev.set_property("PowerSetpoint", PropertyValue::Float(50.0))
            .unwrap();

        assert_eq!(dev.power_setpoint_mw, 46.0);
        assert_eq!(
            dev.get_property("PowerSetpoint").unwrap(),
            PropertyValue::Float(46.0)
        );
    }

    #[test]
    fn set_token_rejects_wrong_echo_token() {
        let transport = make_transport().any("Q=50.0000");
        let mut dev = CoherentCube::new().with_transport(Box::new(transport));
        dev.initialize().unwrap();

        assert_eq!(
            dev.set_property("PowerSetpoint", PropertyValue::Float(50.0))
                .unwrap_err(),
            MmError::SerialInvalidResponse
        );
        assert_eq!(dev.power_setpoint_mw, 10.0);
    }

    #[test]
    fn set_external_power_control() {
        let transport = make_transport().any("EXT=1");
        let mut dev = CoherentCube::new().with_transport(Box::new(transport));
        dev.initialize().unwrap();
        dev.set_property("ExternalLaserPowerControl", PropertyValue::Integer(1))
            .unwrap();
        assert_eq!(
            dev.get_property("ExternalLaserPowerControl").unwrap(),
            PropertyValue::Integer(1)
        );
    }

    #[test]
    fn fire_closes_after_pulse() {
        let transport = make_transport().any("L=1").any("L=0");
        let mut dev = CoherentCube::new().with_transport(Box::new(transport));
        dev.initialize().unwrap();
        dev.fire(0.0).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() {
        let mut dev = CoherentCube::new();
        assert!(dev.initialize().is_err());
    }
}
