/// Thorlabs SC10 shutter controller adapter.
///
/// ASCII serial protocol, 9600 baud, 8N1, no flow control.
///
/// Commands (CR terminated, device echoes the command then replies ending with `>`):
///   `*idn?`   → device identification string
///   `mode=1`  → set to manual mode (required for normal operation)
///   `ens`     → toggle shutter state (open↔closed)
///   `ens?`    → query shutter state: "0" = closed, non-zero = open
///
/// The device echoes every command; the echo is stripped before returning the answer.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct ThorlabsSC10 {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
}

impl ThorlabsSC10 {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
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

    /// Send a command and receive the reply (echo-stripped).
    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let cmd = command.to_string();
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Self::strip_echo(&cmd, &resp)
        })
    }

    fn strip_echo(command: &str, response: &str) -> MmResult<String> {
        let mut answer = response.trim().trim_end_matches('>').trim().to_string();
        if let Some(rest) = answer.strip_prefix(command) {
            answer = rest
                .trim_start_matches(['\r', '\n', ' '])
                .trim()
                .to_string();
        } else if answer.starts_with(' ') {
            let trimmed = answer.trim_start();
            if let Some(rest) = trimmed.strip_prefix(command) {
                answer = rest
                    .trim_start_matches(['\r', '\n', ' '])
                    .trim()
                    .to_string();
            } else {
                return Err(MmError::SerialCommandFailed);
            }
        } else {
            return Err(MmError::SerialCommandFailed);
        }
        Ok(answer)
    }

    fn query_open(&mut self) -> MmResult<bool> {
        let answer = self.cmd("ens?")?;
        let open = answer.trim().parse::<i64>().unwrap_or(0) != 0;
        self.is_open = open;
        Ok(open)
    }
}

impl Default for ThorlabsSC10 {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ThorlabsSC10 {
    fn name(&self) -> &str {
        "SC10"
    }

    fn description(&self) -> &str {
        "SC10 D1 controller adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Query device identity (retry once if the first attempt fails, per C++ original)
        let idn = self.cmd("*idn?").or_else(|_| self.cmd("*idn?"))?;
        if !self.props.has_property("Device Info") {
            self.props
                .define_property("Device Info", PropertyValue::String(idn), true)?;
        }
        // Set manual mode — required for normal shutter operation
        let _ = self.cmd("mode=1")?;
        if !self.props.has_property("SC10 Command:") {
            self.props.define_property(
                "SC10 Command:",
                PropertyValue::String(String::new()),
                false,
            )?;
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidProperty),
            "SC10 Command:" => {
                let command = val.as_str();
                let answer = self.cmd(&command)?;
                self.props.set(name, PropertyValue::String(answer))
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

impl Shutter for ThorlabsSC10 {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        // Only toggle if state differs. The C++ adapter queries the controller
        // each time rather than relying on a cached state.
        let current = self.query_open()?;
        if current != open {
            let _ = self.cmd("ens")?;
        }
        self.is_open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        // We return the cached state; a live query would call self.cmd("ens?")
        // but get_open takes &self so we return cached.
        Ok(self.is_open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_device() -> ThorlabsSC10 {
        // init: *idn? → "SC10 ver1.0", mode=1 → "1"
        let t = MockTransport::new()
            .expect("*idn?", "*idn?\rSC10 ver1.0")
            .expect("mode=1", "mode=1\r1");
        ThorlabsSC10::new().with_transport(Box::new(t))
    }

    #[test]
    fn initialize_succeeds() {
        let mut d = make_device();
        d.initialize().unwrap();
        assert!(d.initialized);
        assert_eq!(d.name(), "SC10");
        assert_eq!(d.description(), "SC10 D1 controller adapter");
        assert_eq!(
            d.get_property("Device Info").unwrap(),
            PropertyValue::String("SC10 ver1.0".into())
        );
        assert!(d.has_property("SC10 Command:"));
    }

    #[test]
    fn no_transport_errors() {
        assert!(ThorlabsSC10::new().initialize().is_err());
    }

    #[test]
    fn set_open_toggles_once() {
        let t = MockTransport::new()
            .expect("*idn?", "*idn?\rSC10 ver1.0")
            .expect("mode=1", "mode=1\r1")
            .expect("ens?", "ens?\r0")
            .expect("ens", "ens\r1"); // one toggle to open
        let mut d = ThorlabsSC10::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        // Initially closed; opening sends "ens"
        d.set_open(true).unwrap();
        assert!(d.get_open().unwrap());
    }

    #[test]
    fn set_open_no_toggle_if_same_state() {
        // No "ens" command expected beyond init
        let t = MockTransport::new()
            .expect("*idn?", "*idn?\rSC10 ver1.0")
            .expect("mode=1", "mode=1\r1")
            .expect("ens?", "ens?\r0");
        let mut d = ThorlabsSC10::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        // Already closed; closing again should not send "ens"
        d.set_open(false).unwrap();
        assert!(!d.get_open().unwrap());
    }

    #[test]
    fn fire_is_unsupported_like_upstream() {
        let t = MockTransport::new()
            .expect("*idn?", "*idn?\rSC10 ver1.0")
            .expect("mode=1", "mode=1\r1");
        let mut d = ThorlabsSC10::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.fire(10.0).unwrap_err(), MmError::UnsupportedCommand);
        assert!(!d.get_open().unwrap());
    }

    #[test]
    fn initialized_port_change_is_rejected() {
        let mut d = make_device();
        d.initialize().unwrap();
        assert_eq!(
            d.set_property("Port", PropertyValue::String("COM2".into()))
                .unwrap_err(),
            MmError::InvalidProperty
        );
    }

    #[test]
    fn command_property_stores_answer() {
        let t = MockTransport::new()
            .expect("*idn?", "*idn?\rSC10 ver1.0")
            .expect("mode=1", "mode=1\r1")
            .expect("ens?", "ens?\r0");
        let mut d = ThorlabsSC10::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_property("SC10 Command:", PropertyValue::String("ens?".into()))
            .unwrap();
        assert_eq!(
            d.get_property("SC10 Command:").unwrap(),
            PropertyValue::String("0".into())
        );
    }

    #[test]
    fn command_property_strips_echo_and_prompt() {
        let t = MockTransport::new()
            .expect("*idn?", "*idn?\rSC10 ver1.0>")
            .expect("mode=1", "mode=1\r1>")
            .expect("ens?", " ens?\r0>");
        let mut d = ThorlabsSC10::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_property("SC10 Command:", PropertyValue::String("ens?".into()))
            .unwrap();
        assert_eq!(
            d.get_property("SC10 Command:").unwrap(),
            PropertyValue::String("0".into())
        );
    }

    #[test]
    fn command_property_rejects_missing_echo() {
        let t = MockTransport::new()
            .expect("*idn?", "*idn?\rSC10 ver1.0")
            .expect("mode=1", "mode=1\r1")
            .expect("ens?", "0");
        let mut d = ThorlabsSC10::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(
            d.set_property("SC10 Command:", PropertyValue::String("ens?".into()))
                .unwrap_err(),
            MmError::SerialCommandFailed
        );
    }

    #[test]
    fn device_type_is_shutter() {
        assert_eq!(ThorlabsSC10::new().device_type(), DeviceType::Shutter);
    }
}
