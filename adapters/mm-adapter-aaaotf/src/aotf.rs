/// AA Crystal Technology AOTF (Acousto-Optic Tunable Filter) adapter.
///
/// ASCII serial protocol (CR terminated, no response from device).
///
/// Commands:
///   `I0`              → set internal modulation mode (sent on init)
///   `L{ch}O0`         → switch channel ch (1–8) off
///   `L{ch}O1`         → switch channel ch (1–8) on
///   `L{ch}D{val}`     → set channel amplitude (dB·10000/maxint, float)
///   `L{ch}F{freq:.2}` → set channel frequency in MHz (float, 2 dp)
///
/// `AaAotf` controls a single channel.
/// `AaMultiAotf` controls multiple channels via an 8-bit bitmask.
///
/// Both implement `Shutter`.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

// ─── Single-channel AOTF ─────────────────────────────────────────────────────

pub struct AaAotf {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    /// Active channel (1–8)
    channel: u8,
    /// Power as percentage (0–100)
    intensity_pct: f64,
    /// Frequency in MHz
    freq_mhz: f64,
    /// Maximum intensity encoding (dB units × 100, range 0–2200)
    max_intensity: i64,
    /// Shutter state
    state: bool,
}

impl AaAotf {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "Channel",
                PropertyValue::String("1".into()),
                false,
            )
            .unwrap();
        props
            .define_property(
                "Power (% of max)",
                PropertyValue::Float(100.0),
                false,
            )
            .unwrap();
        props
            .define_property(
                "Frequency (MHz)",
                PropertyValue::Float(100.0),
                false,
            )
            .unwrap();
        props
            .define_property(
                "Maximum intensity (dB)",
                PropertyValue::Integer(1900),
                false,
            )
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            channel: 1,
            intensity_pct: 100.0,
            freq_mhz: 100.0,
            max_intensity: 1900,
            state: false,
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

    fn send(&mut self, command: &str) -> MmResult<()> {
        let cmd = command.to_string();
        self.call_transport(|t| t.send(&cmd))
    }

    fn set_channel_state(&mut self, open: bool) -> MmResult<()> {
        let flag = if open { 1 } else { 0 };
        let cmd = format!("L{}O{}", self.channel, flag);
        self.send(&cmd)?;
        self.state = open;
        Ok(())
    }

    pub fn set_intensity(&mut self, pct: f64) -> MmResult<()> {
        let val = pct * self.max_intensity as f64 / 10000.0;
        let cmd = format!("L{}D{}", self.channel, val);
        self.send(&cmd)?;
        self.intensity_pct = pct;
        Ok(())
    }

    pub fn set_frequency(&mut self, mhz: f64) -> MmResult<()> {
        let cmd = format!("L{}F{:.2}", self.channel, mhz);
        self.send(&cmd)?;
        self.freq_mhz = mhz;
        Ok(())
    }
}

impl Default for AaAotf {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for AaAotf {
    fn name(&self) -> &str {
        "AAAOTF"
    }

    fn description(&self) -> &str {
        "AA Crystal Technology AOTF shutter controller"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Set internal modulation mode
        self.send("I0")?;
        // Close all channels
        for ch in 1u8..=8 {
            let cmd = format!("L{}O0", ch);
            self.send(&cmd)?;
        }
        self.state = false;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Power (% of max)" => Ok(PropertyValue::Float(self.intensity_pct)),
            "Frequency (MHz)" => Ok(PropertyValue::Float(self.freq_mhz)),
            "Maximum intensity (dB)" => Ok(PropertyValue::Integer(self.max_intensity)),
            "Channel" => Ok(PropertyValue::String(self.channel.to_string())),
            "State" => Ok(PropertyValue::Integer(if self.state { 1 } else { 0 })),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Power (% of max)" => {
                let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_intensity(pct)
            }
            "Frequency (MHz)" => {
                let mhz = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_frequency(mhz)
            }
            "Maximum intensity (dB)" => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.max_intensity = v;
                Ok(())
            }
            "Channel" => {
                let s = val.as_str().to_string();
                let ch: u8 = s
                    .parse()
                    .map_err(|_| MmError::InvalidPropertyValue)?;
                if ch < 1 || ch > 8 {
                    return Err(MmError::InvalidPropertyValue);
                }
                let was_open = self.state;
                self.channel = ch;
                if was_open {
                    self.set_channel_state(true)?;
                }
                Ok(())
            }
            "State" => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_channel_state(v != 0)
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

impl Shutter for AaAotf {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.set_channel_state(open)
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.state)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        self.set_open(true)?;
        self.set_open(false)
    }
}

