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
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

const NUM_CHANNELS: usize = 4;
const DICON_USB_VID_NEW_STR: &str = "9410";
const DICON_USB_VID_OLD: &str = "49745";
const SCOPELED_F_PRODUCT_ID: i64 = 0x1305;

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
            .define_pre_init_property("InitSerialNumber", PropertyValue::String(String::new()))
            .unwrap();
        props
            .define_pre_init_property(
                "VendorID",
                PropertyValue::String(DICON_USB_VID_NEW_STR.to_string()),
            )
            .unwrap();
        props
            .set_allowed_values("VendorID", &[DICON_USB_VID_NEW_STR, DICON_USB_VID_OLD])
            .unwrap();
        props
            .define_pre_init_property("ProductID", PropertyValue::Integer(SCOPELED_F_PRODUCT_ID))
            .unwrap();
        props
            .set_property_limits("ProductID", 0.0, 65535.0)
            .unwrap();
        props
            .define_property("LastError", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("LastDeviceResult", PropertyValue::Integer(0), true)
            .unwrap();
        for ch in 0..NUM_CHANNELS {
            let prop = format!("Channel{}Intensity", ch + 1);
            props
                .define_property(&prop, PropertyValue::Integer(0), false)
                .unwrap();
            props.set_property_limits(&prop, 0.0, 100.0).unwrap();
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
        if intensity > 100 {
            return Err(MmError::InvalidPropertyValue);
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

    fn channel_intensity_property(name: &str) -> Option<usize> {
        let suffix = name.strip_prefix("Channel")?.strip_suffix("Intensity")?;
        let channel = suffix.parse::<usize>().ok()?;
        (1..=NUM_CHANNELS).contains(&channel).then_some(channel - 1)
    }
}

impl Default for ScopeLedShutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ScopeLedShutter {
    fn name(&self) -> &str {
        "ScopeLED-F"
    }
    fn description(&self) -> &str {
        "ScopeLED Fluorescence Microscope Illuminator"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
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
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if let Some(channel) = Self::channel_intensity_property(name) {
            let intensity = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            if !(0..=100).contains(&intensity) {
                return Err(MmError::InvalidPropertyValue);
            }
            self.set_channel_intensity(channel, intensity as u8)?;
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
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_initialized() -> ScopeLedShutter {
        let t = MockTransport::new();
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
    fn initialize_does_not_force_close() {
        let mut s = ScopeLedShutter::new().with_transport(Box::new(MockTransport::new()));
        s.is_open = true;
        s.initialize().unwrap();
        assert!(s.get_open().unwrap());
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
    fn fire_is_unsupported() {
        let mut s = make_initialized();
        assert_eq!(s.fire(10.0).unwrap_err(), MmError::UnsupportedCommand);
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() {
        assert!(ScopeLedShutter::new().initialize().is_err());
    }

    #[test]
    fn upstream_pre_init_usb_properties_are_exposed() {
        let s = ScopeLedShutter::new();
        assert_eq!(
            s.get_property("InitSerialNumber").unwrap(),
            PropertyValue::String(String::new())
        );
        assert_eq!(
            s.get_property("VendorID").unwrap(),
            PropertyValue::String("9410".into())
        );
        assert_eq!(
            s.get_property("ProductID").unwrap(),
            PropertyValue::Integer(4869)
        );
        assert!(s.is_property_read_only("LastError"));
        assert!(s.is_property_read_only("LastDeviceResult"));
        assert!(!s.has_property("Port"));
        assert_eq!(
            s.description(),
            "ScopeLED Fluorescence Microscope Illuminator"
        );
    }

    #[test]
    fn bad_ack_error() {
        let mut s = make_initialized();
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0xFF])));
        assert!(s.set_open(true).is_err());
    }

    #[test]
    fn channel_intensity_property_sends_hardware_command() {
        let mut s = make_initialized();
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0x04])));

        s.set_property("Channel2Intensity", PropertyValue::Integer(42))
            .unwrap();

        assert_eq!(s.intensities[1], 42);
        assert_eq!(
            s.get_property("Channel2Intensity").unwrap(),
            PropertyValue::Integer(42)
        );
    }

    #[test]
    fn channel_intensity_property_preserves_cache_on_bad_ack() {
        let mut s = make_initialized();
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0xFF])));

        assert_eq!(
            s.set_property("Channel2Intensity", PropertyValue::Integer(42))
                .unwrap_err(),
            MmError::SerialInvalidResponse
        );
        assert_eq!(s.intensities[1], 0);
        assert_eq!(
            s.get_property("Channel2Intensity").unwrap(),
            PropertyValue::Integer(0)
        );
    }

    #[test]
    fn channel_intensity_is_limited_to_percent_range() {
        let mut s = make_initialized();
        assert_eq!(
            s.set_property("Channel1Intensity", PropertyValue::Integer(101))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            s.set_channel_intensity(0, 101).unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn shutdown_does_not_force_close() {
        let mut s = make_initialized();
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0x01])));
        s.set_open(true).unwrap();
        s.shutdown().unwrap();
        assert!(s.get_open().unwrap());
        assert!(!s.initialized);
    }
}
