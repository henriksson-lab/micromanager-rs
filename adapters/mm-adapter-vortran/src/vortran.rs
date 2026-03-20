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
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Shutter};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

pub struct VortranStradus {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    power_setpoint_mw: f64,
}

impl VortranStradus {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("PowerSetpoint_mW", PropertyValue::Float(0.0), false).unwrap();
        props.define_property("LaserID", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("FirmwareVersion", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("UsageHours", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("FaultCode", PropertyValue::Integer(0), true).unwrap();
        props.define_property("Interlock", PropertyValue::String("Unknown".into()), true).unwrap();

        Self { props, transport: None, initialized: false, is_open: false, power_setpoint_mw: 0.0 }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where F: FnOnce(&mut dyn Transport) -> MmResult<R> {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let cmd = command.to_string();
        self.call_transport(|t| { let r = t.send_recv(&cmd)?; Ok(r.trim().to_string()) })
    }

    /// Parse `?KEY=value` → value string.
    fn parse_val(resp: &str) -> &str {
        if let Some(pos) = resp.find('=') { &resp[pos + 1..] } else { resp }
    }
}

impl Default for VortranStradus { fn default() -> Self { Self::new() } }

impl Device for VortranStradus {
    fn name(&self) -> &str { "VortranStradus" }
    fn description(&self) -> &str { "Vortran Stradus diode laser" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }

        if let Ok(r) = self.cmd("?li") {
            self.props.entry_mut("LaserID").map(|e| e.value = PropertyValue::String(Self::parse_val(&r).to_string()));
        }
        if let Ok(r) = self.cmd("?fv") {
            self.props.entry_mut("FirmwareVersion").map(|e| e.value = PropertyValue::String(Self::parse_val(&r).to_string()));
        }
        if let Ok(r) = self.cmd("?lh") {
            self.props.entry_mut("UsageHours").map(|e| e.value = PropertyValue::String(Self::parse_val(&r).to_string()));
        }
        if let Ok(r) = self.cmd("?fc") {
            let code: i64 = Self::parse_val(&r).parse().unwrap_or(0);
            self.props.entry_mut("FaultCode").map(|e| e.value = PropertyValue::Integer(code));
        }
        if let Ok(r) = self.cmd("?il") {
            let s = if Self::parse_val(&r) == "1" { "Closed" } else { "Open" };
            self.props.entry_mut("Interlock").map(|e| e.value = PropertyValue::String(s.into()));
        }
        if let Ok(r) = self.cmd("?le") {
            self.is_open = Self::parse_val(&r) == "1";
        }
        if let Ok(r) = self.cmd("?lps") {
            self.power_setpoint_mw = Self::parse_val(&r).parse().unwrap_or(0.0);
            self.props.entry_mut("PowerSetpoint_mW")
                .map(|e| e.value = PropertyValue::Float(self.power_setpoint_mw));
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized { let _ = self.cmd("le=0"); self.is_open = false; self.initialized = false; }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if name == "PowerSetpoint_mW" { return Ok(PropertyValue::Float(self.power_setpoint_mw)); }
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "PowerSetpoint_mW" {
            let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            if self.initialized { self.cmd(&format!("lp={:.4}", mw))?; }
            self.power_setpoint_mw = mw;
            self.props.entry_mut("PowerSetpoint_mW").map(|e| e.value = PropertyValue::Float(mw));
            return Ok(());
        }
        self.props.set(name, val)
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Shutter }
    fn busy(&self) -> bool { false }
}

impl Shutter for VortranStradus {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.cmd(if open { "le=1" } else { "le=0" })?;
        self.is_open = open;
        Ok(())
    }
    fn get_open(&self) -> MmResult<bool> { Ok(self.is_open) }
    fn fire(&mut self, _dt: f64) -> MmResult<()> { self.set_open(true) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

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
        let mut dev = VortranStradus::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(dev.power_setpoint_mw, 30.0);
    }

    #[test]
    fn open_close() {
        let t = make_transport().expect("le=1", "?LE=1").expect("le=0", "?LE=0");
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
        dev.set_property("PowerSetpoint_mW", PropertyValue::Float(45.0)).unwrap();
        assert_eq!(dev.power_setpoint_mw, 45.0);
    }

    #[test]
    fn no_transport_error() { assert!(VortranStradus::new().initialize().is_err()); }
}
