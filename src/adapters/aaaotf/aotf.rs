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
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::thread;
use std::time::Duration;

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
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("State", &["0", "1"]).unwrap();
        props
            .define_property("Power (% of max)", PropertyValue::Float(100.0), false)
            .unwrap();
        props
            .set_property_limits("Power (% of max)", 0.0, 100.0)
            .unwrap();
        props
            .define_property("Frequency (MHz)", PropertyValue::Float(100.0), false)
            .unwrap();
        props
            .set_property_limits("Frequency (MHz)", 50.0, 200.0)
            .unwrap();
        props
            .define_property(
                "Maximum intensity (dB)",
                PropertyValue::Integer(1900),
                false,
            )
            .unwrap();
        props
            .set_property_limits("Maximum intensity (dB)", 0.0, 2200.0)
            .unwrap();
        props
            .define_property("Channel", PropertyValue::String("1".into()), false)
            .unwrap();
        props
            .set_allowed_values("Channel", &["1", "2", "3", "4", "5", "6", "7", "8"])
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
        let cmd = format!("{command}\r");
        self.call_transport(|t| t.send(&cmd))
    }

    fn close_all_channels_startup(&mut self) -> MmResult<()> {
        let mut command = String::new();
        for ch in 1u8..=8 {
            command.push_str(&format!("L{}O0\r", ch));
        }
        self.send(&command)
    }

    fn set_channel_state(&mut self, open: bool) -> MmResult<()> {
        let flag = if open { 1 } else { 0 };
        let cmd = format!("L{}O{}", self.channel, flag);
        self.send(&cmd)?;
        self.state = open;
        self.props
            .set("State", PropertyValue::Integer(if open { 1 } else { 0 }))?;
        Ok(())
    }

    pub fn set_intensity(&mut self, pct: f64) -> MmResult<()> {
        if !(0.0..=100.0).contains(&pct) {
            return Err(MmError::InvalidPropertyValue);
        }
        let val = pct * self.max_intensity as f64 / 10000.0;
        let cmd = format!("L{}D{}", self.channel, val);
        self.send(&cmd)?;
        self.intensity_pct = pct;
        self.props
            .set("Power (% of max)", PropertyValue::Float(pct))?;
        Ok(())
    }

    pub fn set_frequency(&mut self, mhz: f64) -> MmResult<()> {
        if !(50.0..=200.0).contains(&mhz) {
            return Err(MmError::InvalidPropertyValue);
        }
        let cmd = format!("L{}F{:.2}", self.channel, mhz);
        self.send(&cmd)?;
        self.freq_mhz = mhz;
        self.props
            .set("Frequency (MHz)", PropertyValue::Float(mhz))?;
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
        "AA AOTF Shutter Controller driver adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.initialized {
            return Ok(());
        }
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Set internal modulation mode
        self.send("I0")?;
        self.set_channel_state(false)?;
        // Close all channels
        self.close_all_channels_startup()?;
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
            "Port" => {
                if self.initialized {
                    return Err(MmError::InvalidProperty);
                }
                self.props.set(name, val)
            }
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
                self.props
                    .set("Maximum intensity (dB)", PropertyValue::Integer(v))?;
                self.max_intensity = v;
                Ok(())
            }
            "Channel" => {
                let s = val.as_str().to_string();
                let ch: u8 = s.parse().map_err(|_| MmError::InvalidPropertyValue)?;
                if ch < 1 || ch > 8 {
                    return Err(MmError::InvalidPropertyValue);
                }
                let was_open = self.state;
                self.props
                    .set("Channel", PropertyValue::String(ch.to_string()))?;
                self.channel = ch;
                if was_open {
                    self.set_channel_state(true)?;
                }
                Ok(())
            }
            "State" => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if v != 0 && v != 1 {
                    return Err(MmError::InvalidPropertyValue);
                }
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
        Err(MmError::UnsupportedCommand)
    }
}

// ─── Multi-channel AOTF ──────────────────────────────────────────────────────

pub struct AaMultiAotf {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    /// 8-bit bitmask of active channels (bit0=ch1 … bit7=ch8)
    channel_mask: u8,
    /// Milliseconds to wait between per-channel commands
    delay_between_channels_ms: f64,
    /// Shutter state
    state: bool,
}

