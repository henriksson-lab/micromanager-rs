/// Coherent OBIS laser controller.
///
/// Uses SCPI-style commands:
///   Query:  `TOKEN?\r`  → plain value response
///   Set:    `TOKEN value\r` → plain acknowledgement response
///
/// Power is reported by the device in Watts; this adapter stores/exposes mW.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct CoherentObis {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    power_setpoint_mw: f64,
    min_power_mw: f64,
    max_power_mw: f64,
    /// SCPI channel index (default 1).
    channel: u8,
}

impl CoherentObis {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("PowerSetpoint_mW", PropertyValue::Float(0.0), false).unwrap();
        props.define_property("PowerReadback_mW", PropertyValue::Float(0.0), true).unwrap();
        props.define_property("SerialNumber", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("HeadUsageHours", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("Wavelength_nm", PropertyValue::Float(0.0), true).unwrap();
        props.define_property("MinPower_mW", PropertyValue::Float(0.0), true).unwrap();
        props.define_property("MaxPower_mW", PropertyValue::Float(0.0), true).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            power_setpoint_mw: 0.0,
            min_power_mw: 0.0,
            max_power_mw: 100.0,
            channel: 1,
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

    fn sour(&self) -> String { format!("SOUR{}:", self.channel) }
    fn syst(&self) -> String { format!("SYST{}:", self.channel) }

    /// Send `TOKEN?\r` and return the trimmed response.
    fn query(&mut self, token: &str) -> MmResult<String> {
        let cmd = format!("{}?", token);
        let resp = self.call_transport(|t| t.send_recv(&cmd))?;
        Ok(resp.trim().to_string())
    }

    /// Send `TOKEN value\r` and discard the response.
    fn set_cmd(&mut self, token: &str, value: &str) -> MmResult<()> {
        let cmd = format!("{} {}", token, value);
        self.call_transport(|t| {
            t.send(&cmd)?;
            let _ = t.receive_line();
            Ok(())
        })
    }
}

impl Default for CoherentObis {
    fn default() -> Self { Self::new() }
}

impl Device for CoherentObis {
    fn name(&self) -> &str { "CoherentOBIS" }
    fn description(&self) -> &str { "Coherent OBIS laser controller" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Handshake and setup
        let syst = self.syst();
        let _ = self.call_transport(|t| t.send_recv(&format!("{}COMM:HAND On", syst)));
        let syst = self.syst();
        let _ = self.call_transport(|t| t.send_recv(&format!("{}COMM:PROM Off", syst)));
        let syst = self.syst();
        let _ = self.call_transport(|t| t.send_recv(&format!("{}ERR:CLE", syst)));

        // Power limits (device reports in W → convert to mW)
        let max_token = format!("{}POW:LIM:HIGH", self.sour());
        if let Ok(v) = self.query(&max_token) {
            let w: f64 = v.parse().unwrap_or(0.1);
            self.max_power_mw = w * 1000.0;
            self.props.entry_mut("MaxPower_mW")
                .map(|e| e.value = PropertyValue::Float(self.max_power_mw));
        }
        let min_token = format!("{}POW:LIM:LOW", self.sour());
        if let Ok(v) = self.query(&min_token) {
            let w: f64 = v.parse().unwrap_or(0.0);
            self.min_power_mw = w * 1000.0;
            self.props.entry_mut("MinPower_mW")
                .map(|e| e.value = PropertyValue::Float(self.min_power_mw));
        }
        self.props.set_property_limits("PowerSetpoint_mW", self.min_power_mw, self.max_power_mw)?;

        // Read-only identification
        let sn_token = format!("{}INF:SNUM", self.syst());
        if let Ok(sn) = self.query(&sn_token) {
            self.props.entry_mut("SerialNumber").map(|e| e.value = PropertyValue::String(sn));
        }
        let hh_token = format!("{}DIOD:HOUR", self.syst());
        if let Ok(hh) = self.query(&hh_token) {
            self.props.entry_mut("HeadUsageHours").map(|e| e.value = PropertyValue::String(hh));
        }
        let wav_token = format!("{}INF:WAV", self.syst());
        if let Ok(wav) = self.query(&wav_token) {
            let nm: f64 = wav.parse().unwrap_or(0.0);
            self.props.entry_mut("Wavelength_nm").map(|e| e.value = PropertyValue::Float(nm));
        }

        // Current state
        let state_token = format!("{}AM:STATE", self.sour());
        if let Ok(state) = self.query(&state_token) {
            self.is_open = state.trim().eq_ignore_ascii_case("on");
        }
        let pow_token = format!("{}POW:LEV:IMM:AMPL", self.sour());
        if let Ok(pw) = self.query(&pow_token) {
            let w: f64 = pw.parse().unwrap_or(0.0);
            self.power_setpoint_mw = w * 1000.0;
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let token = format!("{}AM:STATE", self.sour());
            let _ = self.set_cmd(&token, "Off");
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
                    let token = format!("{}POW:LEV:IMM:AMPL", self.sour());
                    self.set_cmd(&token, &format!("{:.6}", mw / 1000.0))?;
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

impl Shutter for CoherentObis {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let val = if open { "On" } else { "Off" };
        let token = format!("{}AM:STATE", self.sour());
        self.set_cmd(&token, val)?;
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
            // COMM:HAND On, COMM:PROM Off, ERR:CLE
            .any("OK").any("OK").any("OK")
            // POW:LIM:HIGH → 0.1 W = 100 mW
            .any("0.1")
            // POW:LIM:LOW → 0.001 W = 1 mW
            .any("0.001")
            // INF:SNUM, DIOD:HOUR, INF:WAV
            .any("SN-OBIS-001").any("200.5").any("488")
            // AM:STATE → On, POW:LEV:IMM:AMPL → 0.05 W = 50 mW
            .any("Off").any("0.05")
    }

    #[test]
    fn initialize() {
        let mut dev = CoherentObis::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(dev.power_setpoint_mw, 50.0);
        assert_eq!(dev.max_power_mw, 100.0);
    }

    #[test]
    fn open_close() {
        let t = make_transport().any("OK").any("OK");
        let mut dev = CoherentObis::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_power() {
        let t = make_transport().any("OK");
        let mut dev = CoherentObis::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("PowerSetpoint_mW", PropertyValue::Float(75.0)).unwrap();
        assert_eq!(dev.power_setpoint_mw, 75.0);
    }

    #[test]
    fn no_transport_error() {
        let mut dev = CoherentObis::new();
        assert!(dev.initialize().is_err());
    }
}
