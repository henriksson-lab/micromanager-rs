/// Thorlabs CHROLIS 6-channel LED illuminator adapter.
///
/// The original C++ adapter uses the proprietary TL6WL NI-VISA library
/// (ThorlabsChrolisDeviceWrapper).  This Rust port models the same device
/// behaviour over the abstract Transport layer, using a USB-style packet
/// protocol derived from the C++ wrapper interface:
///
/// Binary packet protocol (host → device):
///   Byte 0: command ID
///   Bytes 1…n: payload
///
/// Command IDs:
///   0x10 — Set master shutter state:  [0x10, 0|1]
///   0x11 — Get shutter state:         [0x11]       → [0x11, 0|1]
///   0x20 — Set LED enable states:     [0x20, bitmask_byte]
///   0x21 — Get LED enable states:     [0x21]       → [0x21, bitmask_byte]
///   0x30 — Set LED brightness(ch,val):[0x30, ch, hi, lo] (u16 big-endian)
///   0x31 — Get LED brightness(ch):    [0x31, ch]   → [0x31, hi, lo]
///
/// NUM_LEDS = 6 channels (index 0…5).
///
/// Note: the original adapter also exports a ChrolisHub and ChrolisStateDevice;
/// this adapter focuses on ChrolisShutter (master on/off) and the LED control
/// device (per-channel enable + brightness).
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub const NUM_LEDS: usize = 6;

/// Master shutter (on/off toggle for all LEDs).
pub struct ChrolisShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
}

impl ChrolisShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("SerialNumber", PropertyValue::String("".into()), false)
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

    fn set_shutter_state(&mut self, open: bool) -> MmResult<()> {
        let state = if open { 0x01u8 } else { 0x00u8 };
        self.call_transport(|t| {
            t.send_bytes(&[0x10, state])?;
            let ack = t.receive_bytes(1)?;
            if ack.first().copied() == Some(0x10) {
                Ok(())
            } else {
                Err(MmError::SerialInvalidResponse)
            }
        })
    }
}

