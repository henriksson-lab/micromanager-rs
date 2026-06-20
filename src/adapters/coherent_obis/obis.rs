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
use std::cell::{Cell, RefCell};
use std::thread;
use std::time::{Duration, Instant};

pub struct CoherentObis {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: Cell<bool>,
    is_open: Cell<bool>,
    device_index: Cell<i64>,
    power_setpoint_mw: Cell<f64>,
    min_power_mw: Cell<f64>,
    max_power_mw: Cell<f64>,
    delay_ms: f64,
    changed_at: Cell<Option<Instant>>,
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
            .define_property("Delay_ms", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("Delay_ms", 0.0, f64::MAX)
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
            transport: RefCell::new(None),
            initialized: Cell::new(false),
            is_open: Cell::new(false),
            device_index: Cell::new(1),
            power_setpoint_mw: Cell::new(0.0),
            min_power_mw: Cell::new(0.0),
            max_power_mw: Cell::new(100.0),
            delay_ms: 0.0,
            changed_at: Cell::new(None),
        }
    }

    pub fn with_transport(self, t: Box<dyn Transport>) -> Self {
        *self.transport.borrow_mut() = Some(t);
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        let mut transport = self.transport.borrow_mut();
        match transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    /// Send `TOKEN?\r` and return the trimmed response.
    fn query(&self, token: &str) -> MmResult<String> {
        let cmd = format!("{}?", token);
        let resp = self.call_transport(|t| {
            t.purge()?;
            t.send_recv(&cmd)
        })?;
        Ok(resp.trim().to_string())
    }

    /// Send `TOKEN value\r` and discard the response.
    fn set_cmd(&self, token: &str, value: &str) -> MmResult<()> {
        let cmd = format!("{} {}", token, value);
        self.call_transport(|t| {
            t.purge()?;
            t.send(&cmd)?;
            t.receive_line()?;
            Ok(())
        })
    }

    fn set_power_setpoint(&self, requested_mw: f64) -> MmResult<f64> {
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

    fn read_power_setpoint_mw(&self) -> MmResult<f64> {
        let w = self
            .query(&self.source_token("POW:LEV:IMM:AMPL"))?
            .parse::<f64>()
            .unwrap_or(0.0);
        let mw = w * 1000.0;
        self.power_setpoint_mw.set(mw);
        Ok(mw)
    }

    fn read_power_readback_mw(&self) -> MmResult<f64> {
        let w = self
            .query(&self.source_token("POW:LEV:IMM:AMPL"))?
            .parse::<f64>()
            .unwrap_or(0.0);
        Ok(w * 1000.0)
    }

    fn read_state_value(&self) -> MmResult<i64> {
        let state = self.query(&self.source_token("AM:STATE"))?;
        let lower = state.to_ascii_lowercase();
        let value = if lower.starts_with("on") {
            1
        } else if lower.starts_with("off") {
            0
        } else {
            2
        };
        self.is_open.set(value == 1);
        Ok(value)
    }

    fn read_limit_w(&self, suffix: &str) -> MmResult<f64> {
        self
            .query(&self.source_token(suffix))?
            .parse::<f64>()
            .map_err(|_| MmError::SerialInvalidResponse)
    }

    fn system_token(&self, suffix: &str) -> String {
        format!("SYST{}:{}", self.device_index.get(), suffix)
    }

    fn source_token(&self, suffix: &str) -> String {
        format!("SOUR{}:{}", self.device_index.get(), suffix)
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
        if self.transport.borrow().is_none() {
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
            self.max_power_mw.set(w * 1000.0);
            self.props
                .entry_mut("Maximum Laser Power")
                .map(|e| e.value = PropertyValue::Float(self.max_power_mw.get()));
        }
        if let Ok(v) = self.query(&self.source_token("POW:LIM:LOW")) {
            let w: f64 = v.parse().unwrap_or(0.0);
            self.min_power_mw.set(w * 1000.0);
            self.props
                .entry_mut("Minimum Laser Power")
                .map(|e| e.value = PropertyValue::Float(self.min_power_mw.get()));
        }
        self.props.set_property_limits(
            "PowerSetpoint",
            self.min_power_mw.get(),
            self.max_power_mw.get(),
        )?;

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
            self.is_open.set(state.trim().eq_ignore_ascii_case("on"));
            self.props
                .entry_mut("State")
                .map(|e| e.value = PropertyValue::Integer(if self.is_open.get() { 1 } else { 0 }));
        }
        if let Ok(pw) = self.query(&self.source_token("POW:LEV:IMM:AMPL")) {
            let w: f64 = pw.parse().unwrap_or(0.0);
            self.power_setpoint_mw.set(w * 1000.0);
            self.props
                .entry_mut("PowerSetpoint")
                .map(|e| e.value = PropertyValue::Float(self.power_setpoint_mw.get()));
        }

        self.initialized.set(true);
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized.get() {
            self.initialized.set(false);
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "PowerSetpoint" if self.initialized.get() => {
                Ok(PropertyValue::Float(self.read_power_setpoint_mw()?))
            }
            "PowerSetpoint" => Ok(PropertyValue::Float(self.power_setpoint_mw.get())),
            "PowerReadback" if self.initialized.get() => {
                Ok(PropertyValue::Float(self.read_power_readback_mw()?))
            }
            "State" if self.initialized.get() => {
                Ok(PropertyValue::Integer(self.read_state_value()?))
            }
            "State" => Ok(PropertyValue::Integer(if self.is_open.get() {
                1
            } else {
                0
            })),
            "HeadID" if self.initialized.get() => Ok(PropertyValue::String(
                self.query(&self.system_token("INF:SNUM"))?,
            )),
            "Head Usage Hours" if self.initialized.get() => {
                let hours = self
                    .query(&self.system_token("DIOD:HOUR"))?
                    .parse::<f64>()
                    .unwrap_or(0.0);
                Ok(PropertyValue::Float(hours))
            }
            "Wavelength" if self.initialized.get() => {
                let nm = self
                    .query(&self.system_token("INF:WAV"))?
                    .parse::<f64>()
                    .unwrap_or(0.0);
                Ok(PropertyValue::Float(nm))
            }
            "Minimum Laser Power" if self.initialized.get() => {
                Ok(PropertyValue::Float(
                    self.read_limit_w("POW:LIM:LOW")? * 1000.0,
                ))
            }
            "Maximum Laser Power" if self.initialized.get() => {
                Ok(PropertyValue::Float(
                    self.read_limit_w("POW:LIM:HIGH")? * 1000.0,
                ))
            }
            "Delay_ms" => Ok(PropertyValue::Float(self.delay_ms)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "PowerSetpoint" => {
                let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                let stored_mw = if self.initialized.get() {
                    self.set_power_setpoint(mw)?
                } else {
                    mw
                };
                self.power_setpoint_mw.set(stored_mw);
                self.props
                    .entry_mut("PowerSetpoint")
                    .map(|e| e.value = PropertyValue::Float(stored_mw));
                Ok(())
            }
            "Delay_ms" => {
                let delay = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if delay < 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.delay_ms = delay;
                self.props.set(name, PropertyValue::Float(delay))
            }
            "DeviceIndex" if self.initialized.get() => Err(MmError::InvalidProperty),
            "DeviceIndex" => {
                let index = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if index != 0 && index != 1 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.device_index.set(index);
                self.props.set(name, PropertyValue::Integer(index))
            }
            "Port" if self.initialized.get() => Err(MmError::InvalidProperty),
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
        match self.changed_at.get() {
            Some(changed_at) => {
                changed_at.elapsed() < Duration::from_secs_f64(self.delay_ms / 1000.0)
            }
            None => false,
        }
    }
}

impl Shutter for CoherentObis {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let val = if open { "On" } else { "Off" };
        let state = self.source_token("AM:STATE");
        self.set_cmd(&state, val)?;
        self.is_open.set(open);
        self.changed_at.set(Some(Instant::now()));
        self.props
            .entry_mut("State")
            .map(|e| e.value = PropertyValue::Integer(if open { 1 } else { 0 }));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        if self.initialized.get() {
            match self.read_state_value()? {
                1 => Ok(true),
                0 => Ok(false),
                _ => Err(MmError::UnknownPosition),
            }
        } else {
            Ok(self.is_open.get())
        }
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
        let t = make_transport()
            .expect("SOUR1:POW:LIM:HIGH?", "0.1")
            .expect("SOUR1:POW:LIM:LOW?", "0.001");
        let mut dev = CoherentObis::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert!(!dev.is_open.get());
        assert_eq!(dev.power_setpoint_mw.get(), 50.0);
        assert_eq!(dev.max_power_mw.get(), 100.0);
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
            .expect("SOUR1:AM:STATE?", "On")
            .expect("SOUR1:AM:STATE Off", "OK")
            .expect("SOUR1:AM:STATE?", "Off");
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
        assert_eq!(dev.power_setpoint_mw.get(), 75.0);
    }

    #[test]
    fn set_power_keeps_achieved_value_when_echo_is_close_but_different() {
        let t = make_transport()
            .expect("SOUR1:POW:LEV:IMM:AMPL 0.075000", "OK")
            .expect("SOUR1:POW:LEV:IMM:AMPL?", "0.070")
            .expect("SOUR1:POW:LEV:IMM:AMPL?", "0.070");
        let mut dev = CoherentObis::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("PowerSetpoint", PropertyValue::Float(75.0))
            .unwrap();
        assert_eq!(dev.power_setpoint_mw.get(), 70.0);
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
        assert!(!dev.is_open.get());
    }

    #[test]
    fn get_property_refreshes_live_serial_values() {
        let t = make_transport()
            .expect("SOUR1:POW:LEV:IMM:AMPL?", "0.060")
            .expect("SOUR1:POW:LEV:IMM:AMPL?", "0.061")
            .expect("SOUR1:AM:STATE?", "On")
            .expect("SYST1:INF:SNUM?", "SN-LIVE");
        let mut dev = CoherentObis::new().with_transport(Box::new(t));
        dev.initialize().unwrap();

        assert_eq!(
            dev.get_property("PowerSetpoint").unwrap(),
            PropertyValue::Float(60.0)
        );
        assert_eq!(
            dev.get_property("PowerReadback").unwrap(),
            PropertyValue::Float(61.0)
        );
        assert_eq!(
            dev.get_property("State").unwrap(),
            PropertyValue::Integer(1)
        );
        assert_eq!(
            dev.get_property("HeadID").unwrap(),
            PropertyValue::String("SN-LIVE".into())
        );
    }

    #[test]
    fn get_open_refreshes_state_and_unrecognized_state_errors() {
        let t = make_transport()
            .expect("SOUR1:AM:STATE?", "Standby")
            .expect("SOUR1:AM:STATE?", "Standby");
        let mut dev = CoherentObis::new().with_transport(Box::new(t));
        dev.initialize().unwrap();

        assert_eq!(
            dev.get_property("State").unwrap(),
            PropertyValue::Integer(2)
        );
        assert_eq!(dev.get_open().unwrap_err(), MmError::UnknownPosition);
    }

    #[test]
    fn busy_tracks_delay_after_state_change() {
        let t = make_transport().expect("SOUR1:AM:STATE On", "OK");
        let mut dev = CoherentObis::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Delay_ms", PropertyValue::Float(50.0))
            .unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.busy());
        thread::sleep(Duration::from_millis(60));
        assert!(!dev.busy());
    }

    #[test]
    fn initialized_port_change_is_rejected() {
        let mut dev = CoherentObis::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert_eq!(
            dev.set_property("Port", PropertyValue::String("COM2".into()))
                .unwrap_err(),
            MmError::InvalidProperty
        );
    }

    #[test]
    fn initialized_device_index_change_is_rejected() {
        let mut dev = CoherentObis::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert_eq!(
            dev.set_property("DeviceIndex", PropertyValue::Integer(0))
                .unwrap_err(),
            MmError::InvalidProperty
        );
    }
}
