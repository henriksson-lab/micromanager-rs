/// NeoPixel LED shutter adapter for Arduino + Adafruit NeoPixel.
///
/// Binary protocol (no text), single-byte commands sent over serial.
///
/// Command bytes (from NeoPixelFirmware.ino):
///   0x01 — Open (all pixels on)   → device echoes back 0x01
///   0x02 — Close (all pixels off) → device echoes back 0x02
///   0x07 r g b — Set colour        → device echoes back 0x07
///   0x1E (30) — Query firmware name → responds "MM-NeoPixel\r\n" (text)
///   0x1F (31) — Query firmware version → responds version number text + \r\n
///   0x20 (32) — Query num rows  → single byte response
///   0x21 (33) — Query num cols  → single byte response
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct NeoPixelShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    version: i64,
    color: String,
    active_state: String,
    num_rows: u8,
    num_columns: u8,
}

impl NeoPixelShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Version", PropertyValue::Integer(0), true)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            version: 0,
            color: "Blue".into(),
            active_state: "None".into(),
            num_rows: 1,
            num_columns: 1,
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

    fn receive_one_byte(&mut self, command: u8) -> MmResult<u8> {
        let bytes = self.call_transport(|t| {
            t.send_bytes(&[command])?;
            t.receive_bytes(1)
        })?;
        bytes.first().copied().ok_or(MmError::SerialInvalidResponse)
    }

    fn send_ack_command(&mut self, command: &[u8]) -> MmResult<()> {
        let expected = command[0];
        self.call_transport(|t| {
            t.send_bytes(command)?;
            let ack = t.receive_bytes(1)?;
            if ack.first().copied() == Some(expected) {
                Ok(())
            } else {
                Err(MmError::SerialInvalidResponse)
            }
        })
    }

    fn define_initialized_properties(&mut self) -> MmResult<()> {
        if !self.props.has_property("NeoPixelColor") {
            self.props.define_property(
                "NeoPixelColor",
                PropertyValue::String(self.color.clone()),
                false,
            )?;
            self.props
                .set_allowed_values("NeoPixelColor", &["Red", "Green", "Blue"])?;
        }
        if !self.props.has_property("AllPixelsActive") {
            self.props.define_property(
                "AllPixelsActive",
                PropertyValue::String(self.active_state.clone()),
                false,
            )?;
            self.props
                .set_allowed_values("AllPixelsActive", &["All", "None", "Some"])?;
        }
        if !self.props.has_property("OnOff") {
            self.props
                .define_property("OnOff", PropertyValue::String("Off".into()), false)?;
            self.props.set_allowed_values("OnOff", &["On", "Off"])?;
        }
        Ok(())
    }
}

