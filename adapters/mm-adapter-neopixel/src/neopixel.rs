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
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct NeoPixelShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
}

impl NeoPixelShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
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
        // Close shutter on init; device echoes back 0x02
        self.call_transport(|t| {
            t.send_bytes(&[0x02])?;
            let ack = t.receive_bytes(1)?;
            if ack.first().copied() == Some(0x02) {
                Ok(())
            } else {
                Err(MmError::SerialInvalidResponse)
            }
        })?;
        self.is_open = false;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.call_transport(|t| {
                t.send_bytes(&[0x02])?;
                let _ = t.receive_bytes(1);
                Ok(())
            });
            self.is_open = false;
            self.initialized = false;
        }
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
        DeviceType::Shutter
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Shutter for NeoPixelShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open { 0x01u8 } else { 0x02u8 };
        self.call_transport(|t| {
            t.send_bytes(&[cmd])?;
            let ack = t.receive_bytes(1)?;
            if ack.first().copied() == Some(cmd) {
                Ok(())
            } else {
                Err(MmError::SerialInvalidResponse)
            }
        })?;
        self.is_open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.is_open)
    }

    fn fire(&mut self, _dt: f64) -> MmResult<()> {
        self.set_open(true)?;
        self.set_open(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_initialized_shutter() -> NeoPixelShutter {
        // init sequence: send 0x1E → receive_line "MM-NeoPixel"
        //                send 0x02 → receive_bytes [0x02]
        let t = MockTransport::new()
            .any("MM-NeoPixel")         // receive_line response for firmware name
            .expect_binary(&[0x02]);    // receive_bytes ack for close
        let mut s = NeoPixelShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s
    }

    #[test]
    fn initialize_and_close() {
        let s = make_initialized_shutter();
        assert!(s.initialized);
        assert!(!s.get_open().unwrap());
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
        let t = MockTransport::new().any("WRONG-DEVICE");
        let mut s = NeoPixelShutter::new().with_transport(Box::new(t));
        assert!(s.initialize().is_err());
    }
}
