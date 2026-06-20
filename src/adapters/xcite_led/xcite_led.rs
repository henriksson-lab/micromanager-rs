/// X-Cite LED (XLED1) shutter adapter.
///
/// Serial ASCII protocol with binary terminator 0x0D (`\r`).
///
/// The XLED1 controller uses 2-character ASCII command codes:
///   `sn?\r`  — get serial number
///   `us?\r`  — get unit status
///   `on=1N\r` — turn LED N on  (N=1..4 as ASCII digit offset from '1')
///   `on=0N\r` — turn LED N off
///   `on?\r`  — query LED on/off state
///   `ip=NNN\r` — set intensity (0-100)
///
/// The XLedSerialIO in the C++ source sends `[cmd0, cmd1, '?', TxTerm]`
/// for queries and `[cmd0, cmd1, '=', value, TxTerm]` for sets, where
/// TxTerm is 0x0D ('\r').
///
/// This adapter models a single LED channel as a Shutter device.
/// LED device number is 0-based (matches the C++ `m_nLedDevNumber`).
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct XCiteLedShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    /// 0-based LED device number (0 = LED1, 1 = LED2, …)
    led_number: u8,
    /// Intensity 0–100 %
    intensity: u32,
}

impl XCiteLedShutter {
    pub fn new(led_number: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Intensity", PropertyValue::Integer(50), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            led_number,
            intensity: 50,
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

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }

    /// Turn this LED on or off.  Command: `on=<1|0><N+1>\r`
    /// where N is the 0-based led_number; the device uses 1-based channel
    /// encoded as ASCII digit after the value byte.
    fn set_led_on_off(&mut self, on: bool) -> MmResult<()> {
        // C++: sCmdSet = {0x6F, 0x6E, 0x3D, 0x31/0x66, TxTerm} then adjust
        // sCmdSet[1] = 0x6E ('n') if on, 0x66 ('f') if off
        // sCmdSet[3] += led_number
        // Decoding: cmd = "on=1N\r" for on, "of=0N\r" ... actually looking at
        // the bytes: 0x6F=o 0x6E=n 0x3D== 0x31='1' → "on=1" for on
        //            0x6F=o 0x66=f 0x3D== 0x31='1' → "of=1" (but 0x66='f')
        // Wait: 0x6E='n' for on, 0x66='f' for off, so:
        //   on:  "on=1N" where N = 0x31+led_number
        //   off: "of=1N"  (of=1<N>) -- but that seems wrong for "off"
        // More careful re-reading: sCmdSet[1]=(on)?0x6E:0x66  so
        //   on:  bytes [0x6F, 0x6E, 0x3D, 0x31+dev, TxTerm] = "on=<'1'+dev>\r"
        //   off: bytes [0x6F, 0x66, 0x3D, 0x31+dev, TxTerm] = "of=<'1'+dev>\r"
        // The digit '1'+dev encodes channel number (not a boolean).
        let second = if on { 'n' } else { 'f' };
        let channel_char = (b'1' + self.led_number) as char;
        let cmd = format!("o{}={}", second, channel_char);
        self.cmd(&cmd)?;
        Ok(())
    }

    pub fn set_intensity(&mut self, percent: u32) -> MmResult<()> {
        // C++ XLedDev::OnLedIntensity converts percent to tenths and selects
        // channels by comma-padding before the value: LED0 -> ip=750,
        // LED1 -> ip=,750, etc.
        let mut tenths = percent.saturating_mul(10);
        if tenths > 0 && tenths < 50 {
            tenths = 50;
        }
        tenths = tenths.min(1000);
        let cmd = format!("ip={}{}", ",".repeat(self.led_number as usize), tenths);
        self.cmd(&cmd)?;
        self.intensity = percent;
        Ok(())
    }
}

impl Default for XCiteLedShutter {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Device for XCiteLedShutter {
    fn name(&self) -> &str {
        "XCite-LED-Shutter"
    }
    fn description(&self) -> &str {
        "X-Cite XLED1 LED shutter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Query serial number to verify connection: "sn?\r"
        self.cmd("sn?")?;
        // Turn LED off at init
        self.set_led_on_off(false)?;
        self.is_open = false;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.set_led_on_off(false);
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Intensity" => {
                let percent = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if percent < 0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.set_intensity(percent as u32)?;
                self.props
                    .set(name, PropertyValue::Integer(self.intensity as i64))
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

impl Shutter for XCiteLedShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.set_led_on_off(open)?;
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

    fn make_initialized() -> XCiteLedShutter {
        // init: "sn?\r" → any, "of=1\r" → any (LED0 = channel '1')
        let t = MockTransport::new().any("SN12345").any("ok");
        let mut s = XCiteLedShutter::new(0).with_transport(Box::new(t));
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
        s.transport = Some(Box::new(MockTransport::new().expect("on=1\r", "ok")));
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn set_open_false() {
        let mut s = make_initialized();
        s.transport = Some(Box::new(MockTransport::new().expect("of=1\r", "ok")));
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn led_number_2_channel_char() {
        let s = XCiteLedShutter::new(2);
        // LED 2 → channel char '3' (b'1' + 2 = b'3')
        assert_eq!((b'1' + s.led_number) as char, '3');
    }

    #[test]
    fn fire_is_unsupported() {
        let mut s = make_initialized();
        assert_eq!(s.fire(1.0), Err(MmError::UnsupportedCommand));
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn set_intensity() {
        let mut s = make_initialized();
        s.transport = Some(Box::new(MockTransport::new().expect("ip=750\r", "ok")));
        s.set_intensity(75).unwrap();
        assert_eq!(s.intensity, 75);
    }

    #[test]
    fn intensity_property_sends_command() {
        let mut s = make_initialized();
        s.transport = Some(Box::new(MockTransport::new().expect("ip=750\r", "ok")));
        s.set_property("Intensity", PropertyValue::Integer(75))
            .unwrap();
        assert_eq!(
            s.get_property("Intensity").unwrap(),
            PropertyValue::Integer(75)
        );
    }

    #[test]
    fn no_transport_error() {
        assert!(XCiteLedShutter::new(0).initialize().is_err());
    }
}
