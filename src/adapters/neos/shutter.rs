/// Neos Technologies acousto-optic device shutter controller.
///
/// Protocol (TX `\r`, NO response from device):
///   `CH <1-8>\r`     → select channel
///   `ON\r`           → open (enable) shutter
///   `OFF\r`          → close (disable) shutter
///   `AM <0-1023>\r`  → set amplitude/intensity (0–1023)
///
/// Device provides no acknowledgement; state is tracked internally.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::time::Instant;

pub struct NeosShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    channel: u8,
    amplitude: u16,
    is_open: bool,
    delay_ms: f64,
    changed_time: Instant,
}

impl NeosShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Channel", PropertyValue::Integer(4), false)
            .unwrap();
        props.set_property_limits("Channel", 1.0, 8.0).unwrap();
        props
            .set_allowed_values("Channel", &["1", "2", "3", "4", "5", "6", "7", "8"])
            .unwrap();
        props
            .define_property("Optimal amplitude", PropertyValue::Integer(1024), false)
            .unwrap();
        props
            .define_property("Delay_ms", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("Delay_ms", 0.0, f64::MAX)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            channel: 4,
            amplitude: 200,
            is_open: false,
            delay_ms: 0.0,
            changed_time: Instant::now(),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn define_initialized_properties(&mut self) -> MmResult<()> {
        if !self.props.has_property("Amplitude") {
            let amplitude_max = self
                .props
                .get("Optimal amplitude")?
                .as_f64()
                .ok_or(MmError::InvalidPropertyValue)?;
            self.props.define_property(
                "Amplitude",
                PropertyValue::Integer(self.amplitude as i64),
                false,
            )?;
            self.props
                .set_property_limits("Amplitude", 0.0, amplitude_max)?;
        }
        Ok(())
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
        let c = format!("{}\r", command);
        self.call_transport(|t| {
            t.purge()?;
            t.send(&c)
        })?;
        self.changed_time = Instant::now();
        Ok(())
    }
}

