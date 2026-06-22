/// Coherent Sapphire laser controller.
///
/// Token-based protocol identical to CoherentCube but with different tokens:
///   Query: `?TOKEN\r`  → value or `TOKEN=value`
///   Set:   `TOKEN=value\r` → echoed response
///
/// Power defaults to 0.5-50 mW and wavelength defaults to 561 nm.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};
use std::time::Instant;

const MIN_POWER_MW: f64 = 0.5;
const MAX_POWER_MW: f64 = 50.0;
const WAVELENGTH_NM: f64 = 561.0;

pub struct Sapphire {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    is_open: Cell<bool>,
    power_setpoint_mw: f64,
    min_power_mw: f64,
    max_power_mw: f64,
    wavelength_nm: f64,
    delay_ms: f64,
    changed_time: Cell<Instant>,
}

impl Sapphire {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("State", &["0", "1"]).unwrap();
        props
            .define_property("PowerSetpoint", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("PowerSetpoint", MIN_POWER_MW, MAX_POWER_MW)
            .unwrap();
        props
            .define_property("PowerReadback", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property(
                "Minimum Laser Power",
                PropertyValue::Float(MIN_POWER_MW),
                false,
            )
            .unwrap();
        props
            .define_property(
                "Maximum Laser Power",
                PropertyValue::Float(MAX_POWER_MW),
                false,
            )
            .unwrap();
        props
            .define_property("Wavelength", PropertyValue::Float(WAVELENGTH_NM), false)
            .unwrap();
        props
            .define_property("Delay_ms", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("Delay_ms", 0.0, f64::MAX)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: Cell::new(false),
            power_setpoint_mw: 0.0,
            min_power_mw: MIN_POWER_MW,
            max_power_mw: MAX_POWER_MW,
            wavelength_nm: WAVELENGTH_NM,
            delay_ms: 0.0,
            changed_time: Cell::new(Instant::now()),
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

    /// Send `?TOKEN` and parse `TOKEN=value` or bare value.
    fn query(&self, token: &str) -> MmResult<String> {
        let cmd = format!("?{}", token);
        let tok = token.to_string();
        self.call_transport(|t| {
            t.purge()?;
            let resp = t.send_recv(&cmd)?;
            let mut resp = resp.trim().to_string();
            if resp.is_empty() {
                resp = t.receive_line()?.trim().to_string();
            }
            if let Some(eq) = resp.find('=') {
                let key = &resp[..eq];
                if key == tok {
                    return Ok(resp[eq + 1..].to_string());
                }
            }
            Ok(resp)
        })
    }

    /// Send `TOKEN=value` and parse the echoed `TOKEN=achieved` response.
    fn set_token(&self, token: &str, value: &str) -> MmResult<String> {
        let cmd = format!("{}={}", token, value);
        let tok = token.to_string();
        self.call_transport(|t| {
            t.purge()?;
            t.send(&cmd)?;
            let resp = t.receive_line()?;
            Self::parse_set_response(&tok, &resp)
        })
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
            .set_token("P", &format!("{:.5}", requested_mw))?
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

    /// Read and discard greeting lines until an empty line is encountered.
    fn read_greeting(&self) -> MmResult<()> {
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
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Sapphire {
    fn name(&self) -> &str {
        "Sapphire"
    }
    fn description(&self) -> &str {
        "Coherent Sapphire Laser"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        self.read_greeting()?;

        let _ = self.set_token("E", "0"); // disable echo
        let _ = self.set_token(">", "0"); // disable prompt
        let _ = self.set_token("T", "1"); // enable TEC servo

        if let Ok(l) = self.query("L") {
            self.is_open.set(l.trim() == "1");
            self.props
                .entry_mut("State")
                .map(|e| e.value = PropertyValue::Integer(if self.is_open.get() { 1 } else { 0 }));
        }
        if let Ok(p) = self.query("P") {
            self.power_setpoint_mw = p.parse().unwrap_or(0.0);
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
            "State" if self.initialized => match self.query("L")?.trim() {
                "0" => Ok(PropertyValue::Integer(0)),
                "1" => Ok(PropertyValue::Integer(1)),
                _ => Err(MmError::SerialInvalidResponse),
            },
            "State" => Ok(PropertyValue::Integer(if self.is_open.get() {
                1
            } else {
                0
            })),
            "PowerSetpoint" if self.initialized => Ok(PropertyValue::Float(
                self.query("P")?
                    .parse()
                    .map_err(|_| MmError::SerialInvalidResponse)?,
            )),
            "PowerSetpoint" => Ok(PropertyValue::Float(self.power_setpoint_mw)),
            "PowerReadback" if self.initialized => Ok(PropertyValue::Float(
                self.query("P")?
                    .parse()
                    .map_err(|_| MmError::SerialInvalidResponse)?,
            )),
            "Minimum Laser Power" => Ok(PropertyValue::Float(self.min_power_mw)),
            "Maximum Laser Power" => Ok(PropertyValue::Float(self.max_power_mw)),
            "Wavelength" => Ok(PropertyValue::Float(self.wavelength_nm)),
            "Delay_ms" => Ok(PropertyValue::Float(self.delay_ms)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "State" => {
                let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                match state {
                    0 | 1 => self.set_open(state == 1),
                    _ => Err(MmError::InvalidPropertyValue),
                }
            }
            "PowerSetpoint" => {
                let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if mw < self.min_power_mw || mw > self.max_power_mw {
                    return Err(MmError::InvalidPropertyValue);
                }
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
            "Minimum Laser Power" => {
                let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.min_power_mw = mw;
                self.props.set(name, PropertyValue::Float(mw))?;
                self.props.set_property_limits(
                    "PowerSetpoint",
                    self.min_power_mw,
                    self.max_power_mw,
                )
            }
            "Maximum Laser Power" => {
                let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.max_power_mw = mw;
                self.props.set(name, PropertyValue::Float(mw))?;
                self.props.set_property_limits(
                    "PowerSetpoint",
                    self.min_power_mw,
                    self.max_power_mw,
                )
            }
            "Wavelength" => {
                let nm = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.wavelength_nm = nm;
                self.props.set(name, PropertyValue::Float(nm))
            }
            "Delay_ms" => {
                let ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.delay_ms = ms;
                self.props.set(name, PropertyValue::Float(ms))
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
        self.changed_time.get().elapsed().as_secs_f64() * 1000.0 < self.delay_ms
    }
}

impl Shutter for Sapphire {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let val = if open { "1" } else { "0" };
        self.set_token("L", val)?;
        self.is_open.set(open);
        self.changed_time.set(Instant::now());
        self.props
            .entry_mut("State")
            .map(|e| e.value = PropertyValue::Integer(if open { 1 } else { 0 }));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        if self.initialized {
            match self.query("L")?.trim() {
                "0" => Ok(false),
                "1" => Ok(true),
                _ => Err(MmError::SerialInvalidResponse),
            }
        } else {
            Ok(self.is_open.get())
        }
    }

    fn fire(&mut self, delta_t: f64) -> MmResult<()> {
        self.set_open(true)?;
        std::thread::sleep(std::time::Duration::from_millis((delta_t + 0.5) as u64));
        self.set_open(false)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            // read_greeting: one banner line + empty
            .any("Sapphire 561-20 CDRH v1.01")
            .any("")
            // E=0, >=0, T=1
            .any("E=0")
            .any(">=0")
            .any("T=1")
            // ?L → 0, ?P → 10.0
            .any("L=0")
            .any("P=10.0")
    }

    #[test]
    fn initialize() {
        let mut dev = Sapphire::new().with_transport(Box::new(make_transport().any("L=0")));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(dev.power_setpoint_mw, 10.0);
        assert!(dev.has_property("State"));
        assert!(dev.has_property("PowerSetpoint"));
        assert!(dev.has_property("PowerReadback"));
        assert!(dev.has_property("Minimum Laser Power"));
        assert!(dev.has_property("Maximum Laser Power"));
        assert!(dev.has_property("Wavelength"));
        assert!(dev.has_property("Delay_ms"));
        assert_eq!(dev.description(), "Coherent Sapphire Laser");
        assert!(!dev.has_property("HeadID"));
        assert!(!dev.has_property("Head Usage Hours"));
        assert!(!dev.has_property("PowerSetpoint_mW"));
        assert!(!dev.has_property("PowerReadback_mW"));
        assert!(!dev.has_property("Wavelength_nm"));
    }

    #[test]
    fn open_close() {
        let t = make_transport()
            .any("L=1")
            .any("L=1")
            .any("L=1")
            .any("L=0")
            .any("L=0")
            .any("L=0");
        let mut dev = Sapphire::new().with_transport(Box::new(t));
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
        let t = make_transport().any("P=25.00000");
        let mut dev = Sapphire::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("PowerSetpoint", PropertyValue::Float(25.0))
            .unwrap();
        assert_eq!(dev.power_setpoint_mw, 25.0);
    }

    #[test]
    fn set_power_keeps_achieved_value_when_echo_is_close_but_different() {
        let t = make_transport().any("P=23.00000").any("P=23.00000");
        let mut dev = Sapphire::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("PowerSetpoint", PropertyValue::Float(25.0))
            .unwrap();

        assert_eq!(dev.power_setpoint_mw, 23.0);
        assert_eq!(
            dev.get_property("PowerSetpoint").unwrap(),
            PropertyValue::Float(23.0)
        );
    }

    #[test]
    fn set_token_rejects_wrong_echo_token() {
        let t = make_transport().any("Q=25.00000");
        let mut dev = Sapphire::new().with_transport(Box::new(t));
        dev.initialize().unwrap();

        assert_eq!(
            dev.set_property("PowerSetpoint", PropertyValue::Float(25.0))
                .unwrap_err(),
            MmError::SerialInvalidResponse
        );
        assert_eq!(dev.power_setpoint_mw, 10.0);
    }

    #[test]
    fn initialized_live_properties_query_controller() {
        let t = make_transport().any("L=1").any("P=12.5").any("P=12.25");
        let mut dev = Sapphire::new().with_transport(Box::new(t));
        dev.initialize().unwrap();

        assert_eq!(
            dev.get_property("State").unwrap(),
            PropertyValue::Integer(1)
        );
        assert_eq!(
            dev.get_property("PowerSetpoint").unwrap(),
            PropertyValue::Float(12.5)
        );
        assert_eq!(
            dev.get_property("PowerReadback").unwrap(),
            PropertyValue::Float(12.25)
        );
    }

    #[test]
    fn initialized_port_change_is_rejected_and_delay_drives_busy() {
        let t = make_transport().any("L=1");
        let mut dev = Sapphire::new().with_transport(Box::new(t));
        dev.initialize().unwrap();

        assert_eq!(
            dev.set_property("Port", PropertyValue::String("COM2".into())),
            Err(MmError::InvalidPropertyValue)
        );
        dev.set_property("Delay_ms", PropertyValue::Float(1000.0))
            .unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.busy());
    }

    #[test]
    fn configurable_power_limits_update_setpoint_limits() {
        let mut dev = Sapphire::new();
        dev.set_property("Minimum Laser Power", PropertyValue::Float(2.0))
            .unwrap();
        dev.set_property("Maximum Laser Power", PropertyValue::Float(20.0))
            .unwrap();

        assert!(dev
            .set_property("PowerSetpoint", PropertyValue::Float(1.0))
            .is_err());
        dev.set_property("PowerSetpoint", PropertyValue::Float(10.0))
            .unwrap();
    }

    #[test]
    fn no_transport_error() {
        let mut dev = Sapphire::new();
        assert!(dev.initialize().is_err());
    }
}
