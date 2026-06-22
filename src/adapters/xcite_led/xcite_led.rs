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
use crate::traits::{Device, Generic, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};

pub const DEVICE_NAME_CONTROLLER: &str = "XLED1 Controller";
pub const DEVICE_NAME_LED1: &str = "LED1 Device";
pub const DEVICE_NAME_LED2: &str = "LED2 Device";
pub const DEVICE_NAME_LED3: &str = "LED3 Device";
pub const DEVICE_NAME_LED4: &str = "LED4 Device";

const LED_DEVICE_NAMES: [&str; 4] = [
    DEVICE_NAME_LED1,
    DEVICE_NAME_LED2,
    DEVICE_NAME_LED3,
    DEVICE_NAME_LED4,
];

const LED_METADATA_PROPERTIES: &[(&str, &str)] = &[
    ("L.00 Device ", "ln?"),
    ("L.02 Device Type", "lt?"),
    ("L.03 Serial Number", "ls?"),
    ("L.04 Manufacturing Date", "md?"),
    ("L.05 WaveLength", "lw?"),
    ("L.06 FWHM Value", "lf?"),
    ("L.08 Hours Elapsed", "lh?"),
    ("L.18 Current Temperature (Deg.C)", "gt?"),
    ("L.19 Max Allowed Temperature (Deg.C)", "mt?"),
    ("L.20 Min Allowed Temperature (Deg.C)", "nt?"),
    ("L.21 Temperature Hysteresis (Deg.C)", "th?"),
];

