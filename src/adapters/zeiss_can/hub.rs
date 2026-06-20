/// Zeiss CAN-bus shared serial hub.
///
/// All Zeiss CAN devices share one serial port at 9600 baud, `\r` terminator.
///
/// Command routing by prefix:
///   HP* → microscope stand (reflectors, objectives, filters, shutters, Z)
///   NP* → MCU28 XY stage controller
///
/// Response prefixes:
///   PH  → stand response (HP commands)
///   PN  → MCU28 response (NP commands)
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Hub};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};
use std::sync::{Arc, Mutex};

pub const DEVICE_NAME_HUB: &str = "ZeissScope";
pub const DEVICE_NAME_SHUTTER: &str = "ZeissShutter";
pub const DEVICE_NAME_SHUTTER_MF: &str = "ZeissShutterMFFirmware";
pub const DEVICE_NAME_FOCUS: &str = "Focus";
pub const DEVICE_NAME_XY: &str = "ZeissXYStage";
pub const DEVICE_NAME_REFLECTOR: &str = "ZeissReflectorTurret";
pub const DEVICE_NAME_OBJECTIVES: &str = "ZeissObjectives";
pub const DEVICE_NAME_EXT_FILTER: &str = "ZeissExternalFilterWheel";
pub const DEVICE_NAME_OPTOVAR: &str = "ZeissOptovar";
pub const DEVICE_NAME_FILTER1: &str = "ZeissFilterWheel1";
pub const DEVICE_NAME_FILTER2: &str = "ZeissFilterWheel2";
pub const DEVICE_NAME_CONDENSER: &str = "ZeissCondenser";
pub const DEVICE_NAME_TUBELENS: &str = "ZeissTubelens";
pub const DEVICE_NAME_BASE_PORT: &str = "ZeissBasePortSlider";
pub const DEVICE_NAME_SIDE_PORT: &str = "ZeissSidePortTurret";
pub const DEVICE_NAME_LAMP_MIRROR: &str = "ZeissExcitationLampSwitcher";
pub const DEVICE_NAME_HALOGEN: &str = "ZeissHalogenLamp";

pub type SharedZeissTransport = Arc<Mutex<Box<dyn Transport>>>;

/// Shared hub that owns the transport and provides send/receive for all sub-devices.
pub struct ZeissHub {
    props: PropertyMap,
    transport: Option<SharedZeissTransport>,
    initialized: Cell<bool>,
    firmware: RefCell<String>,
    version: Cell<f64>,
}

