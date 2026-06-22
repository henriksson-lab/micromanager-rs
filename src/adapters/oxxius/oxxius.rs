/// Oxxius LaserBoxx (LBX/LCX/LMX) laser controller.
///
/// Protocol (LBX model focus):
///   `inf?\n`     -> model string e.g. "LBX-473-100-CSB"
///   `hid?\n`     -> serial number
///   `?sv\n`      -> software version
///   `?sta\n`     -> status integer (2=standby, 3=emission_on, 4=alarm)
///   `dl 1\n`     -> emission on
///   `dl 0\n`     -> emission off
///   `p <mW>\n`   -> set power setpoint
///   `?p\n`       -> power readback (mW)
///   `?hh\n`      -> usage hours
///   `?f\n`       -> fault code (0 = none)
///   `?int\n`     -> interlock (0=open/unsafe, 1=closed/safe)
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::thread;
use std::time::Duration;

const DEVICE_NAME: &str = "Oxxius LaserBoxx LBX or LMX or LCX";
const DEVICE_DESCRIPTION: &str = "Oxxius LaserBoxx Controller";

pub struct OxxiusLaser {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    power_setpoint_mw: f64,
    max_power_mw: f64,
}

impl OxxiusLaser {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("PowerSetpoint_mW", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("PowerReadback_mW", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("Model", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("SerialNumber", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property(
                "SoftwareVersion",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("UsageHours", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("FaultCode", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("Interlock", PropertyValue::String("Unknown".into()), true)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            power_setpoint_mw: 0.0,
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

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let cmd = format!("{}\n", command);
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }

    /// Parse nominal power from model string e.g. "LBX-473-100-CSB" → 100 mW.
    fn parse_max_power(model: &str) -> f64 {
        // Format: TYPE-WAVELENGTH-POWER-VARIANT
        model
            .split('-')
            .nth(2)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(100.0)
    }
}

impl Default for OxxiusLaser {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for OxxiusLaser {
    fn name(&self) -> &str {
        DEVICE_NAME
    }
    fn description(&self) -> &str {
        DEVICE_DESCRIPTION
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        let model = self.cmd("inf?")?;
        self.max_power_mw = Self::parse_max_power(&model);
        self.props
            .entry_mut("Model")
            .map(|e| e.value = PropertyValue::String(model));

        if let Ok(sn) = self.cmd("hid?") {
            self.props
                .entry_mut("SerialNumber")
                .map(|e| e.value = PropertyValue::String(sn));
        }
        if let Ok(sv) = self.cmd("?sv") {
            self.props
                .entry_mut("SoftwareVersion")
                .map(|e| e.value = PropertyValue::String(sv));
        }

        self.props
            .set_property_limits("PowerSetpoint_mW", 0.0, self.max_power_mw)?;

        if let Ok(hh) = self.cmd("?hh") {
            self.props
                .entry_mut("UsageHours")
                .map(|e| e.value = PropertyValue::String(hh));
        }
        if let Ok(f) = self.cmd("?f") {
            let code: i64 = f.parse().unwrap_or(0);
            self.props
                .entry_mut("FaultCode")
                .map(|e| e.value = PropertyValue::Integer(code));
        }
        if let Ok(i) = self.cmd("?int") {
            let s = if i.trim() == "1" { "Closed" } else { "Open" };
            self.props
                .entry_mut("Interlock")
                .map(|e| e.value = PropertyValue::String(s.into()));
        }

        if let Ok(sta) = self.cmd("?sta") {
            self.is_open = sta.trim() == "3";
        }
        if let Ok(p) = self.cmd("?p") {
            let readback = p.parse().unwrap_or(0.0);
            self.props
                .entry_mut("PowerReadback_mW")
                .map(|e| e.value = PropertyValue::Float(readback));
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
            "PowerSetpoint_mW" => Ok(PropertyValue::Float(self.power_setpoint_mw)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "PowerSetpoint_mW" => {
                let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if mw < 0.0 || mw > self.max_power_mw {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized {
                    self.cmd(&format!("p {:.4}", mw))?;
                }
                self.power_setpoint_mw = mw;
                self.props
                    .entry_mut("PowerSetpoint_mW")
                    .map(|e| e.value = PropertyValue::Float(mw));
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

impl Shutter for OxxiusLaser {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open { "dl 1" } else { "dl 0" };
        self.cmd(cmd)?;
        self.is_open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.is_open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        self.set_open(true)?;
        if _delta_t > 0.0 {
            thread::sleep(Duration::from_millis(_delta_t as u64));
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
            .expect("inf?\n", "LBX-473-100-CSB")
            .expect("hid?\n", "OXX-001")
            .expect("?sv\n", "v2.3")
            .expect("?hh\n", "500.0")
            .expect("?f\n", "0")
            .expect("?int\n", "1")
            .expect("?sta\n", "2")
            .expect("?p\n", "50.0")
    }

    #[test]
    fn initialize() {
        let mut dev = OxxiusLaser::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(dev.power_setpoint_mw, 0.0);
        assert_eq!(
            dev.get_property("PowerReadback_mW").unwrap(),
            PropertyValue::Float(50.0)
        );
        assert_eq!(dev.max_power_mw, 100.0);
    }

    #[test]
    fn open_close() {
        let t = make_transport()
            .expect("dl 1\n", "OK")
            .expect("dl 0\n", "OK");
        let mut dev = OxxiusLaser::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_power() {
        let t = make_transport().any("OK");
        let mut dev = OxxiusLaser::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("PowerSetpoint_mW", PropertyValue::Float(75.0))
            .unwrap();
        assert_eq!(dev.power_setpoint_mw, 75.0);
    }

    #[test]
    fn set_power_rejects_out_of_range() {
        let mut dev = OxxiusLaser::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert_eq!(
            dev.set_property("PowerSetpoint_mW", PropertyValue::Float(125.0))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(dev.power_setpoint_mw, 0.0);
    }

    #[test]
    fn parse_max_power_from_model() {
        assert_eq!(OxxiusLaser::parse_max_power("LBX-473-100-CSB"), 100.0);
        assert_eq!(OxxiusLaser::parse_max_power("LBX-638-200-CPP"), 200.0);
    }

    #[test]
    fn no_transport_error() {
        assert!(OxxiusLaser::new().initialize().is_err());
    }

    #[test]
    fn upstream_identity() {
        let dev = OxxiusLaser::new();
        assert_eq!(dev.name(), "Oxxius LaserBoxx LBX or LMX or LCX");
        assert_eq!(dev.description(), "Oxxius LaserBoxx Controller");
    }

    #[test]
    fn fire_opens_then_closes() {
        let t = make_transport()
            .expect("dl 1\n", "OK")
            .expect("dl 0\n", "OK");
        let mut dev = OxxiusLaser::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.fire(0.0).unwrap();
        assert!(!dev.get_open().unwrap());
    }
}
