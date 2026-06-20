/// Vortran Stradus single-wavelength diode laser controller.
///
/// Protocol (`\r` send, `\r\n` receive):
///   Query:  `?<key>\r`  → `?<KEY>=<value>`
///   Set:    `<key>=<value>\r`
///
///   `?le`        → `?LE=0` or `?LE=1`   (laser emission)
///   `le=1`       → enable emission
///   `le=0`       → disable emission
///   `?lps`       → `?LPS=<mW>`          (power setpoint)
///   `lp=<mW>`    → set power setpoint
///   `?li`        → `?LI=<serial>`       (laser ID)
///   `?fv`        → `?FV=<version>`      (firmware version)
///   `?lh`        → `?LH=<hours>`        (usage hours)
///   `?fc`        → `?FC=<code>`         (fault code, 0=ok)
///   `?il`        → `?IL=1` OK / `?IL=0` OPEN (interlock)
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};

pub struct VortranStradus {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    is_open: Cell<bool>,
    power_setpoint_mw: Cell<f64>,
}

impl VortranStradus {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("PowerSetpoint_mW", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("PowerSetting", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("LaserEmission", PropertyValue::String("OFF".into()), false)
            .unwrap();
        props
            .set_allowed_values("LaserEmission", &["OFF", "ON"])
            .unwrap();
        props
            .define_property("LaserID", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property(
                "FirmwareVersion",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("UsageHours", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Hours", PropertyValue::String("0.00".into()), true)
            .unwrap();
        props
            .define_property("FaultCode", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("BaseplateTemp", PropertyValue::String("0.00".into()), true)
            .unwrap();
        props
            .define_property("Current", PropertyValue::String("0.00".into()), true)
            .unwrap();
        props
            .define_property("Interlock", PropertyValue::String("Unknown".into()), true)
            .unwrap();
        props
            .define_property(
                "OperatingCondition",
                PropertyValue::String("No Fault".into()),
                true,
            )
            .unwrap();
        props
            .define_property("Power", PropertyValue::String("0.00".into()), true)
            .unwrap();
        props
            .define_property("SerialCommand", PropertyValue::String(String::new()), false)
            .unwrap();
        props
            .define_property("SerialResponse", PropertyValue::String(String::new()), true)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: Cell::new(false),
            power_setpoint_mw: Cell::new(0.0),
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

    fn cmd(&self, command: &str) -> MmResult<String> {
        let cmd = command.to_string();
        self.call_transport(|t| {
            let r = t.send_recv(&cmd)?;
            Ok(r.trim().to_string())
        })
    }

    /// Parse `?KEY=value` → value string.
    fn parse_val(resp: &str) -> &str {
        if let Some(pos) = resp.find('=') {
            &resp[pos + 1..]
        } else {
            resp
        }
    }

    fn query_value(&self, command: &str) -> MmResult<String> {
        Ok(Self::parse_val(&self.cmd(command)?).to_string())
    }

    fn set_power_setpoint(&mut self, mw: f64) {
        self.power_setpoint_mw.set(mw);
        self.props
            .entry_mut("PowerSetpoint_mW")
            .map(|e| e.value = PropertyValue::Float(mw));
        self.props
            .entry_mut("PowerSetting")
            .map(|e| e.value = PropertyValue::Float(mw));
    }

    fn set_laser_emission(&mut self, open: bool) {
        self.is_open.set(open);
        self.props
            .entry_mut("LaserEmission")
            .map(|e| e.value = PropertyValue::String(if open { "ON" } else { "OFF" }.into()));
    }
}

impl Default for VortranStradus {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for VortranStradus {
    fn name(&self) -> &str {
        "VLTStradus"
    }
    fn description(&self) -> &str {
        "VORTRAN Stradus Laser"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        if let Ok(r) = self.cmd("?li") {
            self.props
                .entry_mut("LaserID")
                .map(|e| e.value = PropertyValue::String(Self::parse_val(&r).to_string()));
        }
        if let Ok(r) = self.cmd("?fv") {
            self.props
                .entry_mut("FirmwareVersion")
                .map(|e| e.value = PropertyValue::String(Self::parse_val(&r).to_string()));
        }
        if let Ok(r) = self.cmd("?lh") {
            let value = Self::parse_val(&r).to_string();
            self.props
                .entry_mut("UsageHours")
                .map(|e| e.value = PropertyValue::String(value.clone()));
            self.props
                .entry_mut("Hours")
                .map(|e| e.value = PropertyValue::String(value));
        }
        if let Ok(r) = self.cmd("?fc") {
            let code: i64 = Self::parse_val(&r).parse().unwrap_or(0);
            self.props
                .entry_mut("FaultCode")
                .map(|e| e.value = PropertyValue::Integer(code));
        }
        if let Ok(r) = self.cmd("?il") {
            let s = if Self::parse_val(&r) == "1" {
                "OK"
            } else {
                "INTERLOCK OPEN!"
            };
            self.props
                .entry_mut("Interlock")
                .map(|e| e.value = PropertyValue::String(s.into()));
        }
        if let Ok(r) = self.cmd("?le") {
            self.set_laser_emission(Self::parse_val(&r) == "1");
        }
        if let Ok(r) = self.cmd("?lps") {
            self.set_power_setpoint(Self::parse_val(&r).parse().unwrap_or(0.0));
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.cmd("le=0");
            self.is_open.set(false);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if self.initialized {
            match name {
                "PowerSetpoint_mW" => {
                    let value = Self::parse_val(&self.cmd("?lps")?)
                        .parse()
                        .map_err(|_| MmError::SerialInvalidResponse)?;
                    self.power_setpoint_mw.set(value);
                    return Ok(PropertyValue::Float(value));
                }
                "PowerSetting" => {
                    let value = Self::parse_val(&self.cmd("?lps")?)
                        .parse()
                        .map_err(|_| MmError::SerialInvalidResponse)?;
                    self.power_setpoint_mw.set(value);
                    return Ok(PropertyValue::Float(value));
                }
                "LaserEmission" => {
                    let open = Self::parse_val(&self.cmd("?le")?) == "1";
                    self.is_open.set(open);
                    return Ok(PropertyValue::String(
                        if open { "ON" } else { "OFF" }.into(),
                    ));
                }
                "LaserID" => return Ok(PropertyValue::String(self.query_value("?li")?)),
                "FirmwareVersion" => return Ok(PropertyValue::String(self.query_value("?fv")?)),
                "UsageHours" | "Hours" => {
                    return Ok(PropertyValue::String(self.query_value("?lh")?))
                }
                "FaultCode" => {
                    let code = Self::parse_val(&self.cmd("?fc")?)
                        .parse()
                        .map_err(|_| MmError::SerialInvalidResponse)?;
                    return Ok(PropertyValue::Integer(code));
                }
                "BaseplateTemp" => {
                    return Ok(PropertyValue::String(self.query_value("?bt")?));
                }
                "Current" => {
                    return Ok(PropertyValue::String(self.query_value("?lc")?));
                }
                "Interlock" => {
                    let state = if self.query_value("?il")? == "1" {
                        "OK"
                    } else {
                        "INTERLOCK OPEN!"
                    };
                    return Ok(PropertyValue::String(state.into()));
                }
                "OperatingCondition" => {
                    return Ok(PropertyValue::String(self.query_value("?fd")?));
                }
                "Power" => {
                    return Ok(PropertyValue::String(self.query_value("?lp")?));
                }
                _ => {}
            }
        }
        if name == "PowerSetpoint_mW" || name == "PowerSetting" {
            return Ok(PropertyValue::Float(self.power_setpoint_mw.get()));
        }
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "PowerSetpoint_mW" || name == "PowerSetting" {
            let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            if self.initialized {
                self.cmd(&format!("lp={:.4}", mw))?;
            }
            self.set_power_setpoint(mw);
            return Ok(());
        }
        if name == "LaserEmission" {
            let value = val.as_str().to_string();
            let open = match value.as_str() {
                "ON" => true,
                "OFF" => false,
                _ => return Err(MmError::InvalidPropertyValue),
            };
            if self.initialized {
                self.cmd(if open { "le=1" } else { "le=0" })?;
            }
            self.set_laser_emission(open);
            return Ok(());
        }
        if name == "SerialCommand" {
            let command = val.as_str().to_string();
            let response = if self.initialized {
                self.cmd(&command)?
            } else {
                String::new()
            };
            self.props
                .set("SerialCommand", PropertyValue::String(command))?;
            self.props
                .entry_mut("SerialResponse")
                .map(|e| e.value = PropertyValue::String(response));
            return Ok(());
        }
        self.props.set(name, val)
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

impl Shutter for VortranStradus {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.cmd(if open { "le=1" } else { "le=0" })?;
        self.set_laser_emission(open);
        Ok(())
    }
    fn get_open(&self) -> MmResult<bool> {
        if self.initialized {
            let open = Self::parse_val(&self.cmd("?le")?) == "1";
            self.is_open.set(open);
            Ok(open)
        } else {
            Ok(self.is_open.get())
        }
    }
    fn fire(&mut self, _dt: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("?li", "?LI=STRADUS-473-50")
            .expect("?fv", "?FV=v2.1")
            .expect("?lh", "?LH=100.5")
            .expect("?fc", "?FC=0")
            .expect("?il", "?IL=1")
            .expect("?le", "?LE=0")
            .expect("?lps", "?LPS=30.0")
    }

    #[test]
    fn initialize() {
        let t = make_transport().expect("?le", "?LE=0");
        let mut dev = VortranStradus::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(dev.power_setpoint_mw.get(), 30.0);
    }

    #[test]
    fn open_close() {
        let t = make_transport()
            .expect("le=1", "?LE=1")
            .expect("?le", "?LE=1")
            .expect("le=0", "?LE=0")
            .expect("?le", "?LE=0");
        let mut dev = VortranStradus::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_power() {
        let t = make_transport().any("OK");
        let mut dev = VortranStradus::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("PowerSetpoint_mW", PropertyValue::Float(45.0))
            .unwrap();
        assert_eq!(dev.power_setpoint_mw.get(), 45.0);
    }

    #[test]
    fn readonly_properties_are_live_reads() {
        let t = make_transport()
            .expect("?lps", "?LPS=45.5")
            .expect("?fc", "?FC=7")
            .expect("?il", "?IL=0");
        let mut dev = VortranStradus::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert_eq!(
            dev.get_property("PowerSetpoint_mW").unwrap(),
            PropertyValue::Float(45.5)
        );
        assert_eq!(
            dev.get_property("FaultCode").unwrap(),
            PropertyValue::Integer(7)
        );
        assert_eq!(
            dev.get_property("Interlock").unwrap(),
            PropertyValue::String("INTERLOCK OPEN!".into())
        );
    }

    #[test]
    fn serial_command_action_stores_response() {
        let t = make_transport().expect("?fc", "?FC=3");
        let mut dev = VortranStradus::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("SerialCommand", PropertyValue::String("?fc".into()))
            .unwrap();
        assert_eq!(
            dev.get_property("SerialResponse").unwrap(),
            PropertyValue::String("?FC=3".into())
        );
    }

    #[test]
    fn no_transport_error() {
        assert!(VortranStradus::new().initialize().is_err());
    }

    #[test]
    fn upstream_identity_and_property_aliases() {
        let t = make_transport()
            .expect("?lps", "?LPS=42.0")
            .expect("lp=55.0000", "?LPS=55.0")
            .expect("?le", "?LE=1")
            .expect("le=0", "?LE=0")
            .expect("?lh", "?LH=101.5")
            .expect("?lp", "?LP=54.9")
            .expect("?lc", "?LC=120.1")
            .expect("?bt", "?BT=31.0")
            .expect("?fd", "?FD=No Fault");
        let mut dev = VortranStradus::new().with_transport(Box::new(t));
        assert_eq!(dev.name(), "VLTStradus");
        assert_eq!(dev.description(), "VORTRAN Stradus Laser");

        dev.initialize().unwrap();
        assert_eq!(
            dev.get_property("PowerSetting").unwrap(),
            PropertyValue::Float(42.0)
        );
        dev.set_property("PowerSetting", PropertyValue::Float(55.0))
            .unwrap();
        assert_eq!(
            dev.get_property("LaserEmission").unwrap(),
            PropertyValue::String("ON".into())
        );
        dev.set_property("LaserEmission", PropertyValue::String("OFF".into()))
            .unwrap();
        assert_eq!(
            dev.get_property("Hours").unwrap(),
            PropertyValue::String("101.5".into())
        );
        assert_eq!(
            dev.get_property("Power").unwrap(),
            PropertyValue::String("54.9".into())
        );
        assert_eq!(
            dev.get_property("Current").unwrap(),
            PropertyValue::String("120.1".into())
        );
        assert_eq!(
            dev.get_property("BaseplateTemp").unwrap(),
            PropertyValue::String("31.0".into())
        );
        assert_eq!(
            dev.get_property("OperatingCondition").unwrap(),
            PropertyValue::String("No Fault".into())
        );
    }
}