impl ZeissHub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "Microscope Version",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: Cell::new(false),
            firmware: RefCell::new(String::new()),
            version: Cell::new(0.0),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(Arc::new(Mutex::new(t)));
        self
    }

    pub fn with_shared_transport(mut self, transport: SharedZeissTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn shared_transport(&self) -> Option<SharedZeissTransport> {
        self.transport.clone()
    }

    pub fn is_connected(&self) -> bool {
        self.transport.is_some()
    }

    /// Send a command (appends `\r`) and return the trimmed response.
    pub fn send(&self, command: &str) -> MmResult<String> {
        let c = format!("{}\r", command);
        match self.transport.as_ref() {
            Some(t) => {
                let mut t = t
                    .lock()
                    .map_err(|_| MmError::LocallyDefined("Zeiss transport lock poisoned".into()))?;
                t.purge()?;
                Ok(t.send_recv(&c)?.trim().to_string())
            }
            None => Err(MmError::NotConnected),
        }
    }

    pub fn execute(&self, command: &str) -> MmResult<()> {
        let c = format!("{}\r", command);
        match self.transport.as_ref() {
            Some(t) => {
                let mut t = t
                    .lock()
                    .map_err(|_| MmError::LocallyDefined("Zeiss transport lock poisoned".into()))?;
                t.purge()?;
                t.send(&c)
            }
            None => Err(MmError::NotConnected),
        }
    }

    pub fn get_version(&self) -> MmResult<String> {
        let response = self.send("HPTv0")?;
        let body = response
            .strip_prefix("PH")
            .ok_or(MmError::SerialInvalidResponse)?;
        let firmware = if body.starts_with("AP") || body.starts_with("AV") {
            "ZM"
        } else {
            "MF"
        };
        *self.firmware.borrow_mut() = firmware.to_string();

        let mut tmp = body.get(4..).unwrap_or_default().to_string();
        if let Some(pos) = tmp.rfind('_') {
            tmp.replace_range(pos..=pos, ".");
        }
        self.version.set(tmp.parse::<f64>().unwrap_or(0.0));

        Ok(format!("Application version: {}", body))
    }

    pub fn get_mcu28_version(&self) -> MmResult<String> {
        let response = self.send("NPTv0")?;
        let body = response
            .strip_prefix("PN")
            .ok_or(MmError::SerialInvalidResponse)?;
        Ok(format!("Application version: {}", body))
    }

    pub fn firmware(&self) -> String {
        self.firmware.borrow().clone()
    }

    pub fn initialized(&self) -> bool {
        self.initialized.get()
    }

    pub fn parse_prefixed_i64(response: &str, prefix: &str) -> MmResult<i64> {
        response
            .strip_prefix(prefix)
            .ok_or(MmError::SerialInvalidResponse)?
            .trim()
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)
    }

    fn query_body(&self, command: &str, prefix: &str) -> MmResult<String> {
        let response = self.send(command)?;
        Ok(response
            .strip_prefix(prefix)
            .ok_or(MmError::SerialInvalidResponse)?
            .to_string())
    }

    pub fn detect_installed_device_names(&self) -> MmResult<Vec<String>> {
        let mut devices = Vec::new();
        let turrets = [
            (1, DEVICE_NAME_REFLECTOR),
            (2, DEVICE_NAME_OBJECTIVES),
            (7, DEVICE_NAME_FILTER1),
            (8, DEVICE_NAME_FILTER2),
            (32, DEVICE_NAME_CONDENSER),
            (36, DEVICE_NAME_TUBELENS),
            (38, DEVICE_NAME_BASE_PORT),
            (39, DEVICE_NAME_SIDE_PORT),
            (51, DEVICE_NAME_LAMP_MIRROR),
            (6, DEVICE_NAME_OPTOVAR),
            (4, DEVICE_NAME_EXT_FILTER),
        ];
        let mut stand_unreachable = false;
        for (id, name) in turrets {
            match self.query_body(&format!("HPCr{},0", id), "PH") {
                Ok(answer) if answer == "1" || answer == "2" => devices.push(name.to_string()),
                Ok(_) => {}
                Err(MmError::SerialInvalidResponse) => {}
                Err(_) => {
                    stand_unreachable = true;
                    break;
                }
            }
        }

        if !stand_unreachable {
            devices.push(DEVICE_NAME_HALOGEN.to_string());
            if let Ok(answer) = self.query_body("HPCk1,0", "PH") {
                if answer != "0" {
                    devices.push(DEVICE_NAME_SHUTTER.to_string());
                }
                if answer != "1F" {
                    devices.push(DEVICE_NAME_FOCUS.to_string());
                }
            }
            if let Ok(answer) = self.query_body("HPCm1,0", "PH") {
                if answer != "0" && answer != "55" {
                    devices.push(
                        (if self.firmware() == "MF" {
                            DEVICE_NAME_SHUTTER_MF
                        } else {
                            DEVICE_NAME_SHUTTER
                        })
                        .to_string(),
                    );
                }
            }
        }

        if self.get_mcu28_version().is_ok() {
            devices.push(DEVICE_NAME_XY.to_string());
        }
        devices.sort();
        devices.dedup();
        Ok(devices)
    }
}

impl Clone for ZeissHub {
    fn clone(&self) -> Self {
        let mut cloned = Self::new().with_optional_shared_transport(self.shared_transport());
        cloned.initialized.set(self.initialized.get());
        *cloned.firmware.borrow_mut() = self.firmware();
        cloned.version.set(self.version.get());
        if let Ok(version) = self.get_property("Microscope Version") {
            if let Some(entry) = cloned.props.entry_mut("Microscope Version") {
                entry.value = version;
            }
        }
        cloned
    }
}

impl ZeissHub {
    fn with_optional_shared_transport(mut self, transport: Option<SharedZeissTransport>) -> Self {
        self.transport = transport;
        self
    }
}

