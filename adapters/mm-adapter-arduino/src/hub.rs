/// ArduinoHub — manages the serial port and shared state (switch + shutter bits).
///
/// Binary protocol:
/// - Send byte `30` → response "MM-Ard\r\n" + optional extended version byte
/// - Switch command: `[1, state_lo, state_hi]` → response `[1]`
/// - DA command:     `[3, channel-1, hi_byte, lo_byte]` → response `[3]`
use parking_lot::Mutex;
use std::sync::Arc;

use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Hub};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

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
    pub shared: Arc<Mutex<HubState>>,
    inverted_logic: bool,
}

impl ArduinoHub {
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

    /// Send the switch state (16-bit) to the Arduino.
    pub fn write_switch_state(&mut self, state: u16) -> MmResult<()> {
        let lo = (state & 0xFF) as u8;
        let hi = ((state >> 8) & 0xFF) as u8;
        let cmd = format!("\x01{}{}", lo as char, hi as char);
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

    /// Send a DA value (0–4095) to a channel (1-based).
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

    pub fn firmware_version(&self) -> u8 {
        self.firmware_version
    }
}

impl Default for ArduinoHub {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ArduinoHub {
    fn name(&self) -> &str { "Arduino-Hub" }
    fn description(&self) -> &str { "Arduino Hub (required)" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Send version query byte (30) and read greeting "MM-Ard-N\r\n"
        let resp = self.call_transport(|t| {
            t.send("\x1e")?; // 0x1e = 30
            t.receive_line()
        })?;

        if !resp.starts_with("MM-Ard") {
            return Err(MmError::LocallyDefined(
                "Arduino board not found or wrong firmware".into(),
            ));
        }

        // Parse version number from response "MM-Ard\r\n" + possible ext version
        // Basic parse: last token after '-' is version digit
        let ver: u8 = resp
            .split('-')
            .last()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);

        if ver < FIRMWARE_MIN || ver > FIRMWARE_MAX {
            return Err(MmError::LocallyDefined(format!(
                "Firmware version {} not supported (expected {}-{})",
                ver, FIRMWARE_MIN, FIRMWARE_MAX
            )));
        }

        self.firmware_version = ver;
        self.props.entry_mut("Version")
            .map(|e| e.value = PropertyValue::Integer(ver as i64));

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

    fn device_type(&self) -> DeviceType { DeviceType::Hub }
    fn busy(&self) -> bool { false }
}

impl Hub for ArduinoHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        Ok(vec![
            "Arduino-Shutter".to_string(),
            "Arduino-Switch".to_string(),
            "Arduino-DAC1".to_string(),
            "Arduino-DAC2".to_string(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn make_hub() -> ArduinoHub {
        let transport = MockTransport::new()
            .any("MM-Ard-2"); // firmware v2 response to byte 30
        ArduinoHub::new().with_transport(Box::new(transport))
    }

    #[test]
    fn initialize_ok() {
        let mut hub = make_hub();
        hub.initialize().unwrap();
        assert_eq!(hub.firmware_version(), 2);
    }

    #[test]
    fn bad_firmware_rejected() {
        let transport = MockTransport::new().any("WrongDevice");
        let mut hub = ArduinoHub::new().with_transport(Box::new(transport));
        assert!(hub.initialize().is_err());
    }
}