impl Default for NeoPixelShutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for NeoPixelShutter {
    fn name(&self) -> &str {
        "NeoPixel-Shutter"
    }
    fn description(&self) -> &str {
        "Arduino NeoPixel LED shutter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Send firmware-name query (0x1E) and check response
        let name = self.call_transport(|t| {
            t.send_bytes(&[0x1E])?;
            t.receive_line()
        })?;
        if name.trim() != "MM-NeoPixel" {
            return Err(MmError::NotConnected);
        }
        let version = self.call_transport(|t| {
            t.send_bytes(&[0x1F])?;
            t.receive_line()
        })?;
        let version = version
            .trim()
            .parse::<i64>()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        if version != 1 {
            return Err(MmError::NotConnected);
        }
        self.version = version;
        self.props
            .entry_mut("Version")
            .map(|e| e.value = PropertyValue::Integer(version));
        self.num_rows = self.receive_one_byte(0x20)?;
        self.num_columns = self.receive_one_byte(0x21)?;
        self.define_initialized_properties()?;
        self.is_open = false;
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
            "OnOff" => Ok(PropertyValue::String(
                if self.is_open { "On" } else { "Off" }.into(),
            )),
            "NeoPixelColor" => Ok(PropertyValue::String(self.color.clone())),
            "AllPixelsActive" => Ok(PropertyValue::String(self.active_state.clone())),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "OnOff" => match val.as_str() {
                "On" => self.set_open(true),
                "Off" => self.set_open(false),
                _ => Err(MmError::InvalidPropertyValue),
            },
            "NeoPixelColor" => {
                let color = val.as_str();
                let mut command = [0x07, 0, 0, 0];
                match color {
                    "Red" => command[1] = 255,
                    "Green" => command[2] = 255,
                    "Blue" => command[3] = 255,
                    _ => return Err(MmError::InvalidPropertyValue),
                }
                self.color = color.to_string();
                self.send_ack_command(&command)
            }
            "AllPixelsActive" => {
                let state = val.as_str();
                let command = match state {
                    "All" => 0x03,
                    "None" => 0x04,
                    "Some" => return Ok(()),
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.send_ack_command(&[command])
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

impl Shutter for NeoPixelShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open { 0x01u8 } else { 0x02u8 };
        self.send_ack_command(&[cmd])?;
        self.is_open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.is_open)
    }

    fn fire(&mut self, _dt: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_initialized_shutter() -> NeoPixelShutter {
        // init sequence: send 0x1E -> name, send 0x1F -> firmware API version.
        let t = MockTransport::new()
            .any("MM-NeoPixel")
            .any("1")
            .expect_binary(&[2])
            .expect_binary(&[3]);
        let mut s = NeoPixelShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s
    }

    #[test]
    fn initialize_does_not_send_close() {
        let s = make_initialized_shutter();
        assert!(s.initialized);
        assert!(!s.get_open().unwrap());
        assert_eq!(
            s.get_property("Version").unwrap(),
            PropertyValue::Integer(1)
        );
        assert_eq!(s.num_rows, 2);
        assert_eq!(s.num_columns, 3);
        assert_eq!(
            s.get_property("NeoPixelColor").unwrap(),
            PropertyValue::String("Blue".into())
        );
        assert_eq!(
            s.get_property("AllPixelsActive").unwrap(),
            PropertyValue::String("None".into())
        );
        assert_eq!(
            s.get_property("OnOff").unwrap(),
            PropertyValue::String("Off".into())
        );
    }

    #[test]
    fn set_open_true() {
        let mut s = make_initialized_shutter();
        // replace transport with one that acks the open command
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0x01])));
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn set_open_false() {
        let mut s = make_initialized_shutter();
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0x02])));
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn wrong_ack_gives_error() {
        let mut s = make_initialized_shutter();
        // ack byte is 0xFF instead of 0x01
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0xFF])));
        assert!(s.set_open(true).is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(NeoPixelShutter::new().initialize().is_err());
    }

    #[test]
    fn wrong_firmware_name_error() {
        let t = MockTransport::new().any("WRONG-DEVICE").any("1");
        let mut s = NeoPixelShutter::new().with_transport(Box::new(t));
        assert!(s.initialize().is_err());
    }

    #[test]
    fn wrong_firmware_version_error() {
        let t = MockTransport::new().any("MM-NeoPixel").any("2");
        let mut s = NeoPixelShutter::new().with_transport(Box::new(t));
        assert!(s.initialize().is_err());
        assert!(!s.initialized);
    }

    #[test]
    fn missing_dimensions_error() {
        let t = MockTransport::new().any("MM-NeoPixel").any("1");
        let mut s = NeoPixelShutter::new().with_transport(Box::new(t));
        assert_eq!(s.initialize(), Err(MmError::SerialTimeout));
        assert!(!s.initialized);
    }

    #[test]
    fn color_property_sends_upstream_rgb_command() {
        let mut s = make_initialized_shutter();
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0x07])));
        s.set_property("NeoPixelColor", PropertyValue::String("Red".into()))
            .unwrap();
        assert_eq!(
            s.get_property("NeoPixelColor").unwrap(),
            PropertyValue::String("Red".into())
        );
    }

    #[test]
    fn onoff_property_routes_to_shutter_command() {
        let mut s = make_initialized_shutter();
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0x01])));
        s.set_property("OnOff", PropertyValue::String("On".into()))
            .unwrap();
        assert!(s.get_open().unwrap());
        assert_eq!(
            s.get_property("OnOff").unwrap(),
            PropertyValue::String("On".into())
        );
    }

    #[test]
    fn all_pixels_active_sends_all_and_none_commands() {
        let mut s = make_initialized_shutter();
        s.transport = Some(Box::new(
            MockTransport::new()
                .expect_binary(&[0x03])
                .expect_binary(&[0x04]),
        ));
        s.set_property("AllPixelsActive", PropertyValue::String("All".into()))
            .unwrap();
        s.set_property("AllPixelsActive", PropertyValue::String("None".into()))
            .unwrap();
        assert_eq!(
            s.get_property("AllPixelsActive").unwrap(),
            PropertyValue::String("None".into())
        );
    }

    #[test]
    fn fire_is_unsupported_like_upstream() {
        let mut s = make_initialized_shutter();
        assert_eq!(s.fire(1.0), Err(MmError::UnsupportedCommand));
    }
}