impl Default for NeosShutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for NeosShutter {
    fn name(&self) -> &str {
        "Neos"
    }
    fn description(&self) -> &str {
        "Neos controller"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.set_open(false)?;
        self.define_initialized_properties()?;
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
        match name {
            "Delay_ms" => Ok(PropertyValue::Float(self.delay_ms)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Port" && self.initialized {
            return Err(MmError::InvalidPropertyValue);
        }
        if name == "Channel" {
            let ch_i64 = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            if !(1..=8).contains(&ch_i64) {
                return Err(MmError::InvalidPropertyValue);
            }
            let ch = ch_i64 as u8;
            let old_channel = self.channel;
            if self.initialized && self.is_open {
                self.set_open(true)?;
            }
            self.channel = ch;
            if self.initialized && self.is_open {
                if let Err(err) = self.set_open(true) {
                    self.channel = old_channel;
                    return Err(err);
                }
            }
            return self.props.set(name, PropertyValue::Integer(ch as i64));
        }
        if name == "Amplitude" {
            if !self.props.has_property("Amplitude") {
                return Err(MmError::UnknownLabel(name.to_string()));
            }
            let amp_i64 = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            if !(0..=1024).contains(&amp_i64) {
                return Err(MmError::InvalidPropertyValue);
            }
            if let Some(entry) = self.props.entry("Amplitude") {
                if entry.has_limits && amp_i64 as f64 > entry.upper_limit {
                    return Err(MmError::InvalidPropertyValue);
                }
            }
            let amp = amp_i64 as u16;
            if self.initialized {
                self.send(&format!("CH {}", self.channel))?;
                self.send(&format!("AM {}", amp))?;
            }
            self.amplitude = amp;
            return self.props.set(name, PropertyValue::Integer(amp as i64));
        }
        if name == "Delay_ms" {
            let delay = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            if delay < 0.0 {
                return Err(MmError::InvalidPropertyValue);
            }
            self.delay_ms = delay;
            return self.props.set(name, PropertyValue::Float(delay));
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
        self.changed_time.elapsed().as_secs_f64() * 1000.0 < self.delay_ms
    }
}

impl Shutter for NeosShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.send(&format!("CH {}", self.channel))?;
        self.send(if open { "ON" } else { "OFF" })?;
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
    use std::sync::{Arc, Mutex};

    struct LoggingTransport {
        commands: Arc<Mutex<Vec<String>>>,
    }

    impl Transport for LoggingTransport {
        fn send(&mut self, cmd: &str) -> MmResult<()> {
            self.commands.lock().unwrap().push(cmd.to_string());
            Ok(())
        }

        fn receive_line(&mut self) -> MmResult<String> {
            Err(MmError::SerialTimeout)
        }

        fn purge(&mut self) -> MmResult<()> {
            Ok(())
        }
    }

    #[test]
    fn initialize() {
        // 2 sends: CH 4, OFF — no responses
        let t = MockTransport::new();
        let mut s = NeosShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn open_close() {
        let t = MockTransport::new();
        let mut s = NeosShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn set_channel() {
        let t = MockTransport::new();
        let mut s = NeosShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("Channel", PropertyValue::Integer(3))
            .unwrap();
        assert_eq!(s.channel, 3);
    }

    #[test]
    fn channel_has_upstream_allowed_values() {
        let s = NeosShutter::new();
        let entry = s.props.entry("Channel").unwrap();
        assert_eq!(
            entry.allowed_values,
            vec!["1", "2", "3", "4", "5", "6", "7", "8"]
        );
    }

    #[test]
    fn invalid_channel_does_not_mutate_cache() {
        let t = MockTransport::new();
        let mut s = NeosShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s
            .set_property("Channel", PropertyValue::Integer(9))
            .is_err());
        assert_eq!(s.channel, 4);
    }

    #[test]
    fn amplitude_is_created_during_initialize_like_upstream() {
        let t = MockTransport::new();
        let mut s = NeosShutter::new().with_transport(Box::new(t));
        assert!(!s.has_property("Amplitude"));
        assert!(s
            .set_property("Amplitude", PropertyValue::Integer(800))
            .is_err());
        assert_eq!(s.amplitude, 200);

        s.initialize().unwrap();

        assert!(s.has_property("Amplitude"));
        assert_eq!(
            s.get_property("Amplitude").unwrap(),
            PropertyValue::Integer(200)
        );
    }

    #[test]
    fn preinit_optimal_amplitude_sets_initialize_time_amplitude_limit() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let t = LoggingTransport {
            commands: Arc::clone(&commands),
        };
        let mut s = NeosShutter::new().with_transport(Box::new(t));
        s.set_property("Optimal amplitude", PropertyValue::Integer(500))
            .unwrap();
        s.initialize().unwrap();
        commands.lock().unwrap().clear();

        assert!(s
            .set_property("Amplitude", PropertyValue::Integer(800))
            .is_err());
        assert_eq!(s.amplitude, 200);
        assert!(commands.lock().unwrap().is_empty());
    }

    #[test]
    fn changing_channel_while_open_reopens_new_channel_once() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let t = LoggingTransport {
            commands: Arc::clone(&commands),
        };
        let mut s = NeosShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        s.set_property("Channel", PropertyValue::Integer(3))
            .unwrap();
        assert_eq!(s.channel, 3);
        assert_eq!(
            *commands.lock().unwrap(),
            vec!["CH 4\r", "OFF\r", "CH 4\r", "ON\r", "CH 4\r", "ON\r", "CH 3\r", "ON\r"]
        );
    }

    #[test]
    fn set_amplitude() {
        let t = MockTransport::new();
        let mut s = NeosShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("Amplitude", PropertyValue::Integer(800))
            .unwrap();
        assert_eq!(s.amplitude, 800);
    }

    #[test]
    fn invalid_amplitude_does_not_mutate_cache() {
        let t = MockTransport::new();
        let mut s = NeosShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s
            .set_property("Amplitude", PropertyValue::Integer(1025))
            .is_err());
        assert_eq!(s.amplitude, 200);
    }

    #[test]
    fn no_transport_error() {
        assert!(NeosShutter::new().initialize().is_err());
    }

    #[test]
    fn fire_is_unsupported_like_upstream() {
        let t = MockTransport::new();
        let mut s = NeosShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.fire(1.0), Err(MmError::UnsupportedCommand));
    }
}
