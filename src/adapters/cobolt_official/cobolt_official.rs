use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};

/// Cobolt Official laser controller.
///
/// Implements the `Shutter` trait: open = emission on (`l1r`), closed = emission off (`l0r`).
/// This is the official Cobolt adapter that works with all Cobolt laser series.
pub struct CoboltOfficialLaser {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    is_open: Cell<bool>,
    power_setpoint_mw: f64,
    model: String,
    subtype: String,
    state_property_name: String,
    cdrh_mode: bool,
    shutter_supported: bool,
}

impl CoboltOfficialLaser {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("None".into()), false)
            .unwrap();
        props
            .define_property("SerialNumber", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Serial Number", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property(
                "FirmwareVersion",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Firmware Version",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("Model", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Name", PropertyValue::String("Unknown".into()), true)
            .unwrap();
        props
            .define_property("Wavelength", PropertyValue::String("Unknown".into()), true)
            .unwrap();
        props
            .define_property("UsageHours", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property(
                "Operating Hours",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("PowerSetpoint_mW", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("PowerSetpoint_mW", 0.0, 1000.0)
            .unwrap();
        props
            .define_property("PowerReadback_mW", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("LaserState", PropertyValue::String("Off".into()), true)
            .unwrap();
        props
            .define_property(
                "Emission Status",
                PropertyValue::String("closed".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("Emission Status", &["closed", "open"])
            .unwrap();
        props
            .define_property(
                "Run Mode",
                PropertyValue::String("Constant Power".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values(
                "Run Mode",
                &["Constant Current", "Constant Power", "Modulation"],
            )
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: Cell::new(false),
            power_setpoint_mw: 0.0,
            model: String::new(),
            subtype: "Unknown".into(),
            state_property_name: "LaserState".into(),
            cdrh_mode: false,
            shutter_supported: true,
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
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }

    fn subtype_for(firmware_version: &str, model: &str) -> (&'static str, &'static str) {
        if model.contains("06-51-")
            || model.contains("06-53-")
            || model.contains("06-57-")
            || model.contains("06-91-")
            || model.contains("06-93-")
            || model.contains("06-97-")
        {
            ("06-DPL", "Dpl06Laser State")
        } else if model.contains("06-01-") || model.contains("06-03-") {
            ("06-MLD", "Mld06Laser State")
        } else if firmware_version.contains("9.001") {
            ("Skyra", "LaserState")
        } else {
            ("Unknown", "LaserState")
        }
    }

    fn wavelength_from_model(model: &str) -> String {
        model
            .split('-')
            .find(|token| token.chars().all(|c| c.is_ascii_digit()) && !token.is_empty())
            .unwrap_or("Unknown")
            .to_string()
    }

    fn state_allows_shutter(&self) -> MmResult<bool> {
        if !self.cdrh_mode {
            return Ok(true);
        }
        let state = self.cmd("gom?")?;
        Ok(match (self.subtype.as_str(), state.as_str()) {
            ("06-DPL", "4") => true,
            ("06-MLD", "2") | ("06-MLD", "3") | ("06-MLD", "4") => true,
            _ => false,
        })
    }

    fn state_label(&self) -> MmResult<String> {
        if !self.cdrh_mode {
            return Ok(match self.cmd("l?")?.as_str() {
                "0" => "Off".into(),
                "1" => "On".into(),
                _ => return Err(MmError::SerialInvalidResponse),
            });
        }
        let state = self.cmd("gom?")?;
        let label = match self.subtype.as_str() {
            "06-DPL" => match state.as_str() {
                "0" => "Off",
                "1" => "Waiting for TEC",
                "2" => "Waiting for Key",
                "3" => "Warming Up",
                "4" => "Completed",
                "5" => "Fault",
                "6" => "Aborted",
                "7" => "Modulation",
                _ => return Err(MmError::SerialInvalidResponse),
            },
            "06-MLD" => match state.as_str() {
                "0" => "Off",
                "1" => "Waiting for Key",
                "2" => "Completed",
                "3" => "Completed (On/Off Modulation)",
                "4" => "Completed (Modulation)",
                "5" => "Fault",
                "6" => "Aborted",
                _ => return Err(MmError::SerialInvalidResponse),
            },
            _ => return Err(MmError::SerialInvalidResponse),
        };
        Ok(label.into())
    }
}

impl Default for CoboltOfficialLaser {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CoboltOfficialLaser {
    fn name(&self) -> &str {
        "Cobolt Laser"
    }

    fn description(&self) -> &str {
        "Official device adapter for Cobolt lasers."
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        if self
            .props
            .get("Port")
            .map(|port| port.as_str() == "None")
            .unwrap_or(true)
        {
            return Err(MmError::InvalidPropertyValue);
        }

        let firmware_version = self.cmd("gfv?")?;
        let model = self.cmd("glm?")?;
        let (subtype, state_property_name) = Self::subtype_for(&firmware_version, &model);
        self.model = model.clone();
        self.subtype = subtype.into();
        self.state_property_name = state_property_name.into();
        self.props
            .entry_mut("FirmwareVersion")
            .map(|e| e.value = PropertyValue::String(firmware_version.clone()));
        self.props
            .entry_mut("Firmware Version")
            .map(|e| e.value = PropertyValue::String(firmware_version));
        self.props
            .entry_mut("Model")
            .map(|e| e.value = PropertyValue::String(model.clone()));
        self.props
            .entry_mut("Name")
            .map(|e| e.value = PropertyValue::String(self.subtype.clone()));
        self.props
            .entry_mut("Wavelength")
            .map(|e| e.value = PropertyValue::String(Self::wavelength_from_model(&model)));

        self.cdrh_mode = self.cmd("gas?").map(|v| v == "1").unwrap_or(false);
        self.shutter_supported = self.cmd("l0r").map(|v| v.contains("OK")).unwrap_or(false);

        if !self.props.has_property(&self.state_property_name) {
            let _ = self.props.define_property(
                self.state_property_name.clone(),
                PropertyValue::String("Off".into()),
                true,
            );
        }

        if let Ok(sn) = self.cmd("gsn?") {
            self.props
                .entry_mut("SerialNumber")
                .map(|e| e.value = PropertyValue::String(sn.clone()));
            self.props
                .entry_mut("Serial Number")
                .map(|e| e.value = PropertyValue::String(sn));
        }

        // Query usage hours
        if let Ok(hrs) = self.cmd("hrs?") {
            let h = hrs.parse::<f64>().unwrap_or(0.0);
            self.props
                .entry_mut("UsageHours")
                .map(|e| e.value = PropertyValue::Float(h));
            self.props
                .entry_mut("Operating Hours")
                .map(|e| e.value = PropertyValue::String(hrs));
        }

        if let Ok(label) = self.state_label() {
            self.props
                .entry_mut("LaserState")
                .map(|e| e.value = PropertyValue::String(label.clone()));
            self.props
                .entry_mut(&self.state_property_name)
                .map(|e| e.value = PropertyValue::String(label));
        }

        // Query power setpoint
        if let Ok(sp) = self.cmd("glp?") {
            if let Ok(mw) = sp.parse::<f64>() {
                self.power_setpoint_mw = mw;
                self.props
                    .entry_mut("PowerSetpoint_mW")
                    .map(|e| e.value = PropertyValue::Float(mw));
            }
        }

        // Query power readback
        if let Ok(p) = self.cmd("pa?") {
            if let Ok(mw) = p.parse::<f64>() {
                self.props
                    .entry_mut("PowerReadback_mW")
                    .map(|e| e.value = PropertyValue::Float(mw));
            }
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
            "PowerSetpoint_mW" => Ok(PropertyValue::Float(self.power_setpoint_mw)),
            "PowerReadback_mW" if self.initialized => Ok(PropertyValue::Float(
                self.cmd("pa?")?
                    .parse::<f64>()
                    .map_err(|_| MmError::SerialInvalidResponse)?,
            )),
            "LaserState" if self.initialized => Ok(PropertyValue::String(self.state_label()?)),
            name if name == self.state_property_name && self.initialized => {
                Ok(PropertyValue::String(self.state_label()?))
            }
            "Run Mode" if self.initialized => {
                let label = match self.cmd("gam?")?.as_str() {
                    "0" => "Constant Current",
                    "1" => "Constant Power",
                    "2" => "Modulation",
                    _ => return Err(MmError::SerialInvalidResponse),
                };
                Ok(PropertyValue::String(label.into()))
            }
            "Emission Status" => Ok(PropertyValue::String(if self.is_open.get() {
                "open".into()
            } else {
                "closed".into()
            })),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "PowerSetpoint_mW" => {
                let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    let resp = self.cmd(&format!("slp {:.4}", mw))?;
                    if resp != "OK" {
                        return Err(MmError::SerialInvalidResponse);
                    }
                }
                self.power_setpoint_mw = mw;
                self.props
                    .entry_mut("PowerSetpoint_mW")
                    .map(|e| e.value = PropertyValue::Float(mw));
                Ok(())
            }
            "Emission Status" => match val.as_str() {
                "open" => self.set_open(true),
                "closed" => self.set_open(false),
                _ => Err(MmError::InvalidPropertyValue),
            },
            "Run Mode" => {
                let command = match val.as_str() {
                    "Constant Current" => "ecc",
                    "Constant Power" => "ecp",
                    "Modulation" => "em",
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                if self.initialized {
                    let resp = self.cmd(command)?;
                    if resp != "OK" {
                        return Err(MmError::SerialInvalidResponse);
                    }
                }
                self.props.set(name, val)
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

impl Shutter for CoboltOfficialLaser {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        if !self.shutter_supported || !self.state_allows_shutter()? {
            return Err(MmError::LocallyDefined("laser startup incomplete".into()));
        }
        let cmd = if open { "l1r" } else { "l0r" };
        let resp = self.cmd(cmd)?;
        if resp != "OK" {
            return Err(MmError::SerialInvalidResponse);
        }
        self.is_open.set(open);
        let label = if open { "On" } else { "Off" };
        self.props
            .entry_mut("LaserState")
            .map(|e| e.value = PropertyValue::String(label.into()));
        self.props.entry_mut("Emission Status").map(|e| {
            e.value = PropertyValue::String(if open { "open".into() } else { "closed".into() })
        });
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.state_allows_shutter()? && self.is_open.get())
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
            .expect("gfv?", "2.0.1")
            .expect("glm?", "06-01-488")
            .expect("gas?", "0")
            .expect("l0r", "OK")
            .expect("gsn?", "ABC-12345")
            .expect("hrs?", "100.0")
            .expect("l?", "0")
            .expect("glp?", "50.0")
            .expect("pa?", "0.0")
    }

    fn make_laser(t: MockTransport) -> CoboltOfficialLaser {
        let mut laser = CoboltOfficialLaser::new().with_transport(Box::new(t));
        laser
            .set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        laser
    }

    #[test]
    fn initialize_reads_fields() {
        let mut laser = make_laser(make_transport());
        laser.initialize().unwrap();
        assert!(!laser.get_open().unwrap());
        assert_eq!(laser.power_setpoint_mw, 50.0);
        assert_eq!(
            laser.get_property("SerialNumber").unwrap(),
            PropertyValue::String("ABC-12345".into())
        );
        assert_eq!(
            laser.get_property("Name").unwrap(),
            PropertyValue::String("06-MLD".into())
        );
        assert_eq!(
            laser.get_property("Model").unwrap(),
            PropertyValue::String("06-01-488".into())
        );
    }

    #[test]
    fn open_close_laser() {
        let t = make_transport().expect("l1r", "OK").expect("l0r", "OK");
        let mut laser = make_laser(t);
        laser.initialize().unwrap();
        laser.set_open(true).unwrap();
        assert!(laser.get_open().unwrap());
        laser.set_open(false).unwrap();
        assert!(!laser.get_open().unwrap());
    }

    #[test]
    fn laser_state_is_read_only_readback() {
        let mut laser = make_laser(make_transport().expect("l?", "0"));
        laser.initialize().unwrap();

        assert!(laser.is_property_read_only("LaserState"));
        laser
            .set_property("LaserState", PropertyValue::String("On".into()))
            .unwrap();

        assert!(!laser.get_open().unwrap());
        assert_eq!(
            laser.get_property("LaserState").unwrap(),
            PropertyValue::String("Off".into())
        );
    }

    #[test]
    fn set_power_setpoint() {
        let t = make_transport().expect("slp 75.0000", "OK");
        let mut laser = make_laser(t);
        laser.initialize().unwrap();
        laser
            .set_property("PowerSetpoint_mW", PropertyValue::Float(75.0))
            .unwrap();
        assert_eq!(laser.power_setpoint_mw, 75.0);
    }

    #[test]
    fn no_transport_error() {
        let mut laser = CoboltOfficialLaser::new();
        assert!(laser.initialize().is_err());
    }

    #[test]
    fn initialize_requires_selected_port_like_upstream() {
        let mut laser = CoboltOfficialLaser::new().with_transport(Box::new(MockTransport::new()));
        assert_eq!(
            laser.initialize().unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn fire_closes_after_pulse() {
        let t = make_transport().expect("l1r", "OK").expect("l0r", "OK");
        let mut laser = make_laser(t);
        laser.initialize().unwrap();
        laser.fire(0.0).unwrap();
        assert!(!laser.get_open().unwrap());
    }

    #[test]
    fn cdrh_state_gates_shutter_open() {
        let t = MockTransport::new()
            .expect("gfv?", "2.0.1")
            .expect("glm?", "06-01-488")
            .expect("gas?", "1")
            .expect("l0r", "OK")
            .expect("gsn?", "ABC-12345")
            .expect("hrs?", "100.0")
            .expect("gom?", "1")
            .expect("glp?", "50.0")
            .expect("pa?", "0.0")
            .expect("gom?", "1");

        let mut laser = make_laser(t);
        laser.initialize().unwrap();
        assert!(laser.set_open(true).is_err());
    }

    #[test]
    fn cdrh_completed_state_allows_shutter_open() {
        let t = MockTransport::new()
            .expect("gfv?", "2.0.1")
            .expect("glm?", "06-01-488")
            .expect("gas?", "1")
            .expect("l0r", "OK")
            .expect("gsn?", "ABC-12345")
            .expect("hrs?", "100.0")
            .expect("gom?", "2")
            .expect("glp?", "50.0")
            .expect("pa?", "0.0")
            .expect("gom?", "2")
            .expect("l1r", "OK");

        let mut laser = make_laser(t);
        laser.initialize().unwrap();
        laser.set_open(true).unwrap();
        assert_eq!(
            laser.get_property("Emission Status").unwrap(),
            PropertyValue::String("open".into())
        );
    }

    #[test]
    fn shutdown_only_clears_initialized_like_upstream() {
        let mut laser = make_laser(make_transport());
        laser.initialize().unwrap();
        laser.is_open.set(true);
        laser.shutdown().unwrap();
        assert!(laser.get_open().unwrap());
    }

    #[test]
    fn initialized_port_change_is_rejected_and_preserved() {
        let mut laser = CoboltOfficialLaser::new().with_transport(Box::new(make_transport()));
        laser
            .set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        laser.initialize().unwrap();

        assert_eq!(
            laser
                .set_property("Port", PropertyValue::String("COM2".into()))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            laser.get_property("Port").unwrap(),
            PropertyValue::String("COM1".into())
        );
    }

    #[test]
    fn run_mode_readback_is_live() {
        let mut laser = make_laser(make_transport().expect("gam?", "2"));
        laser.initialize().unwrap();

        assert_eq!(
            laser.get_property("Run Mode").unwrap(),
            PropertyValue::String("Modulation".into())
        );
    }
}
