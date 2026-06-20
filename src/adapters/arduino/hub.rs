/// ArduinoHub — manages the serial port and shared state (switch + shutter bits).
///
/// Binary protocol:
/// - Send byte `30` → response "MM-Ard\r\n" + optional extended version byte
/// - Switch command: `[1, state]` → response `[1]`
/// - DA command:     `[3, channel-1, hi_byte, lo_byte]` → response `[3]`
use parking_lot::Mutex;
use std::sync::Arc;

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Hub};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub const FIRMWARE_MIN: u8 = 1;
pub const FIRMWARE_MAX: u8 = 5;

/// Shared mutable state between hub and its peripherals.
#[derive(Debug, Default)]
pub struct HubState {
    /// Current 16-bit digital output state.
    pub switch_state: u16,
    /// Current shutter bit (bit 0 of switch_state).
    pub shutter_state: bool,
}

pub struct ArduinoHub {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    firmware_version: u8,
    extended_version: i64,
    max_num_patterns: u16,
    num_da_channels: u8,
    num_digital_pins: u8,
    pub shared: Arc<Mutex<HubState>>,
    inverted_logic: bool,
}

impl ArduinoHub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Logic", PropertyValue::String("Normal".into()), false)
            .unwrap();
        props
            .set_allowed_values("Logic", &["Normal", "Inverted"])
            .unwrap();
        props
            .define_property("Version", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("ExtendedVersion", PropertyValue::Integer(0), true)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            firmware_version: 0,
            extended_version: 0,
            max_num_patterns: 12,
            num_da_channels: 2,
            num_digital_pins: 6,
            shared: Arc::new(Mutex::new(HubState::default())),
            inverted_logic: false,
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

    /// Send the switch state (16-bit) to the Arduino.
    pub fn write_switch_state(&mut self, state: u16) -> MmResult<()> {
        let mask = (1u16 << self.num_digital_pins.min(8)) - 1;
        let mut value = state & mask;
        if self.inverted_logic {
            value = !value;
        }
        let cmd = [1, value as u8];
        self.call_transport(|t| {
            t.purge()?;
            t.send_bytes(&cmd)?;
            let resp = t.receive_bytes(1)?;
            if resp.first() != Some(&1) {
                return Err(MmError::SerialInvalidResponse);
            }
            Ok(())
        })
    }

    /// Send a DA value (0–4095) to a channel (1-based).
    pub fn write_da(&mut self, channel: u8, value: u16) -> MmResult<()> {
        let hi = ((value >> 8) & 0xFF) as u8;
        let lo = (value & 0xFF) as u8;
        let cmd = [3, channel - 1, hi, lo];
        self.call_transport(|t| {
            t.purge()?;
            t.send_bytes(&cmd)?;
            let resp = t.receive_bytes(4)?;
            if resp.first() != Some(&3) {
                return Err(MmError::SerialInvalidResponse);
            }
            Ok(())
        })
    }

    pub fn firmware_version(&self) -> u8 {
        self.firmware_version
    }

    pub fn num_da_channels(&self) -> u8 {
        self.num_da_channels
    }

    pub fn num_digital_pins(&self) -> u8 {
        self.num_digital_pins
    }
}

