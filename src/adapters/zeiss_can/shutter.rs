/// Zeiss CAN-bus shutter (reflected light / fluorescence shutter).
///
/// Protocol (TX `\r`, RX `\r`):
///   `HPCK1,1\r`  (close internal shutter)
///   `HPCK1,2\r`  (open internal shutter)
///   `HPCk1,1\r`  → `PH{1|2}\r`  (query shutter state)
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::types::{DeviceType, PropertyValue};
use std::cell::Cell;
use std::time::Instant;

use super::hub::{ZeissHub, DEVICE_NAME_SHUTTER, DEVICE_NAME_SHUTTER_MF};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShutterFirmware {
    Standard,
    Mf,
}

pub struct ZeissShutter {
    props: PropertyMap,
    hub: ZeissHub,
    initialized: bool,
    open: Cell<bool>,
    shutter_nr: Cell<u8>,
    external: Cell<bool>,
    firmware: ShutterFirmware,
    changed_time: Cell<Instant>,
    delay_ms: f64,
}

impl ZeissShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("ZeissShutterNr", PropertyValue::Integer(1), false)
            .unwrap();
        props.set_allowed_values("ZeissShutterNr", &["1"]).unwrap();
        props
            .define_property(
                "External-Internal Shutter",
                PropertyValue::String("Internal".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("External-Internal Shutter", &["Internal", "External"])
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("State", &["0", "1"]).unwrap();
        props
            .define_property("Delay_ms", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("Delay_ms", 0.0, f64::MAX)
            .unwrap();
        Self {
            props,
            hub: ZeissHub::new(),
            initialized: false,
            open: Cell::new(false),
            shutter_nr: Cell::new(1),
            external: Cell::new(false),
            firmware: ShutterFirmware::Standard,
            changed_time: Cell::new(Instant::now()),
            delay_ms: 0.0,
        }
    }

    pub fn new_mf() -> Self {
        let mut s = Self::new();
        s.firmware = ShutterFirmware::Mf;
        s.props
            .set_allowed_values("ZeissShutterNr", &["1", "2", "3"])
            .unwrap();
        s
    }

    pub fn new_with_hub(hub: ZeissHub) -> Self {
        let mut s = Self::new();
        s.hub = hub;
        s
    }

    pub fn new_mf_with_hub(hub: ZeissHub) -> Self {
        let mut s = Self::new_mf();
        s.hub = hub;
        s
    }

    fn send(&self, cmd: &str) -> MmResult<String> {
        self.hub.send(cmd)
    }

    fn command_prefix(&self) -> &'static str {
        if self.external.get() {
            "SP"
        } else {
            "HP"
        }
    }

    fn read_open(&self) -> MmResult<bool> {
        if self.firmware == ShutterFirmware::Mf {
            let resp = self.send(&format!("HPCm{},1", self.shutter_nr.get()))?;
            if resp == "H1" {
                return Ok(false);
            }
            let body = resp
                .strip_prefix("PH")
                .ok_or(MmError::SerialInvalidResponse)?;
            return match body.trim().parse::<u8>() {
                Ok(1) => Ok(false),
                Ok(2) => Ok(true),
                _ => Err(MmError::SerialInvalidResponse),
            };
        }

        let resp = self.send(&format!(
            "{}Ck{},1",
            self.command_prefix(),
            self.shutter_nr.get()
        ))?;
        if resp == "H1" {
            return Ok(false);
        }
        let body = resp
            .strip_prefix(if self.external.get() { "PS" } else { "PH" })
            .ok_or(MmError::SerialInvalidResponse)?;
        match body.chars().next() {
            Some('1') => Ok(false),
            Some('2') => Ok(true),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }
}

impl Default for ZeissShutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ZeissShutter {
    fn name(&self) -> &str {
        if self.firmware == ShutterFirmware::Mf {
            DEVICE_NAME_SHUTTER_MF
        } else {
            DEVICE_NAME_SHUTTER
        }
    }
    fn description(&self) -> &str {
        "Zeiss CAN-bus reflected light shutter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.hub.is_connected() {
            return Err(MmError::NotConnected);
        }
        self.open.set(self.read_open()?);
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::Integer(if self.read_open()? {
                1
            } else {
                0
            })),
            "ZeissShutterNr" => Ok(PropertyValue::Integer(self.shutter_nr.get() as i64)),
            "External-Internal Shutter" => Ok(PropertyValue::String(
                if self.external.get() {
                    "External"
                } else {
                    "Internal"
                }
                .into(),
            )),
            "Delay_ms" => Ok(PropertyValue::Float(self.delay_ms)),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let open = val.as_i64().ok_or(MmError::InvalidPropertyValue)? != 0;
                self.set_open(open)?;
                self.props
                    .set(name, PropertyValue::Integer(if open { 1 } else { 0 }))
            }
            "ZeissShutterNr" => {
                if self.initialized {
                    return Err(MmError::InvalidPropertyValue);
                }
                let nr = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                let max_nr = if self.firmware == ShutterFirmware::Mf {
                    3
                } else {
                    1
                };
                if nr < 1 || nr > max_nr {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.shutter_nr.set(nr as u8);
                self.props.set(name, PropertyValue::Integer(nr))
            }
            "External-Internal Shutter" => {
                let value = val.to_string();
                self.external.set(value == "External");
                self.props.set(name, PropertyValue::String(value))
            }
            "Delay_ms" => {
                let delay = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.delay_ms = delay;
                self.props.set(name, PropertyValue::Float(delay))
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
        let timer_busy = self.changed_time.get().elapsed().as_secs_f64() * 1000.0 < self.delay_ms;
        let cmd = if self.firmware == ShutterFirmware::Mf {
            "HPSb2"
        } else if self.external.get() {
            "SPSb2"
        } else {
            "HPSb2"
        };
        let shutter_busy = self
            .send(cmd)
            .ok()
            .and_then(|resp| {
                resp.strip_prefix("PH")
                    .or_else(|| resp.strip_prefix("PS"))
                    .and_then(|s| s.trim().parse::<u8>().ok())
            })
            .map(|status| {
                if self.firmware == ShutterFirmware::Mf {
                    ((status >> (3 + self.shutter_nr.get())) & 1) != 0
                } else {
                    ((status >> 4) & 1) != 0
                }
            })
            .unwrap_or(false);
        timer_busy || shutter_busy
    }
}

