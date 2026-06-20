/// X-Cite Turbo/XT600 and NOVEM/XT900 LED shutter adapter.
///
/// Serial ASCII protocol identical to X-Cite LED (XLED1) with `\r` terminator.
///
/// The XT600 supports up to 6 LED channels (XT600) or 9 (XT900).
/// Commands use the same `on=<state><channel_char>` form as XLED1.
///
/// LED channel characters (0-based index):
///   0 → 'R' (LED1)
///   1 → 'S' (LED2)
///   ...
///   8 → 'Z' (LED9)
///
/// The C++ XLedDev::SetOpen uses:
///   sCmdSet = {0x6F, 0x6E|0x66, 0x3D, 0x31+led_number, TxTerm}
/// Translating: "on=<'1'+dev>\r" for on, "of=<'1'+dev>\r" for off.
///
/// The controller device (XT600Ctrl) uses 2-byte command codes like
/// sn? (serial number), us? (status), pm? (pulse mode), etc.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};

const XT600_LED_METADATA_PROPERTIES: &[(&str, &str)] = &[
    ("L.01 Device ", "ln?"),
    ("L.02 Manufacturing Date", "md?"),
    ("L.03 WaveLength", "lw?"),
    ("L.04 FWHM Value", "lf?"),
    ("L.07 Hours Elapsed", "lh?"),
    ("L.08 Minimum Intensity", "ni?"),
    ("L.09 Current Temperature (Deg.C)", "gt?"),
    ("L.10 Max Allowed Temperature (Deg.C)", "mt?"),
    ("L.11 Min Allowed Temperature (Deg.C)", "nt?"),
    ("L.12 Temperature Hysteresis (Deg.C)", "th?"),
    ("L.13 Trigger Sequence", "ts?"),
];

/// Which XT600 hardware variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Xt600Model {
    /// Turbo/XT600 — 6 LED channels.
    Xt600,
    /// NOVEM/XT900 — 9 LED channels.
    Xt900,
}

pub struct Xt600Shutter {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    is_open: Cell<bool>,
    model: Xt600Model,
    /// 0-based LED device number.
    led_number: u8,
    /// Intensity 0–100 %.
    intensity: Cell<u32>,
}

