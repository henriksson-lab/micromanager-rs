/// Coherent Sapphire laser controller.
///
/// Token-based protocol identical to CoherentCube but with different tokens:
///   Query: `?TOKEN\r`  → value or `TOKEN=value`
///   Set:   `TOKEN=value\r` → echoed response
///
/// Power range is fixed (0.5–50 mW) and wavelength is fixed (561 nm).
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const MIN_POWER_MW: f64 = 0.5;
const MAX_POWER_MW: f64 = 50.0;
const WAVELENGTH_NM: f64 = 561.0;

pub struct Sapphire {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    power_setpoint_mw: f64,
}

impl Sapphire {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("PowerSetpoint_mW", PropertyValue::Float(0.0), false).unwrap();
        props.set_property_limits("PowerSetpoint_mW", MIN_POWER_MW, MAX_POWER_MW).unwrap();
        props.define_property("PowerReadback_mW", PropertyValue::Float(0.0), true).unwrap();
        props.define_property("HeadID", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("HeadUsageHours", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("Wavelength_nm", PropertyValue::Float(WAVELENGTH_NM), true).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            power_setpoint_mw: 0.0,
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

    /// Send `?TOKEN` and parse `TOKEN=value` or bare value.
    fn query(&mut self, token: &str) -> MmResult<String> {
        let cmd = format!("?{}", token);
        let tok = token.to_string();
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            let resp = resp.trim();
            if let Some(eq) = resp.find('=') {
                let key = &resp[..eq];
                if key == tok {
                    return Ok(resp[eq + 1..].to_string());
                }
            }
            Ok(resp.to_string())
        })
    }

    /// Send `TOKEN=value` and discard the response.
    fn set_token(&mut self, token: &str, value: &str) -> MmResult<()> {
        let cmd = format!("{}={}", token, value);
        self.call_transport(|t| {
            t.send(&cmd)?;
            let _ = t.receive_line();
            Ok(())
        })
    }

    /// Read and discard greeting lines until an empty line is encountered.
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

impl Default for Sapphire {
    fn default() -> Self { Self::new() }
}

impl Device for Sapphire {
    fn name(&self) -> &str { "Sapphire" }
    fn description(&self) -> &str { "Coherent Sapphire laser controller" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        self.read_greeting()?;

        let _ = self.set_token("E", "0");   // disable echo
        let _ = self.set_token(">", "0");   // disable prompt
        let _ = self.set_token("T", "1");   // enable TEC servo

        if let Ok(hid) = self.query("HID") {
            self.props.entry_mut("HeadID").map(|e| e.value = PropertyValue::String(hid));
        }
        if let Ok(hh) = self.query("HH") {
            self.props.entry_mut("HeadUsageHours").map(|e| e.value = PropertyValue::String(hh));
        }

        if let Ok(l) = self.query("L") {
            self.is_open = l.trim() == "1";
        }
        if let Ok(p) = self.query("P") {
            self.power_setpoint_mw = p.parse().unwrap_or(0.0);
            self.props.entry_mut("PowerSetpoint_mW")
                .map(|e| e.value = PropertyValue::Float(self.power_setpoint_mw));
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.set_token("L", "0");
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "PowerSetpoint_mW" => Ok(PropertyValue::Float(self.power_setpoint_mw)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "PowerSetpoint_mW" => {
                let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.set_token("P", &format!("{:.5}", mw))?;
                }
                self.power_setpoint_mw = mw;
                self.props.entry_mut("PowerSetpoint_mW")
                    .map(|e| e.value = PropertyValue::Float(mw));
                Ok(())
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

impl Shutter for Sapphire {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let val = if open { "1" } else { "0" };
        self.set_token("L", val)?;
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
            // read_greeting: one banner line + empty
            .any("Sapphire 561-20 CDRH v1.01").any("")
            // E=0, >=0, T=1
            .any("E=0").any(">=0").any("T=1")
            // ?HID, ?HH
            .any("HID=SAP-001").any("HH=50.0")
            // ?L → 0, ?P → 10.0
            .any("L=0").any("P=10.0")
    }

    #[test]
    fn initialize() {
        let mut dev = Sapphire::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(dev.power_setpoint_mw, 10.0);
    }

    #[test]
    fn open_close() {
        let t = make_transport().any("L=1").any("L=0");
        let mut dev = Sapphire::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_power() {
        let t = make_transport().any("P=25.00000");
        let mut dev = Sapphire::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("PowerSetpoint_mW", PropertyValue::Float(25.0)).unwrap();
        assert_eq!(dev.power_setpoint_mw, 25.0);
    }

    #[test]
    fn no_transport_error() {
        let mut dev = Sapphire::new();
        assert!(dev.initialize().is_err());
    }
}
