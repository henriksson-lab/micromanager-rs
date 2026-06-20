/// Coherent OBIS laser controller.
///
/// Uses SCPI-style commands:
///   Query:  `TOKEN?\r`  → plain value response
///   Set:    `TOKEN value\r` → plain acknowledgement response
///
/// Power is reported by the device in Watts; this adapter stores/exposes mW.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::thread;
use std::time::Duration;

pub struct CoherentObis {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    device_index: i64,
    power_setpoint_mw: f64,
    min_power_mw: f64,
    max_power_mw: f64,
}

impl CoherentObis {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("DeviceIndex", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .set_allowed_values("DeviceIndex", &["0", "1"])
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("State", &["0", "1"]).unwrap();
        props
            .define_property("PowerSetpoint", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("PowerReadback", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("HeadID", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Head Usage Hours", PropertyValue::Float(0.0), true)
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
            device_index: 1,
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

    fn set_power_setpoint(&mut self, requested_mw: f64) -> MmResult<f64> {
        let power_setpoint = self.source_token("POW:LEV:IMM:AMPL");
        self.set_cmd(&power_setpoint, &format!("{:.6}", requested_mw / 1000.0))?;
        let achieved_w = self.query(&power_setpoint)?.parse::<f64>().unwrap_or(0.0);
        let achieved_mw = achieved_w * 1000.0;

        if requested_mw != 0.0 {
            let fraction_error = ((achieved_mw - requested_mw) / requested_mw).abs();
            if fraction_error > 0.05 && fraction_error < 0.10 {
                return Ok(achieved_mw);
            }
        }

        Ok(requested_mw)
    }

    fn system_token(&self, suffix: &str) -> String {
        format!("SYST{}:{}", self.device_index, suffix)
    }

    fn source_token(&self, suffix: &str) -> String {
        format!("SOUR{}:{}", self.device_index, suffix)
    }
}

impl Default for CoherentObis {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CoherentObis {
    fn name(&self) -> &str {
        "CoherentObis"
    }
    fn description(&self) -> &str {
        "CoherentObis Laser"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Handshake and setup
        let handshaking = self.system_token("COMM:HAND");
        let prompt = self.system_token("COMM:PROM");
        let _ = self.set_cmd(&handshaking, "On");
        let _ = self.set_cmd(&prompt, "Off");
        let clear_error = self.system_token("ERR:CLE");
        let _ = self.call_transport(|t| t.send(&clear_error));

        // Power limits (device reports in W → convert to mW)
        if let Ok(v) = self.query(&self.source_token("POW:LIM:HIGH")) {
            let w: f64 = v.parse().unwrap_or(0.1);
            self.max_power_mw = w * 1000.0;
            self.props
                .entry_mut("Maximum Laser Power")
                .map(|e| e.value = PropertyValue::Float(self.max_power_mw));
        }
        if let Ok(v) = self.query(&self.source_token("POW:LIM:LOW")) {
            let w: f64 = v.parse().unwrap_or(0.0);
            self.min_power_mw = w * 1000.0;
            self.props
                .entry_mut("Minimum Laser Power")
                .map(|e| e.value = PropertyValue::Float(self.min_power_mw));
        }
        self.props
            .set_property_limits("PowerSetpoint", self.min_power_mw, self.max_power_mw)?;

        // Read-only identification
        if let Ok(sn) = self.query(&self.system_token("INF:SNUM")) {
            self.props
                .entry_mut("HeadID")
                .map(|e| e.value = PropertyValue::String(sn));
        }
        if let Ok(hh) = self.query(&self.system_token("DIOD:HOUR")) {
            let hours: f64 = hh.parse().unwrap_or(0.0);
            self.props
                .entry_mut("Head Usage Hours")
                .map(|e| e.value = PropertyValue::Float(hours));
        }
        if let Ok(wav) = self.query(&self.system_token("INF:WAV")) {
            let nm: f64 = wav.parse().unwrap_or(0.0);
            self.props
                .entry_mut("Wavelength")
                .map(|e| e.value = PropertyValue::Float(nm));
        }

        // Current state
        if let Ok(state) = self.query(&self.source_token("AM:STATE")) {
            self.is_open = state.trim().eq_ignore_ascii_case("on");
            self.props
                .entry_mut("State")
                .map(|e| e.value = PropertyValue::Integer(if self.is_open { 1 } else { 0 }));
        }
        if let Ok(pw) = self.query(&self.source_token("POW:LEV:IMM:AMPL")) {
            let w: f64 = pw.parse().unwrap_or(0.0);
            self.power_setpoint_mw = w * 1000.0;
            self.props
                .entry_mut("PowerSetpoint")
                .map(|e| e.value = PropertyValue::Float(self.power_setpoint_mw));
        }

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
            "DeviceIndex" => {
                let index = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if index != 0 && index != 1 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.device_index = index;
                self.props.set(name, PropertyValue::Integer(index))
            }
            "State" => {
                let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                match state {
                    0 | 1 => self.set_open(state == 1),
                    _ => Err(MmError::InvalidPropertyValue),
                }
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

impl Shutter for CoherentObis {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let val = if open { "On" } else { "Off" };
        let state = self.source_token("AM:STATE");
        self.set_cmd(&state, val)?;
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
        if delta_t > 0.0 {
            thread::sleep(Duration::from_millis(delta_t as u64));
        }
        self.set_open(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            // COMM:HAND On, COMM:PROM Off, ERR:CLE (send-only)
            .expect("SYST1:COMM:HAND On", "OK")
            .expect("SYST1:COMM:PROM Off", "OK")
            // POW:LIM:HIGH → 0.1 W = 100 mW
            .expect("SOUR1:POW:LIM:HIGH?", "0.1")
            // POW:LIM:LOW → 0.001 W = 1 mW
            .expect("SOUR1:POW:LIM:LOW?", "0.001")
            // INF:SNUM, DIOD:HOUR, INF:WAV
            .expect("SYST1:INF:SNUM?", "SN-OBIS-001")
            .expect("SYST1:DIOD:HOUR?", "200.5")
            .expect("SYST1:INF:WAV?", "488")
            // AM:STATE → On, POW:LEV:IMM:AMPL → 0.05 W = 50 mW
            .expect("SOUR1:AM:STATE?", "Off")
            .expect("SOUR1:POW:LEV:IMM:AMPL?", "0.05")
    }

    #[test]
    fn initialize() {
        let mut dev = CoherentObis::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(dev.power_setpoint_mw, 50.0);
        assert_eq!(dev.max_power_mw, 100.0);
        assert_eq!(
            dev.get_property("Maximum Laser Power").unwrap(),
            PropertyValue::Float(100.0)
        );
        assert_eq!(
            dev.get_property("Minimum Laser Power").unwrap(),
            PropertyValue::Float(1.0)
        );
    }

    #[test]
    fn open_close() {
        let t = make_transport()
            .expect("SOUR1:AM:STATE On", "OK")
            .expect("SOUR1:AM:STATE Off", "OK");
        let mut dev = CoherentObis::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_power() {
        let t = make_transport()
            .expect("SOUR1:POW:LEV:IMM:AMPL 0.075000", "OK")
            .expect("SOUR1:POW:LEV:IMM:AMPL?", "0.075");
        let mut dev = CoherentObis::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("PowerSetpoint", PropertyValue::Float(75.0))
            .unwrap();
        assert_eq!(dev.power_setpoint_mw, 75.0);
    }

    #[test]
    fn set_power_keeps_achieved_value_when_echo_is_close_but_different() {
        let t = make_transport()
            .expect("SOUR1:POW:LEV:IMM:AMPL 0.075000", "OK")
            .expect("SOUR1:POW:LEV:IMM:AMPL?", "0.070");
        let mut dev = CoherentObis::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("PowerSetpoint", PropertyValue::Float(75.0))
            .unwrap();
        assert_eq!(dev.power_setpoint_mw, 70.0);
        assert_eq!(
            dev.get_property("PowerSetpoint").unwrap(),
            PropertyValue::Float(70.0)
        );
    }

    #[test]
    fn no_transport_error() {
        let mut dev = CoherentObis::new();
        assert!(dev.initialize().is_err());
    }

    #[test]
    fn fire_closes_after_pulse() {
        let t = make_transport()
            .expect("SOUR1:AM:STATE On", "OK")
            .expect("SOUR1:AM:STATE Off", "OK");
        let mut dev = CoherentObis::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.fire(0.0).unwrap();
        assert!(!dev.get_open().unwrap());
    }
}
