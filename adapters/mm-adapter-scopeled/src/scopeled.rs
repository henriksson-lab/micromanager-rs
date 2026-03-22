/// ScopeLED illuminator shutter adapter.
///
/// The original C++ adapter communicates over USB HID using a proprietary
/// USBCommAdapter library (not serial).  This Rust port implements the shutter
/// state-machine over the abstract Transport layer, using a simplified command
/// packet format derived from the C++ source:
///
/// Packet layout (host → device, 64 bytes total):
///   byte 0: message ID
///   byte 1: command byte
///   remaining bytes: payload
///
/// Message IDs used here:
///   0x01 — Set illumination on/off:  [0x01, state(0/1)]
///   0x04 — Set channel intensity:    [0x04, channel, intensity_byte]
///
/// Responses are 64-byte packets; first byte echoes message ID on success.
///
/// Because the original adapter is USB-HID and not RS-232, the Transport
/// abstraction is used for testability only.  In production a real USB
/// HID transport would be injected.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const NUM_CHANNELS: usize = 4;

pub struct ScopeLedShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    /// Per-channel intensity 0–100.
    intensities: [u8; NUM_CHANNELS],
}

impl ScopeLedShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        for ch in 0..NUM_CHANNELS {
            props
                .define_property(
                    &format!("Channel{}Intensity", ch + 1),
                    PropertyValue::Integer(0),
                    false,
                )
                .unwrap();
        }
        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            intensities: [0u8; NUM_CHANNELS],
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

    /// Send a 2-byte command and check the 1-byte ack.
    fn send_cmd(&mut self, msg_id: u8, payload: u8) -> MmResult<()> {
        self.call_transport(|t| {
            t.send_bytes(&[msg_id, payload])?;
            let ack = t.receive_bytes(1)?;
            if ack.first().copied() == Some(msg_id) {
                Ok(())
            } else {
                Err(MmError::SerialInvalidResponse)
            }
        })
    }

    pub fn set_channel_intensity(&mut self, channel: usize, intensity: u8) -> MmResult<()> {
        if channel >= NUM_CHANNELS {
            return Err(MmError::InvalidInputParam);
        }
        self.call_transport(|t| {
            t.send_bytes(&[0x04, channel as u8, intensity])?;
            let ack = t.receive_bytes(1)?;
            if ack.first().copied() == Some(0x04) {
                Ok(())
            } else {
                Err(MmError::SerialInvalidResponse)
            }
        })?;
        self.intensities[channel] = intensity;
        Ok(())
    }
}

impl Default for ScopeLedShutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ScopeLedShutter {
    fn name(&self) -> &str {
        "ScopeLED-Shutter"
    }
    fn description(&self) -> &str {
        "ScopeLED fluorescence illuminator shutter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Turn off all channels at init
        self.send_cmd(0x01, 0x00)?;
        self.is_open = false;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send_cmd(0x01, 0x00);
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

impl Shutter for ScopeLedShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let state = if open { 0x01u8 } else { 0x00u8 };
        self.send_cmd(0x01, state)?;
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

    fn make_initialized() -> ScopeLedShutter {
        // init sends [0x01, 0x00], expects ack [0x01]
        let t = MockTransport::new().expect_binary(&[0x01]);
        let mut s = ScopeLedShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s
    }

    #[test]
    fn initialize_succeeds() {
        let s = make_initialized();
        assert!(s.initialized);
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn set_open_true() {
        let mut s = make_initialized();
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0x01])));
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn set_open_false() {
        let mut s = make_initialized();
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0x01])));
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn fire_opens_then_closes() {
        let mut s = make_initialized();
        s.transport = Some(Box::new(
            MockTransport::new()
                .expect_binary(&[0x01]) // open ack
                .expect_binary(&[0x01]), // close ack
        ));
        s.fire(10.0).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() {
        assert!(ScopeLedShutter::new().initialize().is_err());
    }

    #[test]
    fn bad_ack_error() {
        let mut s = make_initialized();
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0xFF])));
        assert!(s.set_open(true).is_err());
    }
}