impl Default for ZeissHub {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ZeissHub {
    fn name(&self) -> &str {
        DEVICE_NAME_HUB
    }
    fn description(&self) -> &str {
        "Zeiss CAN-bus hub"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.is_connected() {
            return Err(MmError::NotConnected);
        }
        let version = self
            .get_version()
            .or_else(|_| self.get_version())
            .or_else(|_| self.get_mcu28_version())?;
        if let Some(entry) = self.props.entry_mut("Microscope Version") {
            entry.value = PropertyValue::String(version);
        }
        self.initialized.set(true);
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized.set(false);
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
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
        DeviceType::Hub
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Hub for ZeissHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        self.detect_installed_device_names()
    }
}

/// Encode a signed position as 24-bit two's complement uppercase hex (6 chars).
pub fn encode_pos(steps: i32) -> String {
    let raw = if steps >= 0 {
        steps as u32
    } else {
        (steps as i64 + 0x100_0000) as u32
    };
    format!("{:06X}", raw & 0xFF_FFFF)
}

/// Decode a 24-bit two's complement hex string to a signed i32.
pub fn decode_pos(hex: &str) -> MmResult<i32> {
    let raw = u32::from_str_radix(hex.trim(), 16)
        .map_err(|_| MmError::LocallyDefined(format!("Zeiss hex parse error: '{}'", hex)))?;
    Ok(if raw & 0x80_0000 != 0 {
        (raw as i64 - 0x100_0000) as i32
    } else {
        raw as i32
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{Device, Hub};
    use crate::transport::MockTransport;
    use crate::transport::Transport;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    struct RecordingTransport {
        script: VecDeque<String>,
        commands: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingTransport {
        fn new(responses: &[&str], commands: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                script: responses.iter().map(|s| s.to_string()).collect(),
                commands,
            }
        }
    }

    impl Transport for RecordingTransport {
        fn send(&mut self, cmd: &str) -> MmResult<()> {
            self.commands.lock().unwrap().push(cmd.to_string());
            Ok(())
        }

        fn receive_line(&mut self) -> MmResult<String> {
            self.script.pop_front().ok_or(MmError::SerialTimeout)
        }

        fn purge(&mut self) -> MmResult<()> {
            Ok(())
        }
    }

    #[test]
    fn encode_positive() {
        assert_eq!(encode_pos(100), "000064");
    }
    #[test]
    fn encode_zero() {
        assert_eq!(encode_pos(0), "000000");
    }
    #[test]
    fn encode_negative() {
        assert_eq!(encode_pos(-1), "FFFFFF");
    }
    #[test]
    fn roundtrip() {
        for v in [-100_000i32, -1, 0, 1, 100_000] {
            assert_eq!(decode_pos(&encode_pos(v)).unwrap(), v);
        }
    }

    #[test]
    fn initialize_parses_stand_version() {
        let mut hub = ZeissHub::new()
            .with_transport(Box::new(MockTransport::new().expect("HPTv0\r", "PHAP2_09")));
        hub.initialize().unwrap();
        assert_eq!(hub.firmware(), "ZM");
        assert_eq!(
            hub.get_property("Microscope Version").unwrap(),
            PropertyValue::String("Application version: AP2_09".into())
        );
    }

    #[test]
    fn cloned_hubs_share_one_transport() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let hub = ZeissHub::new().with_transport(Box::new(RecordingTransport::new(
            &["PH1", "PH2"],
            commands.clone(),
        )));
        let child_hub = hub.clone();

        assert_eq!(hub.send("HPCr1,0").unwrap(), "PH1");
        assert_eq!(child_hub.send("HPCr2,0").unwrap(), "PH2");

        assert_eq!(
            commands.lock().unwrap().as_slice(),
            &["HPCr1,0\r", "HPCr2,0\r"]
        );
    }

    #[test]
    fn discovery_checks_turrets_shutter_focus_and_mcu28() {
        let t = MockTransport::new()
            .expect("HPCr1,0\r", "PH1")
            .expect("HPCr2,0\r", "PH0")
            .expect("HPCr7,0\r", "PH0")
            .expect("HPCr8,0\r", "PH0")
            .expect("HPCr32,0\r", "PH0")
            .expect("HPCr36,0\r", "PH0")
            .expect("HPCr38,0\r", "PH0")
            .expect("HPCr39,0\r", "PH0")
            .expect("HPCr51,0\r", "PH0")
            .expect("HPCr6,0\r", "PH0")
            .expect("HPCr4,0\r", "PH0")
            .expect("HPCk1,0\r", "PH2")
            .expect("HPCm1,0\r", "PH55")
            .expect("NPTv0\r", "PNMCU28");
        let mut hub = ZeissHub::new().with_transport(Box::new(t));
        let devices = hub.detect_installed_devices().unwrap();
        assert!(devices.contains(&DEVICE_NAME_REFLECTOR.to_string()));
        assert!(devices.contains(&DEVICE_NAME_SHUTTER.to_string()));
        assert!(devices.contains(&DEVICE_NAME_FOCUS.to_string()));
        assert!(devices.contains(&DEVICE_NAME_XY.to_string()));
    }
}