impl Xt600Shutter {
    pub fn new(model: Xt600Model, led_number: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Intensity", PropertyValue::Integer(50), false)
            .unwrap();
        props
            .define_property(
                "L.15 Intensity (0.0 or 5.0 - 100.0)%",
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
        props
            .define_property(
                "Model",
                PropertyValue::String(match model {
                    Xt600Model::Xt600 => "XT600".into(),
                    Xt600Model::Xt900 => "XT900".into(),
                }),
                true,
            )
            .unwrap();
        for (name, _) in XT600_LED_METADATA_PROPERTIES {
            props
                .define_property(*name, PropertyValue::String(String::new()), true)
                .unwrap();
        }
        Self {
            props,
            transport: None,
            initialized: false,
            is_open: Cell::new(false),
            model,
            led_number,
            intensity: Cell::new(50),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(RefCell::new(t));
        self
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

    fn max_channels(&self) -> u8 {
        match self.model {
            Xt600Model::Xt600 => 6,
            Xt600Model::Xt900 => 9,
        }
    }

    fn set_led_on_off(&mut self, on: bool) -> MmResult<()> {
        let second = if on { 'n' } else { 'f' };
        let channel_char = (b'1' + self.led_number) as char;
        let cmd = format!("o{}={}", second, channel_char);
        self.cmd(&cmd)?;
        Ok(())
    }

    fn live_open(&self) -> MmResult<bool> {
        let response = self.cmd("on?")?;
        let idx = self.led_number as usize;
        let open = response
            .as_bytes()
            .get(idx)
            .map(|b| *b == b'1' || *b == b'Y' || *b == b'y')
            .unwrap_or_else(|| response.trim() == "1");
        self.is_open.set(open);
        Ok(open)
    }

    pub fn set_intensity(&mut self, percent: u32) -> MmResult<()> {
        // C++ XLedDev::OnLedIntensity converts percent to tenths and selects
        // channels by comma-padding before the value: LED0 -> ip=800,
        // LED1 -> ip=,800, etc.
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

pub struct Xt600Controller {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    model: Xt600Model,
}

impl Xt600Controller {
    pub fn new(model: Xt600Model) -> Self {
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
            model,
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

impl Device for Xt600Controller {
    fn name(&self) -> &str {
        match self.model {
            Xt600Model::Xt600 => "XT600Controller",
            Xt600Model::Xt900 => "XT900Controller",
        }
    }
    fn description(&self) -> &str {
        "X-Cite XT600/XT900 controller"
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

impl Default for Xt600Shutter {
    fn default() -> Self {
        Self::new(Xt600Model::Xt600, 0)
    }
}

impl Device for Xt600Shutter {
    fn name(&self) -> &str {
        match self.model {
            Xt600Model::Xt600 => "XT600-LED-Shutter",
            Xt600Model::Xt900 => "XT900-LED-Shutter",
        }
    }
    fn description(&self) -> &str {
        "X-Cite Turbo/XT600 or NOVEM/XT900 LED shutter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let max = self.max_channels();
        if self.led_number >= max {
            return Err(MmError::InvalidInputParam);
        }
        // Query serial number to verify connection
        let serial = self.cmd("sn?")?;
        self.props
            .entry_mut("SerialNumber")
            .map(|e| e.value = PropertyValue::String(serial));
        if let Ok(status) = self.cmd("us?") {
            self.props
                .entry_mut("UnitStatus")
                .map(|e| e.value = PropertyValue::String(status));
        }
        // Turn LED off at init
        self.set_led_on_off(false)?;
        self.is_open.set(false);
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.set_led_on_off(false);
            self.is_open.set(false);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if self.initialized {
            match name {
                "SerialNumber" => return Ok(PropertyValue::String(self.cmd("sn?")?)),
                "UnitStatus" => return Ok(PropertyValue::String(self.cmd("us?")?)),
                "L.15 Intensity (0.0 or 5.0 - 100.0)%" => {
                    return Ok(PropertyValue::String(self.read_led_field("ip?")?))
                }
                "LED-On-State" => {
                    return Ok(PropertyValue::String(
                        if self.live_open()? { "On" } else { "Off" }.into(),
                    ))
                }
                _ => {
                    if let Some((_, command)) = XT600_LED_METADATA_PROPERTIES
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
            "Intensity" | "L.15 Intensity (0.0 or 5.0 - 100.0)%" => {
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
                    "L.15 Intensity (0.0 or 5.0 - 100.0)%",
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

impl Shutter for Xt600Shutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.set_led_on_off(open)?;
        self.is_open.set(open);
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

    fn make_xt600() -> Xt600Shutter {
        let t = MockTransport::new().any("SN99999").any("0").any("ok");
        let mut s = Xt600Shutter::new(Xt600Model::Xt600, 0).with_transport(Box::new(t));
        s.initialize().unwrap();
        s
    }

    fn make_xt900() -> Xt600Shutter {
        let t = MockTransport::new().any("SN99999").any("0").any("ok");
        let mut s = Xt600Shutter::new(Xt600Model::Xt900, 8).with_transport(Box::new(t));
        s.initialize().unwrap();
        s
    }

    #[test]
    fn xt600_initialize() {
        let s = make_xt600();
        assert!(s.initialized);
        assert!(!s.is_open.get());
    }

    #[test]
    fn xt900_led9_initialize() {
        let s = make_xt900();
        assert!(s.initialized);
        // LED 8 → channel char '9'
        assert_eq!((b'1' + s.led_number) as char, '9');
    }

    #[test]
    fn xt600_out_of_range_led_fails() {
        // XT600 has 6 channels; led_number=6 is out of range
        let t = MockTransport::new();
        let mut s = Xt600Shutter::new(Xt600Model::Xt600, 6).with_transport(Box::new(t));
        assert!(s.initialize().is_err());
    }

    #[test]
    fn set_open_true() {
        let mut s = make_xt600();
        s.transport = Some(RefCell::new(Box::new(
            MockTransport::new()
                .expect("on=1\r", "ok")
                .expect("on?\r", "1"),
        )));
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn set_open_false() {
        let mut s = make_xt600();
        s.transport = Some(RefCell::new(Box::new(
            MockTransport::new()
                .expect("of=1\r", "ok")
                .expect("on?\r", "0"),
        )));
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn fire_is_unsupported() {
        let mut s = make_xt600();
        assert_eq!(s.fire(2.0), Err(MmError::UnsupportedCommand));
        assert!(!s.is_open.get());
    }

    #[test]
    fn set_intensity() {
        let mut s = make_xt600();
        s.transport = Some(RefCell::new(Box::new(
            MockTransport::new().expect("ip=800\r", "ok"),
        )));
        s.set_intensity(80).unwrap();
        assert_eq!(s.intensity.get(), 80);
    }

    #[test]
    fn intensity_property_sends_command() {
        let mut s = make_xt600();
        s.transport = Some(RefCell::new(Box::new(
            MockTransport::new().expect("ip=800\r", "ok"),
        )));
        s.set_property("Intensity", PropertyValue::Integer(80))
            .unwrap();
        assert_eq!(
            s.get_property("Intensity").unwrap(),
            PropertyValue::Integer(80)
        );
    }

    #[test]
    fn metadata_and_state_are_live_reads() {
        let t = MockTransport::new()
            .any("SN99999")
            .any("0")
            .any("ok")
            .expect("sn?\r", "SNXT")
            .expect("us?\r", "READY")
            .expect("on?\r", "1");
        let mut s = Xt600Shutter::new(Xt600Model::Xt600, 0).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.get_property("SerialNumber").unwrap(),
            PropertyValue::String("SNXT".into())
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
            .any("SN99999")
            .any("0")
            .any("ok")
            .expect("lw?\r", "385,470,550,635,740,770")
            .expect("ts?\r", "0,1,2,3,4,5");
        let mut s = Xt600Shutter::new(Xt600Model::Xt600, 4).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.get_property("L.03 WaveLength").unwrap(),
            PropertyValue::String("740".into())
        );
        assert_eq!(
            s.get_property("L.13 Trigger Sequence").unwrap(),
            PropertyValue::String("4".into())
        );
    }

    #[test]
    fn no_transport_error() {
        assert!(Xt600Shutter::new(Xt600Model::Xt600, 0)
            .initialize()
            .is_err());
    }
}