impl Default for ChrolisShutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ChrolisShutter {
    fn name(&self) -> &str {
        "CHROLIS_Shutter"
    }
    fn description(&self) -> &str {
        "Thorlabs CHROLIS master shutter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Query current shutter state
        self.call_transport(|t| {
            t.send_bytes(&[0x11])?;
            let resp = t.receive_bytes(2)?;
            if resp.first().copied() == Some(0x11) {
                Ok(())
            } else {
                Err(MmError::SerialInvalidResponse)
            }
        })?;
        // Close shutter at init
        self.set_shutter_state(false)?;
        self.is_open = false;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.set_shutter_state(false);
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

impl Shutter for ChrolisShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.set_shutter_state(open)?;
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

/// Per-channel LED control device.
pub struct ChrolisLed {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    /// Per-channel enable states.
    enabled: [bool; NUM_LEDS],
    /// Per-channel brightness 0–1000.
    brightness: [u16; NUM_LEDS],
}

impl ChrolisLed {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        for i in 0..NUM_LEDS {
            props
                .define_property(
                    &format!("LED{}Enable", i + 1),
                    PropertyValue::Integer(0),
                    false,
                )
                .unwrap();
            props
                .define_property(
                    &format!("LED{}Brightness", i + 1),
                    PropertyValue::Integer(0),
                    false,
                )
                .unwrap();
        }
        Self {
            props,
            transport: None,
            initialized: false,
            enabled: [false; NUM_LEDS],
            brightness: [0u16; NUM_LEDS],
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

    /// Encode all enable states into a bitmask (bit 0 = LED 0, etc.).
    fn encode_enables(&self) -> u8 {
        let mut mask = 0u8;
        for (i, &en) in self.enabled.iter().enumerate() {
            if en {
                mask |= 1 << i;
            }
        }
        mask
    }

    /// Set all LED enable states via bitmask.
    pub fn set_enable_states(&mut self, mask: u8) -> MmResult<()> {
        self.call_transport(|t| {
            t.send_bytes(&[0x20, mask])?;
            let ack = t.receive_bytes(1)?;
            if ack.first().copied() == Some(0x20) {
                Ok(())
            } else {
                Err(MmError::SerialInvalidResponse)
            }
        })?;
        for i in 0..NUM_LEDS {
            self.enabled[i] = (mask & (1 << i)) != 0;
        }
        Ok(())
    }

    /// Set a single LED's enable state.
    pub fn set_led_enable(&mut self, led: usize, enable: bool) -> MmResult<()> {
        if led >= NUM_LEDS {
            return Err(MmError::InvalidInputParam);
        }
        self.enabled[led] = enable;
        let mask = self.encode_enables();
        self.set_enable_states(mask)
    }

    /// Set a single LED's brightness (0–1000).
    pub fn set_brightness(&mut self, led: usize, value: u16) -> MmResult<()> {
        if led >= NUM_LEDS {
            return Err(MmError::InvalidInputParam);
        }
        let hi = (value >> 8) as u8;
        let lo = (value & 0xFF) as u8;
        self.call_transport(|t| {
            t.send_bytes(&[0x30, led as u8, hi, lo])?;
            let ack = t.receive_bytes(1)?;
            if ack.first().copied() == Some(0x30) {
                Ok(())
            } else {
                Err(MmError::SerialInvalidResponse)
            }
        })?;
        self.brightness[led] = value;
        Ok(())
    }
}

impl Default for ChrolisLed {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ChrolisLed {
    fn name(&self) -> &str {
        "CHROLIS_LED_Control"
    }
    fn description(&self) -> &str {
        "Thorlabs CHROLIS per-channel LED control"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Get initial enable states
        self.call_transport(|t| {
            t.send_bytes(&[0x21])?;
            let resp = t.receive_bytes(2)?;
            if resp.first().copied() == Some(0x21) {
                Ok(())
            } else {
                Err(MmError::SerialInvalidResponse)
            }
        })?;
        // Disable all LEDs at init
        self.set_enable_states(0x00)?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.set_enable_states(0x00);
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
        DeviceType::Generic
    }
    fn busy(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_shutter() -> ChrolisShutter {
        // init: query state [0x11] → [0x11, 0], then close [0x10, 0x00] → [0x10]
        let t = MockTransport::new()
            .expect_binary(&[0x11, 0x00]) // get state response
            .expect_binary(&[0x10]);       // close ack
        let mut s = ChrolisShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s
    }

    #[test]
    fn shutter_initialize() {
        let s = make_shutter();
        assert!(s.initialized);
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn shutter_set_open() {
        let mut s = make_shutter();
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0x10])));
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn shutter_fire() {
        let mut s = make_shutter();
        s.transport = Some(Box::new(
            MockTransport::new()
                .expect_binary(&[0x10])
                .expect_binary(&[0x10]),
        ));
        s.fire(1.0).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn shutter_no_transport() {
        assert!(ChrolisShutter::new().initialize().is_err());
    }

    fn make_led() -> ChrolisLed {
        // init: get enables [0x21] → [0x21, 0xFF], then disable all [0x20, 0x00] → [0x20]
        let t = MockTransport::new()
            .expect_binary(&[0x21, 0xFF])
            .expect_binary(&[0x20]);
        let mut s = ChrolisLed::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s
    }

    #[test]
    fn led_initialize() {
        let s = make_led();
        assert!(s.initialized);
        assert_eq!(s.encode_enables(), 0x00);
    }

    #[test]
    fn led_enable_single() {
        let mut s = make_led();
        // enable LED 0: mask = 0x01 → ack 0x20
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0x20])));
        s.set_led_enable(0, true).unwrap();
        assert!(s.enabled[0]);
        assert_eq!(s.encode_enables(), 0x01);
    }

    #[test]
    fn led_enable_mask() {
        let mut s = make_led();
        // enable LEDs 0 and 2: mask = 0b00000101 = 0x05
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0x20])));
        s.set_enable_states(0x05).unwrap();
        assert!(s.enabled[0]);
        assert!(!s.enabled[1]);
        assert!(s.enabled[2]);
    }

    #[test]
    fn led_brightness() {
        let mut s = make_led();
        // set LED 1 brightness to 500 (0x01F4): hi=0x01, lo=0xF4
        s.transport = Some(Box::new(MockTransport::new().expect_binary(&[0x30])));
        s.set_brightness(1, 500).unwrap();
        assert_eq!(s.brightness[1], 500);
    }

    #[test]
    fn led_out_of_range() {
        let mut s = make_led();
        s.transport = Some(Box::new(MockTransport::new()));
        assert!(s.set_led_enable(NUM_LEDS, true).is_err());
    }
}