pub struct XCiteLedShutter {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    is_open: Cell<bool>,
    /// 0-based LED device number (0 = LED1, 1 = LED2, …)
    led_number: u8,
    /// Intensity 0–100 %
    intensity: Cell<u32>,
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
        props
            .define_property(
                "L.10 Intensity (0.0 or 5.0 - 100.0)%",
                PropertyValue::Integer(50),
                false,
            )
            .unwrap();
        props
            .define_property("SerialNumber", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("UnitStatus", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property(
                "LED-On-State",
                PropertyValue::String("Unknown".into()),
                true,
            )
            .unwrap();
        for (name, _) in LED_METADATA_PROPERTIES {
            props
                .define_property(*name, PropertyValue::String(String::new()), true)
                .unwrap();
        }
        Self {
            props,
            transport: None,
            initialized: false,
            is_open: Cell::new(false),
            led_number,
            intensity: Cell::new(50),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(RefCell::new(t));
        self
    }

    fn device_name(&self) -> &'static str {
        LED_DEVICE_NAMES
            .get(self.led_number as usize)
            .copied()
            .unwrap_or(DEVICE_NAME_LED1)
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_ref() {
            Some(t) => f(t.borrow_mut().as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }

    fn led_field(&self, response: &str) -> String {
        response
            .split(',')
            .nth(self.led_number as usize)
            .unwrap_or(response)
            .trim()
            .to_string()
    }

    fn read_led_field(&self, command: &str) -> MmResult<String> {
        let response = self.cmd(command)?;
        Ok(self.led_field(&response))
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

    fn live_open(&self) -> MmResult<bool> {
        let response = self.cmd("on?")?;
        let field = self.led_field(&response);
        let open = matches!(field.as_str(), "1" | "Y" | "y");
        self.is_open.set(open);
        Ok(open)
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
        self.intensity.set(percent);
        Ok(())
    }
}

pub struct XCiteLedController {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
}

impl XCiteLedController {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("SerialNumber", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("UnitStatus", PropertyValue::String(String::new()), true)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(RefCell::new(t));
        self
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        match self.transport.as_ref() {
            Some(t) => Ok(t.borrow_mut().send_recv(&full)?.trim().to_string()),
            None => Err(MmError::NotConnected),
        }
    }
}

impl Default for XCiteLedController {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for XCiteLedController {
    fn name(&self) -> &str {
        DEVICE_NAME_CONTROLLER
    }
    fn description(&self) -> &str {
        DEVICE_NAME_CONTROLLER
    }

    fn initialize(&mut self) -> MmResult<()> {
        let serial = self.cmd("sn?")?;
        let status = self.cmd("us?")?;
        self.props
            .entry_mut("SerialNumber")
            .map(|e| e.value = PropertyValue::String(serial));
        self.props
            .entry_mut("UnitStatus")
            .map(|e| e.value = PropertyValue::String(status));
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if self.initialized {
            match name {
                "SerialNumber" => return Ok(PropertyValue::String(self.cmd("sn?")?)),
                "UnitStatus" => return Ok(PropertyValue::String(self.cmd("us?")?)),
                _ => {}
            }
        }
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

impl Generic for XCiteLedController {}

impl Default for XCiteLedShutter {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Device for XCiteLedShutter {
    fn name(&self) -> &str {
        self.device_name()
    }
    fn description(&self) -> &str {
        self.device_name()
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Query serial number to verify connection: "sn?\r"
        let serial = self.cmd("sn?")?;
        self.props
            .entry_mut("SerialNumber")
            .map(|e| e.value = PropertyValue::String(serial));
        if let Ok(status) = self.cmd("us?") {
            self.props
                .entry_mut("UnitStatus")
                .map(|e| e.value = PropertyValue::String(status));
        }
        self.live_open()?;
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
        if self.initialized {
            match name {
                "SerialNumber" => return Ok(PropertyValue::String(self.cmd("sn?")?)),
                "UnitStatus" => return Ok(PropertyValue::String(self.cmd("us?")?)),
                "L.10 Intensity (0.0 or 5.0 - 100.0)%" => {
                    return Ok(PropertyValue::String(self.read_led_field("ip?")?))
                }
                "LED-On-State" => {
                    return Ok(PropertyValue::String(
                        if self.live_open()? { "On" } else { "Off" }.into(),
                    ))
                }
                _ => {
                    if let Some((_, command)) = LED_METADATA_PROPERTIES
                        .iter()
                        .find(|(prop_name, _)| *prop_name == name)
                    {
                        return Ok(PropertyValue::String(self.read_led_field(command)?));
                    }
                }
            }
        }
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Intensity" | "L.10 Intensity (0.0 or 5.0 - 100.0)%" => {
                let percent = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if percent < 0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.set_intensity(percent as u32)?;
                self.props.set(
                    "Intensity",
                    PropertyValue::Integer(self.intensity.get() as i64),
                )?;
                self.props.set(
                    "L.10 Intensity (0.0 or 5.0 - 100.0)%",
                    PropertyValue::Integer(self.intensity.get() as i64),
                )
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
        self.live_open()?;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        if self.initialized {
            self.live_open()
        } else {
            Ok(self.is_open.get())
        }
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
        let t = MockTransport::new().any("SN12345").any("0").any("0");
        let mut s = XCiteLedShutter::new(0).with_transport(Box::new(t));
        s.initialize().unwrap();
        s
    }

    #[test]
    fn initialize_succeeds() {
        let s = make_initialized();
        assert!(s.initialized);
        assert!(!s.is_open.get());
    }

    #[test]
    fn set_open_true() {
        let mut s = make_initialized();
        s.transport = Some(RefCell::new(Box::new(
            MockTransport::new()
                .expect("on=1\r", "ok")
                .expect("on?\r", "1")
                .expect("on?\r", "1"),
        )));
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn set_open_false() {
        let mut s = make_initialized();
        s.transport = Some(RefCell::new(Box::new(
            MockTransport::new()
                .expect("of=1\r", "ok")
                .expect("on?\r", "0")
                .expect("on?\r", "0"),
        )));
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
        assert!(!s.is_open.get());
    }

    #[test]
    fn set_intensity() {
        let mut s = make_initialized();
        s.transport = Some(RefCell::new(Box::new(
            MockTransport::new().expect("ip=750\r", "ok"),
        )));
        s.set_intensity(75).unwrap();
        assert_eq!(s.intensity.get(), 75);
    }

    #[test]
    fn intensity_property_sends_command() {
        let mut s = make_initialized();
        s.transport = Some(RefCell::new(Box::new(
            MockTransport::new().expect("ip=750\r", "ok"),
        )));
        s.set_property("Intensity", PropertyValue::Integer(75))
            .unwrap();
        assert_eq!(
            s.get_property("Intensity").unwrap(),
            PropertyValue::Integer(75)
        );
    }

    #[test]
    fn metadata_and_state_are_live_reads() {
        let t = MockTransport::new()
            .any("SN12345")
            .any("0")
            .any("0")
            .expect("sn?\r", "SN67890")
            .expect("us?\r", "READY")
            .expect("on?\r", "1");
        let mut s = XCiteLedShutter::new(0).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.get_property("SerialNumber").unwrap(),
            PropertyValue::String("SN67890".into())
        );
        assert_eq!(
            s.get_property("UnitStatus").unwrap(),
            PropertyValue::String("READY".into())
        );
        assert_eq!(
            s.get_property("LED-On-State").unwrap(),
            PropertyValue::String("On".into())
        );
    }

    #[test]
    fn upstream_led_metadata_uses_indexed_query_fields() {
        let t = MockTransport::new()
            .any("SN12345")
            .any("0")
            .any("0")
            .expect("lw?\r", "385,470,550,635")
            .expect("gt?\r", "31,32,33,34");
        let mut s = XCiteLedShutter::new(2).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.get_property("L.05 WaveLength").unwrap(),
            PropertyValue::String("550".into())
        );
        assert_eq!(
            s.get_property("L.18 Current Temperature (Deg.C)").unwrap(),
            PropertyValue::String("33".into())
        );
    }

    #[test]
    fn open_state_uses_indexed_query_field() {
        let t = MockTransport::new()
            .any("SN12345")
            .any("0")
            .any("0")
            .expect("on?\r", "0,1,0,0");
        let mut s = XCiteLedShutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() {
        assert!(XCiteLedShutter::new(0).initialize().is_err());
    }

    #[test]
    fn initialize_preserves_live_open_state() {
        let t = MockTransport::new()
            .expect("sn?\r", "SN12345")
            .expect("us?\r", "0")
            .expect("on?\r", "1");
        let mut s = XCiteLedShutter::new(0).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.is_open.get());
    }

    #[test]
    fn set_open_uses_verified_live_state() {
        let mut s = make_initialized();
        s.transport = Some(RefCell::new(Box::new(
            MockTransport::new()
                .expect("on=1\r", "ok")
                .expect("on?\r", "0"),
        )));
        s.set_open(true).unwrap();
        assert!(!s.is_open.get());
    }
}