// ─── Multi-channel AOTF ──────────────────────────────────────────────────────

pub struct AaMultiAotf {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    /// 8-bit bitmask of active channels (bit0=ch1 … bit7=ch8)
    channel_mask: u8,
    /// Shutter state
    state: bool,
}

impl AaMultiAotf {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "Channels (8 bit word 1..255)",
                PropertyValue::Integer(200),
                false,
            )
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            channel_mask: 200,
            state: false,
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

    fn send(&mut self, command: &str) -> MmResult<()> {
        let cmd = command.to_string();
        self.call_transport(|t| t.send(&cmd))
    }

    fn set_channels_state(&mut self, open: bool) -> MmResult<()> {
        for ch in 1u8..=8 {
            let bit = 1u8 << (ch - 1);
            let flag = if open && (self.channel_mask & bit != 0) {
                1
            } else {
                0
            };
            let cmd = format!("L{}O{}", ch, flag);
            self.send(&cmd)?;
        }
        self.state = open;
        Ok(())
    }
}

impl Default for AaMultiAotf {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for AaMultiAotf {
    fn name(&self) -> &str {
        "multiAAAOTF"
    }

    fn description(&self) -> &str {
        "AA Crystal Technology multi-channel AOTF shutter controller"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.send("I0")?;
        // Close all channels
        for ch in 1u8..=8 {
            let cmd = format!("L{}O0", ch);
            self.send(&cmd)?;
        }
        self.state = false;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Channels (8 bit word 1..255)" => {
                Ok(PropertyValue::Integer(self.channel_mask as i64))
            }
            "State" => Ok(PropertyValue::Integer(if self.state { 1 } else { 0 })),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Channels (8 bit word 1..255)" => {
                let mask = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if mask < 1 || mask > 255 {
                    return Err(MmError::InvalidPropertyValue);
                }
                let was_open = self.state;
                self.channel_mask = mask as u8;
                if was_open {
                    self.set_channels_state(true)?;
                }
                Ok(())
            }
            "State" => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_channels_state(v != 0)
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

impl Shutter for AaMultiAotf {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.set_channels_state(open)
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.state)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        self.set_open(true)?;
        self.set_open(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    // ─── AaAotf tests ─────────────────────────────────────────────────────────

    fn init_aotf() -> AaAotf {
        // init: I0 + L1O0..L8O0 (9 sends, no responses)
        let t = MockTransport::new();
        let mut d = AaAotf::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d
    }

    #[test]
    fn aotf_initialize() {
        let d = init_aotf();
        assert!(d.initialized);
        assert!(!d.state);
    }

    #[test]
    fn aotf_no_transport_error() {
        assert!(AaAotf::new().initialize().is_err());
    }

    #[test]
    fn aotf_set_open_sends_command() {
        let t = MockTransport::new();
        let mut d = AaAotf::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_open(true).unwrap();
        assert!(d.get_open().unwrap());
        d.set_open(false).unwrap();
        assert!(!d.get_open().unwrap());
    }

    #[test]
    fn aotf_set_intensity() {
        let t = MockTransport::new();
        let mut d = AaAotf::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_intensity(50.0).unwrap();
        assert!((d.intensity_pct - 50.0).abs() < 0.01);
    }

    #[test]
    fn aotf_set_frequency() {
        let t = MockTransport::new();
        let mut d = AaAotf::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_frequency(150.0).unwrap();
        assert!((d.freq_mhz - 150.0).abs() < 0.01);
    }

    #[test]
    fn aotf_fire() {
        let t = MockTransport::new();
        let mut d = AaAotf::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.fire(0.0).unwrap();
        assert!(!d.get_open().unwrap());
    }

    #[test]
    fn aotf_device_type() {
        assert_eq!(AaAotf::new().device_type(), DeviceType::Shutter);
    }

    // ─── AaMultiAotf tests ────────────────────────────────────────────────────

    #[test]
    fn multi_aotf_initialize() {
        let t = MockTransport::new();
        let mut d = AaMultiAotf::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert!(d.initialized);
        assert!(!d.state);
    }

    #[test]
    fn multi_aotf_open_uses_mask() {
        let t = MockTransport::new();
        // mask=0b00000011 = channels 1 and 2
        let mut d = AaMultiAotf::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.channel_mask = 0b00000011;
        d.set_open(true).unwrap();
        assert!(d.state);
    }

    #[test]
    fn multi_aotf_no_transport_error() {
        assert!(AaMultiAotf::new().initialize().is_err());
    }

    #[test]
    fn multi_aotf_device_type() {
        assert_eq!(AaMultiAotf::new().device_type(), DeviceType::Shutter);
    }
}