impl AaMultiAotf {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("State", &["0", "1"]).unwrap();
        props
            .define_property(
                "Delay between channels (ms)",
                PropertyValue::Float(0.0),
                false,
            )
            .unwrap();
        props
            .define_property(
                "Channels (8 bit word 1..255)",
                PropertyValue::Integer(200),
                false,
            )
            .unwrap();
        props
            .set_property_limits("Channels (8 bit word 1..255)", 1.0, 255.0)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            channel_mask: 1,
            delay_between_channels_ms: 0.0,
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
        let cmd = format!("{command}\r");
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
            if self.delay_between_channels_ms > 0.0 {
                thread::sleep(Duration::from_millis(
                    self.delay_between_channels_ms.ceil() as u64
                ));
            }
        }
        self.state = open;
        self.props
            .set("State", PropertyValue::Integer(if open { 1 } else { 0 }))?;
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
        "multiline AA AOTF Shutter Controller driver adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.initialized {
            return Ok(());
        }
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.send("I0")?;
        self.set_channels_state(false)?;
        // Close all channels
        for ch in 1u8..=8 {
            thread::sleep(Duration::from_millis(50));
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
            "Channels (8 bit word 1..255)" => Ok(PropertyValue::Integer(self.channel_mask as i64)),
            "Delay between channels (ms)" => {
                Ok(PropertyValue::Float(self.delay_between_channels_ms))
            }
            "State" => Ok(PropertyValue::Integer(if self.state { 1 } else { 0 })),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" => {
                if self.initialized {
                    return Err(MmError::InvalidProperty);
                }
                self.props.set(name, val)
            }
            "Channels (8 bit word 1..255)" => {
                let mask = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if mask < 1 || mask > 255 {
                    return Err(MmError::InvalidPropertyValue);
                }
                let was_open = self.state;
                self.props
                    .set("Channels (8 bit word 1..255)", PropertyValue::Integer(mask))?;
                self.channel_mask = mask as u8;
                if was_open {
                    self.set_channels_state(true)?;
                }
                Ok(())
            }
            "Delay between channels (ms)" => {
                let delay = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.delay_between_channels_ms = delay.max(0.0);
                self.props.set(
                    "Delay between channels (ms)",
                    PropertyValue::Float(self.delay_between_channels_ms),
                )
            }
            "State" => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if v != 0 && v != 1 {
                    return Err(MmError::InvalidPropertyValue);
                }
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
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;
    use std::sync::{Arc, Mutex};

    struct RecordingTransport {
        received: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingTransport {
        fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
            let received = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    received: received.clone(),
                },
                received,
            )
        }
    }

    impl Transport for RecordingTransport {
        fn send(&mut self, cmd: &str) -> MmResult<()> {
            self.received.lock().unwrap().push(cmd.to_string());
            Ok(())
        }

        fn receive_line(&mut self) -> MmResult<String> {
            Err(MmError::SerialTimeout)
        }

        fn purge(&mut self) -> MmResult<()> {
            Ok(())
        }
    }

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
    fn aotf_property_surface_matches_upstream_order() {
        let d = AaAotf::new();
        assert_eq!(
            d.property_names(),
            vec![
                "Port",
                "State",
                "Power (% of max)",
                "Frequency (MHz)",
                "Maximum intensity (dB)",
                "Channel",
            ]
        );
    }

    #[test]
    fn aotf_commands_are_cr_terminated() {
        let (transport, received) = RecordingTransport::new();
        let mut d = AaAotf::new().with_transport(Box::new(transport));
        d.initialize().unwrap();
        d.set_open(true).unwrap();
        d.set_intensity(50.0).unwrap();
        d.set_frequency(150.0).unwrap();

        let received = received.lock().unwrap();
        assert_eq!(received[0], "I0\r");
        assert_eq!(received[1], "L1O0\r");
        assert_eq!(
            received[2],
            "L1O0\rL2O0\rL3O0\rL4O0\rL5O0\rL6O0\rL7O0\rL8O0\r\r"
        );
        assert_eq!(received[3], "L1O1\r");
        assert_eq!(received[4], "L1D9.5\r");
        assert_eq!(received[5], "L1F150.00\r");
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
        assert_eq!(d.fire(0.0).unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn aotf_rejects_out_of_range_action_values() {
        let t = MockTransport::new();
        let mut d = AaAotf::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(
            d.set_property("Power (% of max)", PropertyValue::Float(101.0))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            d.set_property("Frequency (MHz)", PropertyValue::Float(49.0))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            d.set_property("State", PropertyValue::Integer(2))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn aotf_rejects_port_change_after_initialize() {
        let t = MockTransport::new();
        let mut d = AaAotf::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(
            d.set_property("Port", PropertyValue::String("COM2".into()))
                .unwrap_err(),
            MmError::InvalidProperty
        );
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
    fn multi_aotf_upstream_defaults() {
        let d = AaMultiAotf::new();
        assert_eq!(
            d.property_names(),
            vec![
                "Port",
                "State",
                "Delay between channels (ms)",
                "Channels (8 bit word 1..255)",
            ]
        );
        assert_eq!(
            d.get_property("Channels (8 bit word 1..255)").unwrap(),
            PropertyValue::Integer(1)
        );
        assert_eq!(
            d.get_property("Delay between channels (ms)").unwrap(),
            PropertyValue::Float(0.0)
        );
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
    fn multi_aotf_delay_clamps_negative() {
        let mut d = AaMultiAotf::new();
        d.set_property("Delay between channels (ms)", PropertyValue::Float(-2.5))
            .unwrap();
        assert_eq!(
            d.get_property("Delay between channels (ms)").unwrap(),
            PropertyValue::Float(0.0)
        );
    }

    #[test]
    fn multi_aotf_rejects_port_change_after_initialize() {
        let t = MockTransport::new();
        let mut d = AaMultiAotf::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(
            d.set_property("Port", PropertyValue::String("COM2".into()))
                .unwrap_err(),
            MmError::InvalidProperty
        );
    }

    #[test]
    fn multi_aotf_fire() {
        let t = MockTransport::new();
        let mut d = AaMultiAotf::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.fire(0.0).unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn multi_aotf_device_type() {
        assert_eq!(AaMultiAotf::new().device_type(), DeviceType::Shutter);
    }
}
