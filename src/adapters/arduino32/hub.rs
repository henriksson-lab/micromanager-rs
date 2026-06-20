//! Arduino32Hub — manages serial port and shared state.
//!
//! Protocol (identical to original Arduino except uses 8-bit write and separate version query):
//! - Send byte 30 → response "MM-Ard\r\n" (board identification)
//! - Send byte 31 → response "<version integer>\r\n"
//! - Switch: `[1, value]` -> response byte `[1]`
//! - DA:     `[3, ch-1, hi, lo]` -> response byte `[3]`

use parking_lot::Mutex;
use std::sync::Arc;

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Hub};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub const FIRMWARE_MIN: i32 = 3;
pub const FIRMWARE_MAX: i32 = 3;

#[derive(Debug, Default)]
pub struct HubState {
    pub switch_state: u8,
    pub shutter_open: bool,
}

pub struct Arduino32Hub {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    firmware_version: i32,
    pub shared: Arc<Mutex<HubState>>,
    inverted_logic: bool,
}

impl Arduino32Hub {
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

        Self {
            props,
            transport: None,
            initialized: false,
            firmware_version: 0,
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

    fn write_switch_byte(&mut self, value: u8) -> MmResult<()> {
        let effective = if self.inverted_logic { !value } else { value };
        self.call_transport(|t| {
            t.send_bytes(&[1, effective])?;
            let resp = t.receive_bytes(1)?;
            if resp.first() != Some(&1) {
                return Err(MmError::SerialInvalidResponse);
            }
            Ok(())
        })
    }

    /// Send the switch state as the upstream adapter does: bit 0 is reserved for
    /// the shutter, so switch writes only affect bits 1..7.
    pub fn write_switch_state(&mut self, state: u8) -> MmResult<()> {
        self.write_switch_byte(state & 254)?;
        self.shared.lock().switch_state = state;
        Ok(())
    }

    /// Send the shutter-controlled output. Closed sends zero; open restores the
    /// cached switch state, limited to the six lower non-shutter bits upstream uses.
    pub fn write_shutter_state(&mut self, open: bool) -> MmResult<()> {
        let switch_state = self.shared.lock().switch_state;
        let value = if open { switch_state & 63 } else { 0 };
        self.write_switch_byte(value)?;
        self.shared.lock().shutter_open = open;
        Ok(())
    }

    /// Send a 12-bit DA value to a 1-based channel.
    pub fn write_da(&mut self, channel: u8, value: u16) -> MmResult<()> {
        let hi = ((value >> 8) & 0x0F) as u8;
        let lo = (value & 0xFF) as u8;
        self.call_transport(|t| {
            t.send_bytes(&[3, channel - 1, hi, lo])?;
            let resp = t.receive_bytes(4)?;
            if resp.first() != Some(&3) {
                return Err(MmError::SerialInvalidResponse);
            }
            Ok(())
        })
    }

    pub fn firmware_version(&self) -> i32 {
        self.firmware_version
    }
    pub fn is_inverted(&self) -> bool {
        self.inverted_logic
    }
}

impl Default for Arduino32Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Arduino32Hub {
    fn name(&self) -> &str {
        "Arduino32-Hub"
    }
    fn description(&self) -> &str {
        "Arduino32 Hub (required)"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Step 1: identify board — send byte 30, expect "MM-Ard"
        let id_resp = self.call_transport(|t| {
            t.send("\x1e")?; // 0x1e = 30
            t.receive_line()
        })?;

        if id_resp.trim() != "MM-Ard" {
            return Err(MmError::LocallyDefined(
                "Arduino32 board not found or wrong firmware".into(),
            ));
        }

        // Step 2: query version — send byte 31, expect integer string
        let ver_resp = self.call_transport(|t| {
            t.send("\x1f")?; // 0x1f = 31
            t.receive_line()
        })?;

        let ver: i32 = ver_resp
            .trim()
            .parse()
            .map_err(|_| MmError::LocallyDefined("Could not parse firmware version".into()))?;

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
            self.inverted_logic = val.as_str() == "Inverted";
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

impl Hub for Arduino32Hub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        Ok(vec![
            "Arduino32-Shutter".into(),
            "Arduino32-Switch".into(),
            "Arduino32-Input".into(),
            "Arduino32-DAC/PWM-1".into(),
            "Arduino32-DAC/PWM-2".into(),
            "Arduino32-DAC/PWM-3".into(),
            "Arduino32-DAC/PWM-4".into(),
            "Arduino32-DAC/PWM-5".into(),
            "Arduino32-DAC/PWM-6".into(),
            "Arduino32-DAC/PWM-7".into(),
            "Arduino32-DAC/PWM-8".into(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex as StdMutex};

    struct RecordingTransport {
        sent: Arc<StdMutex<Vec<Vec<u8>>>>,
        replies: VecDeque<Vec<u8>>,
    }

    impl RecordingTransport {
        fn new(
            sent: Arc<StdMutex<Vec<Vec<u8>>>>,
            replies: impl IntoIterator<Item = Vec<u8>>,
        ) -> Self {
            Self {
                sent,
                replies: replies.into_iter().collect(),
            }
        }
    }

    impl Transport for RecordingTransport {
        fn send(&mut self, _cmd: &str) -> MmResult<()> {
            Err(MmError::LocallyDefined("text send not expected".into()))
        }

        fn receive_line(&mut self) -> MmResult<String> {
            Err(MmError::LocallyDefined("line receive not expected".into()))
        }

        fn purge(&mut self) -> MmResult<()> {
            Ok(())
        }

        fn send_bytes(&mut self, bytes: &[u8]) -> MmResult<()> {
            self.sent.lock().unwrap().push(bytes.to_vec());
            Ok(())
        }

        fn receive_bytes(&mut self, n: usize) -> MmResult<Vec<u8>> {
            let reply = self.replies.pop_front().ok_or(MmError::SerialTimeout)?;
            Ok(reply[..reply.len().min(n)].to_vec())
        }
    }

    fn make_hub() -> Arduino32Hub {
        let t = MockTransport::new()
            .expect("\x1e", "MM-Ard") // id query
            .expect("\x1f", "3"); // version query
        Arduino32Hub::new().with_transport(Box::new(t))
    }

    #[test]
    fn initialize_ok() {
        let mut hub = make_hub();
        hub.initialize().unwrap();
        assert_eq!(hub.firmware_version(), 3);
    }

    #[test]
    fn default_logic_matches_upstream_internal_state() {
        let hub = Arduino32Hub::new();
        assert!(!hub.is_inverted());
        assert_eq!(
            hub.get_property("Logic").unwrap(),
            PropertyValue::String("Normal".into())
        );
    }

    #[test]
    fn bad_id_rejected() {
        let t = MockTransport::new().any("WrongBoard").any("3");
        let mut hub = Arduino32Hub::new().with_transport(Box::new(t));
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn switch_write_uses_raw_bytes_and_masks_shutter_bit() {
        let sent = Arc::new(StdMutex::new(Vec::new()));
        let t = RecordingTransport::new(sent.clone(), [vec![1]]);
        let mut hub = Arduino32Hub::new().with_transport(Box::new(t));
        hub.write_switch_state(0x81).unwrap();
        assert_eq!(sent.lock().unwrap().as_slice(), &[vec![1, 0x80]]);
    }

    #[test]
    fn explicit_inverted_logic_inverts_switch_byte() {
        let sent = Arc::new(StdMutex::new(Vec::new()));
        let t = RecordingTransport::new(sent.clone(), [vec![1]]);
        let mut hub = Arduino32Hub::new().with_transport(Box::new(t));
        hub.set_property("Logic", PropertyValue::String("Inverted".into()))
            .unwrap();
        hub.write_switch_state(0x81).unwrap();
        assert_eq!(sent.lock().unwrap().as_slice(), &[vec![1, 0x7f]]);
    }

    #[test]
    fn da_write_uses_raw_bytes() {
        let sent = Arc::new(StdMutex::new(Vec::new()));
        let t = RecordingTransport::new(sent.clone(), [vec![3, 0, 0, 0]]);
        let mut hub = Arduino32Hub::new().with_transport(Box::new(t));
        hub.write_da(2, 0x0abc).unwrap();
        assert_eq!(sent.lock().unwrap().as_slice(), &[vec![3, 1, 0x0a, 0xbc]]);
    }

    #[test]
    fn installed_devices_match_upstream_order() {
        let mut hub = Arduino32Hub::new();
        assert_eq!(
            hub.detect_installed_devices().unwrap(),
            vec![
                "Arduino32-Shutter",
                "Arduino32-Switch",
                "Arduino32-Input",
                "Arduino32-DAC/PWM-1",
                "Arduino32-DAC/PWM-2",
                "Arduino32-DAC/PWM-3",
                "Arduino32-DAC/PWM-4",
                "Arduino32-DAC/PWM-5",
                "Arduino32-DAC/PWM-6",
                "Arduino32-DAC/PWM-7",
                "Arduino32-DAC/PWM-8",
            ]
        );
    }
}
