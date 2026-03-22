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
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

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
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    model: Xt600Model,
    /// 0-based LED device number.
    led_number: u8,
    /// Intensity 0–100 %.
    intensity: u32,
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
                "Model",
                PropertyValue::String(match model {
                    Xt600Model::Xt600 => "XT600".into(),
                    Xt600Model::Xt900 => "XT900".into(),
                }),
                true,
            )
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            model,
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

    pub fn set_intensity(&mut self, percent: u32) -> MmResult<()> {
        let channel_char = (b'1' + self.led_number) as char;
        let cmd = format!("ip={},{}", percent, channel_char);
        self.cmd(&cmd)?;
        self.intensity = percent;
        Ok(())
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

impl Shutter for Xt600Shutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.set_led_on_off(open)?;
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

    fn make_xt600() -> Xt600Shutter {
        let t = MockTransport::new().any("SN99999").any("ok");
        let mut s = Xt600Shutter::new(Xt600Model::Xt600, 0).with_transport(Box::new(t));
        s.initialize().unwrap();
        s
    }

    fn make_xt900() -> Xt600Shutter {
        let t = MockTransport::new().any("SN99999").any("ok");
        let mut s = Xt600Shutter::new(Xt600Model::Xt900, 8).with_transport(Box::new(t));
        s.initialize().unwrap();
        s
    }

    #[test]
    fn xt600_initialize() {
        let s = make_xt600();
        assert!(s.initialized);
        assert!(!s.get_open().unwrap());
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
        s.transport = Some(Box::new(MockTransport::new().expect("on=1\r", "ok")));
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn set_open_false() {
        let mut s = make_xt600();
        s.transport = Some(Box::new(MockTransport::new().expect("of=1\r", "ok")));
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn fire_opens_then_closes() {
        let mut s = make_xt600();
        s.transport = Some(Box::new(
            MockTransport::new()
                .expect("on=1\r", "ok")
                .expect("of=1\r", "ok"),
        ));
        s.fire(2.0).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn set_intensity() {
        let mut s = make_xt600();
        s.transport = Some(Box::new(MockTransport::new().expect("ip=80,1\r", "ok")));
        s.set_intensity(80).unwrap();
        assert_eq!(s.intensity, 80);
    }

    #[test]
    fn no_transport_error() {
        assert!(Xt600Shutter::new(Xt600Model::Xt600, 0).initialize().is_err());
    }
}