impl Default for ArduinoHub {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ArduinoHub {
    fn name(&self) -> &str {
        "Arduino-Hub"
    }
    fn description(&self) -> &str {
        "Arduino Hub (required)"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Command 30 only identifies the controller. Command 31 returns the
        // firmware API version used for compatibility checks.
        let resp = self.call_transport(|t| {
            t.purge()?;
            t.send_bytes(&[30])?;
            t.receive_line()
        })?;

        if !resp.starts_with("MM-Ard") {
            return Err(MmError::LocallyDefined(
                "Arduino board not found or wrong firmware".into(),
            ));
        }

        if resp.len() > 7 {
            self.extended_version = resp[7..].trim().parse().unwrap_or(0);
        }

        let version_resp = self.call_transport(|t| {
            t.send_bytes(&[31])?;
            t.receive_line()
        })?;
        let ver: u8 = version_resp
            .trim()
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;

        if ver < FIRMWARE_MIN || ver > FIRMWARE_MAX {
            return Err(MmError::LocallyDefined(format!(
                "Firmware version {} not supported (expected {}-{})",
                ver, FIRMWARE_MIN, FIRMWARE_MAX
            )));
        }

        self.firmware_version = ver;
        self.props
            .entry_mut("Version")
            .map(|e| e.value = PropertyValue::Integer(ver as i64));
        self.props
            .entry_mut("ExtendedVersion")
            .map(|e| e.value = PropertyValue::Integer(self.extended_version));

        if ver >= 3 {
            let answer = self.call_transport(|t| {
                t.send_bytes(&[32])?;
                t.receive_bytes(3)
            })?;
            if answer.len() != 3 || answer[0] != 32 {
                return Err(MmError::SerialInvalidResponse);
            }
            self.max_num_patterns = ((answer[1] as u16) << 8) | answer[2] as u16;
        }

        if ver >= 5 {
            let da_answer = self.call_transport(|t| {
                t.send_bytes(&[34])?;
                t.receive_bytes(2)
            })?;
            if da_answer.len() != 2 || da_answer[0] != 34 {
                return Err(MmError::SerialInvalidResponse);
            }
            self.num_da_channels = da_answer[1];

            let digital_answer = self.call_transport(|t| {
                t.send_bytes(&[35])?;
                t.receive_bytes(2)
            })?;
            if digital_answer.len() != 2 || digital_answer[0] != 35 {
                return Err(MmError::SerialInvalidResponse);
            }
            self.num_digital_pins = digital_answer[1].min(8);
        }

        // Check logic setting
        if let Ok(PropertyValue::String(logic)) = self.props.get("Logic").cloned() {
            self.inverted_logic = logic == "Inverted";
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Logic" {
            let s = val.as_str().to_string();
            self.inverted_logic = s == "Inverted";
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
        DeviceType::Hub
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Hub for ArduinoHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        let mut devices = vec!["Arduino-Shutter".to_string(), "Arduino-Switch".to_string()];
        if self.num_da_channels >= 1 {
            devices.push("Arduino-DAC1".to_string());
        }
        if self.num_da_channels >= 2 {
            devices.push("Arduino-DAC2".to_string());
        }
        Ok(devices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;
    use crate::transport::Transport;
    use std::collections::VecDeque;

    struct ByteTransport {
        responses: VecDeque<Vec<u8>>,
        sent: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    }

    impl ByteTransport {
        fn new(responses: Vec<Vec<u8>>) -> (Self, Arc<std::sync::Mutex<Vec<Vec<u8>>>>) {
            let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
            (
                Self {
                    responses: responses.into(),
                    sent: sent.clone(),
                },
                sent,
            )
        }
    }

    impl Transport for ByteTransport {
        fn send(&mut self, cmd: &str) -> MmResult<()> {
            self.sent.lock().unwrap().push(cmd.bytes().collect());
            Ok(())
        }

        fn receive_line(&mut self) -> MmResult<String> {
            Err(MmError::SerialTimeout)
        }

        fn purge(&mut self) -> MmResult<()> {
            Ok(())
        }

        fn send_bytes(&mut self, bytes: &[u8]) -> MmResult<()> {
            self.sent.lock().unwrap().push(bytes.to_vec());
            Ok(())
        }

        fn receive_bytes(&mut self, n: usize) -> MmResult<Vec<u8>> {
            let response = self.responses.pop_front().ok_or(MmError::SerialTimeout)?;
            Ok(response[..response.len().min(n)].to_vec())
        }
    }

    fn make_hub() -> ArduinoHub {
        let transport = MockTransport::new().any("MM-Ard").any("2"); // firmware v2 response to byte 31
        ArduinoHub::new().with_transport(Box::new(transport))
    }

    #[test]
    fn initialize_ok() {
        let mut hub = make_hub();
        hub.initialize().unwrap();
        assert_eq!(hub.firmware_version(), 2);
    }

    #[test]
    fn initializes_v5_capabilities() {
        let transport = MockTransport::new()
            .any("MM-Ard")
            .any("5")
            .expect_binary(&[32, 0, 12])
            .expect_binary(&[34, 2])
            .expect_binary(&[35, 6]);
        let mut hub = ArduinoHub::new().with_transport(Box::new(transport));
        hub.initialize().unwrap();
        assert_eq!(hub.firmware_version(), 5);
        assert_eq!(hub.num_da_channels(), 2);
        assert_eq!(hub.num_digital_pins(), 6);
    }

    #[test]
    fn bad_firmware_rejected() {
        let transport = MockTransport::new().any("WrongDevice");
        let mut hub = ArduinoHub::new().with_transport(Box::new(transport));
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn default_logic_writes_normal_switch_state() {
        let (transport, sent) = ByteTransport::new(vec![vec![1]]);
        let mut hub = ArduinoHub::new().with_transport(Box::new(transport));

        hub.write_switch_state(0b0010_1010).unwrap();

        assert_eq!(&*sent.lock().unwrap(), &[vec![1, 0b0010_1010]]);
    }

    #[test]
    fn inverted_logic_inverts_masked_switch_state() {
        let (transport, sent) = ByteTransport::new(vec![vec![1]]);
        let mut hub = ArduinoHub::new().with_transport(Box::new(transport));
        hub.set_property("Logic", PropertyValue::String("Inverted".into()))
            .unwrap();

        hub.write_switch_state(0b0010_1010).unwrap();

        assert_eq!(&*sent.lock().unwrap(), &[vec![1, 0b1101_0101]]);
    }
}