impl Shutter for ZeissShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.changed_time.set(Instant::now());
        let state = if open { 2 } else { 1 };
        if self.firmware == ShutterFirmware::Mf {
            self.hub
                .execute(&format!("HPCM{},{}", self.shutter_nr.get(), state))?;
        } else {
            self.hub.execute(&format!(
                "{}CK{},{}",
                self.command_prefix(),
                self.shutter_nr.get(),
                state
            ))?;
        }
        self.open.set(open);
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        let open = self.read_open()?;
        self.open.set(open);
        Ok(open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::NotSupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;
    use crate::transport::Transport;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    fn shutter_with(t: MockTransport) -> ZeissShutter {
        let hub = ZeissHub::new().with_transport(Box::new(t));
        ZeissShutter::new_with_hub(hub)
    }

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
    fn initialize_reads_state() {
        let t = MockTransport::new()
            .expect("HPCk1,1\r", "PH1")
            .expect("HPCk1,1\r", "PH1");
        let mut s = shutter_with(t);
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn open_close() {
        let t = MockTransport::new()
            .expect("HPCk1,1\r", "PH1")
            .expect("HPCk1,1\r", "PH2")
            .expect("HPCk1,1\r", "PH1");
        let mut s = shutter_with(t);
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn mf_shutter_uses_m_commands_and_allows_three_shutter_numbers() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let t = RecordingTransport::new(&["PH1", "PH2", "PH64"], commands.clone());
        let hub = ZeissHub::new().with_transport(Box::new(t));
        let mut s = ZeissShutter::new_mf_with_hub(hub);
        s.set_property("ZeissShutterNr", PropertyValue::Integer(3))
            .unwrap();

        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        assert!(s.busy());

        assert_eq!(
            commands.lock().unwrap().as_slice(),
            &["HPCm3,1\r", "HPCM3,2\r", "HPCm3,1\r", "HPSb2\r"]
        );
    }
}
