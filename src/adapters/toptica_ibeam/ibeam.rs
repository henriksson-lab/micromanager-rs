use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

/// Toptica iBeam Smart CW laser controller.
///
/// Implements the `Shutter` trait: open = laser on (`la on`), closed = laser off (`la off`).
///
/// The iBeam Smart uses a multi-line protocol where each command returns multiple lines
/// terminated by `[OK]`. The adapter simplifies this by using `send_recv` which gets the
/// first response line (the mock transport supplies the relevant line directly).
pub struct IBeamSmartCW {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    power_mw: f64,
    max_power_mw: f64,
    fine_on: bool,
    ext_on: bool,
    fine_a_pct: f64,
    fine_b_pct: f64,
}

impl IBeamSmartCW {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Serial ID", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property(
                "Firmware version",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Maximum power (mW)",
                PropertyValue::String("125".into()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Clipping status",
                PropertyValue::String("Undefined".into()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Laser Operation",
                PropertyValue::String("Off".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("Laser Operation", &["Off", "On"])
            .unwrap();
        props
            .define_property("Power (mW)", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property(
                "Enable ext trigger",
                PropertyValue::String("Off".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("Enable ext trigger", &["Off", "On"])
            .unwrap();
        props
            .define_property("Enable Fine", PropertyValue::String("Off".into()), false)
            .unwrap();
        props
            .set_allowed_values("Enable Fine", &["Off", "On"])
            .unwrap();
        props
            .define_property("Fine A (%)", PropertyValue::Float(0.0), false)
            .unwrap();
        props.set_property_limits("Fine A (%)", 0.0, 100.0).unwrap();
        props
            .define_property("Fine B (%)", PropertyValue::Float(10.0), false)
            .unwrap();
        props.set_property_limits("Fine B (%)", 0.0, 100.0).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            power_mw: 0.0,
            max_power_mw: 125.0,
            fine_on: false,
            ext_on: false,
            fine_a_pct: 0.0,
            fine_b_pct: 10.0,
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

    /// Send a command and return the trimmed response.
    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let cmd = command.to_string();
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            let resp = resp.trim().trim_start_matches("CMD>").trim().to_string();
            if resp.starts_with("%SYS") && !resp.contains("%SYS-I") {
                return Err(MmError::LocallyDefined(format!(
                    "iBeamSmart error response: {}",
                    resp
                )));
            }
            Ok(resp)
        })
    }

    fn parse_on_off(resp: &str) -> Option<bool> {
        if resp.contains("ON") {
            Some(true)
        } else if resp.contains("OFF") {
            Some(false)
        } else {
            None
        }
    }

    fn parse_fine_percentage(resp: &str) -> Option<f64> {
        let value_start = resp.find("-> ")? + 3;
        let value_end = resp[value_start..]
            .find(" %")
            .map(|pos| value_start + pos)
            .unwrap_or(resp.len());
        resp[value_start..value_end].trim().parse().ok()
    }

    fn parse_power_mw(resp: &str) -> Option<f64> {
        let pwr_pos = resp.find("PWR:")?;
        resp[pwr_pos + 4..]
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
    }

    fn set_on_off_property(&mut self, name: &str, on: bool) {
        let label = if on { "On" } else { "Off" };
        self.props
            .entry_mut(name)
            .map(|e| e.value = PropertyValue::String(label.into()));
    }

    /// Refresh one live property through the serial command used by upstream
    /// `BeforeGet` action handlers.
    pub fn refresh_live_property(&mut self, name: &str) -> MmResult<PropertyValue> {
        if !self.initialized {
            return self.props.get(name).cloned();
        }

        match name {
            "Laser Operation" => {
                let resp = self.cmd("sta la")?;
                self.is_open = Self::parse_on_off(&resp).ok_or(MmError::SerialInvalidResponse)?;
                self.set_on_off_property(name, self.is_open);
                Ok(PropertyValue::String(
                    if self.is_open { "On" } else { "Off" }.into(),
                ))
            }
            "Power (mW)" => {
                let resp = self.cmd("sh level pow")?;
                self.power_mw =
                    Self::parse_power_mw(&resp).ok_or(MmError::SerialInvalidResponse)?;
                self.props
                    .entry_mut(name)
                    .map(|e| e.value = PropertyValue::Float(self.power_mw));
                Ok(PropertyValue::Float(self.power_mw))
            }
            "Enable ext trigger" => {
                let resp = self.cmd("sta ext")?;
                self.ext_on = Self::parse_on_off(&resp).ok_or(MmError::SerialInvalidResponse)?;
                self.set_on_off_property(name, self.ext_on);
                Ok(PropertyValue::String(
                    if self.ext_on { "On" } else { "Off" }.into(),
                ))
            }
            "Enable Fine" => {
                let resp = self.cmd("sta fine")?;
                self.fine_on = Self::parse_on_off(&resp).ok_or(MmError::SerialInvalidResponse)?;
                self.set_on_off_property(name, self.fine_on);
                Ok(PropertyValue::String(
                    if self.fine_on { "On" } else { "Off" }.into(),
                ))
            }
            "Fine A (%)" | "Fine B (%)" => {
                let fine = if name == "Fine A (%)" { 'a' } else { 'b' };
                let resp = self.cmd("sh data")?;
                let needle = format!("fine {}", fine);
                if !resp.contains(&needle) {
                    return Err(MmError::SerialInvalidResponse);
                }
                let pct =
                    Self::parse_fine_percentage(&resp).ok_or(MmError::SerialInvalidResponse)?;
                if fine == 'a' {
                    self.fine_a_pct = pct;
                } else {
                    self.fine_b_pct = pct;
                }
                self.props
                    .entry_mut(name)
                    .map(|e| e.value = PropertyValue::Float(pct));
                Ok(PropertyValue::Float(pct))
            }
            "Clipping status" => {
                let clip = self.cmd("sta clip")?;
                self.props
                    .entry_mut(name)
                    .map(|e| e.value = PropertyValue::String(clip.clone()));
                Ok(PropertyValue::String(clip))
            }
            _ => self.props.get(name).cloned(),
        }
    }
}

impl Default for IBeamSmartCW {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for IBeamSmartCW {
    fn name(&self) -> &str {
        "iBeamSmartCW"
    }

    fn description(&self) -> &str {
        "Toptica iBeam smart laser in CW mode."
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Disable prompt so we get clean responses
        self.cmd("prom off")?;

        // Get serial number (response contains "iBEAM-xxxx")
        if let Ok(serial) = self.cmd("id") {
            self.props
                .entry_mut("Serial ID")
                .map(|e| e.value = PropertyValue::String(serial));
        }

        // Get firmware version (response contains "iB..." version string)
        if let Ok(ver) = self.cmd("ver") {
            self.props
                .entry_mut("Firmware version")
                .map(|e| e.value = PropertyValue::String(ver));
        }

        // Get clip status
        if let Ok(clip) = self.cmd("sta clip") {
            self.props
                .entry_mut("Clipping status")
                .map(|e| e.value = PropertyValue::String(clip));
        }

        // Get laser on/off status
        if let Ok(la) = self.cmd("sta la") {
            self.is_open = Self::parse_on_off(&la).unwrap_or(false);
            let label = if self.is_open { "On" } else { "Off" };
            self.props
                .entry_mut("Laser Operation")
                .map(|e| e.value = PropertyValue::String(label.into()));
        }

        // Get power level (response: "CH2, PWR: <f> mW")
        if let Ok(pow_resp) = self.cmd("sh level pow") {
            if let Some(mw) = Self::parse_power_mw(&pow_resp) {
                self.power_mw = mw;
                self.props
                    .entry_mut("Power (mW)")
                    .map(|e| e.value = PropertyValue::Float(mw));
            }
        }

        if let Ok(ext) = self.cmd("sta ext") {
            self.ext_on = Self::parse_on_off(&ext).unwrap_or(false);
            let label = if self.ext_on { "On" } else { "Off" };
            self.props
                .entry_mut("Enable ext trigger")
                .map(|e| e.value = PropertyValue::String(label.into()));
        }

        if let Ok(fine) = self.cmd("sta fine") {
            self.fine_on = Self::parse_on_off(&fine).unwrap_or(false);
            let label = if self.fine_on { "On" } else { "Off" };
            self.props
                .entry_mut("Enable Fine")
                .map(|e| e.value = PropertyValue::String(label.into()));
        }

        if let Ok(fine_a) = self.cmd("sh data") {
            if fine_a.contains("fine a") {
                if let Some(pct) = Self::parse_fine_percentage(&fine_a) {
                    self.fine_a_pct = pct;
                    self.props
                        .entry_mut("Fine A (%)")
                        .map(|e| e.value = PropertyValue::Float(pct));
                }
            }
        }

        if let Ok(fine_b) = self.cmd("sh data") {
            if fine_b.contains("fine b") {
                if let Some(pct) = Self::parse_fine_percentage(&fine_b) {
                    self.fine_b_pct = pct;
                    self.props
                        .entry_mut("Fine B (%)")
                        .map(|e| e.value = PropertyValue::Float(pct));
                }
            }
        }

        self.props
            .set_property_limits("Power (mW)", 0.0, self.max_power_mw)
            .ok();

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.cmd("la off");
            let _ = self.cmd("prom on");
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Power (mW)" => Ok(PropertyValue::Float(self.power_mw)),
            "Enable ext trigger" => Ok(PropertyValue::String(
                if self.ext_on { "On" } else { "Off" }.into(),
            )),
            "Enable Fine" => Ok(PropertyValue::String(
                if self.fine_on { "On" } else { "Off" }.into(),
            )),
            "Fine A (%)" => Ok(PropertyValue::Float(self.fine_a_pct)),
            "Fine B (%)" => Ok(PropertyValue::Float(self.fine_b_pct)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "Power (mW)" => {
                let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0.0..=self.max_power_mw).contains(&mw) {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized {
                    self.cmd(&format!("set pow {}", mw))?;
                }
                self.power_mw = mw;
                self.props
                    .entry_mut("Power (mW)")
                    .map(|e| e.value = PropertyValue::Float(mw));
                Ok(())
            }
            "Laser Operation" => {
                let s = match &val {
                    PropertyValue::String(s) => s.clone(),
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                let open = s == "On";
                if self.initialized {
                    let cmd = if open { "la on" } else { "la off" };
                    self.cmd(cmd)?;
                    self.is_open = open;
                }
                self.props.set(name, PropertyValue::String(s))
            }
            "Enable ext trigger" => {
                let s = val.as_str();
                if s != "On" && s != "Off" {
                    return Err(MmError::InvalidPropertyValue);
                }
                let on = s == "On";
                if self.initialized {
                    self.cmd(if on { "en x" } else { "di x" })?;
                }
                self.ext_on = on;
                self.props.set(name, PropertyValue::String(s.into()))
            }
            "Enable Fine" => {
                let s = val.as_str();
                if s != "On" && s != "Off" {
                    return Err(MmError::InvalidPropertyValue);
                }
                let on = s == "On";
                if self.initialized {
                    self.cmd(if on { "fine on" } else { "fine off" })?;
                }
                self.fine_on = on;
                self.props.set(name, PropertyValue::String(s.into()))
            }
            "Fine A (%)" | "Fine B (%)" => {
                let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0.0..=100.0).contains(&pct) {
                    return Err(MmError::InvalidPropertyValue);
                }
                let fine = if name == "Fine A (%)" { 'a' } else { 'b' };
                if self.initialized {
                    self.cmd(&format!("fine {} {}", fine, pct))?;
                }
                if fine == 'a' {
                    self.fine_a_pct = pct;
                } else {
                    self.fine_b_pct = pct;
                }
                self.props.set(name, PropertyValue::Float(pct))
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
        DeviceType::Generic
    }

    fn busy(&self) -> bool {
        false
    }
}

impl Generic for IBeamSmartCW {}

impl Shutter for IBeamSmartCW {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open { "la on" } else { "la off" };
        self.cmd(cmd)?;
        self.is_open = open;
        let label = if open { "On" } else { "Off" };
        self.props
            .entry_mut("Laser Operation")
            .map(|e| e.value = PropertyValue::String(label.into()));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.is_open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("prom off", "[OK]")
            .expect("id", "iBEAM-1234")
            .expect("ver", "iB-V2.0.1 [OK]")
            .expect("sta clip", "PASS")
            .expect("sta la", "OFF")
            .expect("sh level pow", "CH2, PWR: 0.0 mW")
            .expect("sta ext", "OFF")
            .expect("sta fine", "ON")
            .expect("sh data", "fine a -> 12.5 %")
            .expect("sh data", "fine b -> 33.5 %")
    }

    #[test]
    fn initialize_reads_fields() {
        let mut dev = IBeamSmartCW::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(
            dev.get_property("Serial ID").unwrap(),
            PropertyValue::String("iBEAM-1234".into())
        );
        assert_eq!(
            dev.get_property("Clipping status").unwrap(),
            PropertyValue::String("PASS".into())
        );
        assert_eq!(
            dev.get_property("Enable Fine").unwrap(),
            PropertyValue::String("On".into())
        );
        assert_eq!(
            dev.get_property("Fine A (%)").unwrap(),
            PropertyValue::Float(12.5)
        );
        assert_eq!(
            dev.get_property("Fine B (%)").unwrap(),
            PropertyValue::Float(33.5)
        );
    }

    #[test]
    fn open_close_laser() {
        let t = make_transport()
            .expect("la on", "[OK]")
            .expect("la off", "[OK]");
        let mut dev = IBeamSmartCW::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_power() {
        let t = make_transport().expect("set pow 50", "[OK]");
        let mut dev = IBeamSmartCW::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Power (mW)", PropertyValue::Float(50.0))
            .unwrap();
        assert_eq!(dev.power_mw, 50.0);
    }

    #[test]
    fn fine_and_ext_properties_send_upstream_commands() {
        let t = make_transport()
            .expect("fine on", "[OK]")
            .expect("fine a 25", "[OK]")
            .expect("en x", "[OK]");
        let mut dev = IBeamSmartCW::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Enable Fine", PropertyValue::String("On".into()))
            .unwrap();
        dev.set_property("Fine A (%)", PropertyValue::Float(25.0))
            .unwrap();
        dev.set_property("Enable ext trigger", PropertyValue::String("On".into()))
            .unwrap();
        assert_eq!(
            dev.get_property("Enable Fine").unwrap(),
            PropertyValue::String("On".into())
        );
        assert_eq!(
            dev.get_property("Enable ext trigger").unwrap(),
            PropertyValue::String("On".into())
        );
    }

    #[test]
    fn sys_error_response_is_propagated() {
        let t = make_transport().expect("la on", "%SYS-E-001 bad");
        let mut dev = IBeamSmartCW::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert!(dev.set_open(true).is_err());
    }

    #[test]
    fn fire_is_unsupported_like_upstream_generic_device() {
        let mut dev = IBeamSmartCW::new();
        assert_eq!(dev.fire(1.0).unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn no_transport_error() {
        let mut dev = IBeamSmartCW::new();
        assert!(dev.initialize().is_err());
    }

    #[test]
    fn description_matches_upstream_registration() {
        let dev = IBeamSmartCW::new();
        assert_eq!(dev.description(), "Toptica iBeam smart laser in CW mode.");
    }

    #[test]
    fn initialized_port_change_is_rejected_like_upstream() {
        let mut dev = IBeamSmartCW::new().with_transport(Box::new(make_transport()));
        dev.set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        dev.initialize().unwrap();

        assert_eq!(
            dev.set_property("Port", PropertyValue::String("COM2".into())),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            dev.get_property("Port").unwrap(),
            PropertyValue::String("COM1".into())
        );
    }

    #[test]
    fn refresh_live_property_mirrors_upstream_before_get_handlers() {
        let t = make_transport()
            .expect("sta la", "ON")
            .expect("sh level pow", "CH2, PWR: 42.5 mW")
            .expect("sta ext", "ON")
            .expect("sta fine", "OFF")
            .expect("sh data", "fine a -> 22.25 %")
            .expect("sh data", "fine b -> 77.75 %")
            .expect("sta clip", "GOOD");
        let mut dev = IBeamSmartCW::new().with_transport(Box::new(t));
        dev.initialize().unwrap();

        assert_eq!(
            dev.refresh_live_property("Laser Operation").unwrap(),
            PropertyValue::String("On".into())
        );
        assert!(dev.get_open().unwrap());
        assert_eq!(
            dev.refresh_live_property("Power (mW)").unwrap(),
            PropertyValue::Float(42.5)
        );
        assert_eq!(
            dev.refresh_live_property("Enable ext trigger").unwrap(),
            PropertyValue::String("On".into())
        );
        assert_eq!(
            dev.refresh_live_property("Enable Fine").unwrap(),
            PropertyValue::String("Off".into())
        );
        assert_eq!(
            dev.refresh_live_property("Fine A (%)").unwrap(),
            PropertyValue::Float(22.25)
        );
        assert_eq!(
            dev.refresh_live_property("Fine B (%)").unwrap(),
            PropertyValue::Float(77.75)
        );
        assert_eq!(
            dev.refresh_live_property("Clipping status").unwrap(),
            PropertyValue::String("GOOD".into())
        );
    }

    #[test]
    fn refresh_live_property_rejects_unparseable_live_response() {
        let t = make_transport().expect("sh level pow", "CH2, PWR: bad mW");
        let mut dev = IBeamSmartCW::new().with_transport(Box::new(t));
        dev.initialize().unwrap();

        assert_eq!(
            dev.refresh_live_property("Power (mW)").unwrap_err(),
            MmError::SerialInvalidResponse
        );
        assert_eq!(
            dev.get_property("Power (mW)").unwrap(),
            PropertyValue::Float(0.0)
        );
    }
}
