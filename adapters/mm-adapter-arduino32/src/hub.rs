//! Arduino32Hub — manages serial port and shared state.
//!
//! Protocol (identical to original Arduino except uses 8-bit write and separate version query):
//! - Send byte 30 → response "MM-Ard\r\n" (board identification)
//! - Send byte 31 → response "<version integer>\r\n"
//! - Switch: `[1, value]` → response byte `[1]`
//! - DA:     `[3, ch-1, hi, lo]` → response byte `[3]`

use parking_lot::Mutex;
use std::sync::Arc;

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Hub};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

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
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Logic", PropertyValue::String("Inverted".into()), false).unwrap();
        props.set_allowed_values("Logic", &["Normal", "Inverted"]).unwrap();
        props.define_property("Version", PropertyValue::Integer(0), true).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            firmware_version: 0,
            shared: Arc::new(Mutex::new(HubState::default())),
            inverted_logic: true,
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

    /// Send an 8-bit switch state to the Arduino32.
    pub fn write_switch_state(&mut self, state: u8) -> MmResult<()> {
        let effective = if self.inverted_logic { !state } else { state };
        let cmd = format!("\x01{}", effective as char);
        self.call_transport(|t| {
            t.send(&cmd)?;
            let resp = t.receive_line()?;
            if resp.as_bytes().first() != Some(&1) {
                return Err(MmError::SerialInvalidResponse);
            }
            Ok(())
        })?;
        self.shared.lock().switch_state = state;
        Ok(())
    }

    /// Send a 12-bit DA value to a 1-based channel.
    pub fn write_da(&mut self, channel: u8, value: u16) -> MmResult<()> {
        let hi = ((value >> 8) & 0x0F) as u8;
        let lo = (value & 0xFF) as u8;
        let cmd = format!("\x03{}{}{}", (channel - 1) as char, hi as char, lo as char);
        self.call_transport(|t| {
            t.send(&cmd)?;
            let resp = t.receive_line()?;
            if resp.as_bytes().first() != Some(&3) {
                return Err(MmError::SerialInvalidResponse);
            }
            Ok(())
        })
    }

    pub fn firmware_version(&self) -> i32 { self.firmware_version }
    pub fn is_inverted(&self) -> bool { self.inverted_logic }
}

impl Default for Arduino32Hub {
    fn default() -> Self { Self::new() }
}

impl Device for Arduino32Hub {
    fn name(&self) -> &str { "Arduino32-Hub" }
    fn description(&self) -> &str { "Arduino32 Hub (required)" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Step 1: identify board — send byte 30, expect "MM-Ard"
        let id_resp = self.call_transport(|t| {
            t.send("\x1e")?;   // 0x1e = 30
            t.receive_line()
        })?;

        if id_resp.trim() != "MM-Ard" {
            return Err(MmError::LocallyDefined(
                "Arduino32 board not found or wrong firmware".into(),
            ));
        }

        // Step 2: query version — send byte 31, expect integer string
        let ver_resp = self.call_transport(|t| {
            t.send("\x1f")?;   // 0x1f = 31
            t.receive_line()
        })?;

        let ver: i32 = ver_resp.trim().parse().map_err(|_| {
            MmError::LocallyDefined("Could not parse firmware version".into())
        })?;

        if ver < FIRMWARE_MIN || ver > FIRMWARE_MAX {
            return Err(MmError::LocallyDefined(format!(
                "Firmware version {} not supported (expected {}-{})",
                ver, FIRMWARE_MIN, FIRMWARE_MAX
            )));
        }

        self.firmware_version = ver;
        self.props.entry_mut("Version")
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

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Hub }
    fn busy(&self) -> bool { false }
}

impl Hub for Arduino32Hub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        Ok(vec![
            "Arduino32-Shutter".into(),
            "Arduino32-Switch".into(),
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
    use micromanager::transport::MockTransport;

    fn make_hub() -> Arduino32Hub {
        let t = MockTransport::new()
            .expect("\x1e", "MM-Ard")   // id query
            .expect("\x1f", "3");        // version query
        Arduino32Hub::new().with_transport(Box::new(t))
    }

    #[test]
    fn initialize_ok() {
        let mut hub = make_hub();
        hub.initialize().unwrap();
        assert_eq!(hub.firmware_version(), 3);
    }

    #[test]
    fn bad_id_rejected() {
        let t = MockTransport::new().any("WrongBoard").any("3");
        let mut hub = Arduino32Hub::new().with_transport(Box::new(t));
        assert!(hub.initialize().is_err());
    }
}
