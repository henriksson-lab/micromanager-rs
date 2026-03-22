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
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

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
            Ok(resp.trim().to_string())
        })
    }
}

impl Default for ThorlabsSC10 {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ThorlabsSC10 {
    fn name(&self) -> &str {
        "ThorlabsSC10"
    }

    fn description(&self) -> &str {
        "Thorlabs SC10 shutter controller"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Query device identity (retry once if the first attempt fails, per C++ original)
        let _idn = self.cmd("*idn?").or_else(|_| self.cmd("*idn?"))?;
        // Set manual mode — required for normal shutter operation
        let _ = self.cmd("mode=1")?;
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

impl Shutter for ThorlabsSC10 {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        // Only toggle if state differs
        let current = self.get_open()?;
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
        self.set_open(true)?;
        self.set_open(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_device() -> ThorlabsSC10 {
        // init: *idn? → "SC10 ver1.0", mode=1 → "1"
        let t = MockTransport::new()
            .expect("*idn?", "SC10 ver1.0")
            .expect("mode=1", "1");
        ThorlabsSC10::new().with_transport(Box::new(t))
    }

    #[test]
    fn initialize_succeeds() {
        let mut d = make_device();
        d.initialize().unwrap();
        assert!(d.initialized);
    }

    #[test]
    fn no_transport_errors() {
        assert!(ThorlabsSC10::new().initialize().is_err());
    }

    #[test]
    fn set_open_toggles_once() {
        let t = MockTransport::new()
            .expect("*idn?", "SC10 ver1.0")
            .expect("mode=1", "1")
            .expect("ens", "1"); // one toggle to open
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
            .expect("*idn?", "SC10 ver1.0")
            .expect("mode=1", "1");
        let mut d = ThorlabsSC10::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        // Already closed; closing again should not send "ens"
        d.set_open(false).unwrap();
        assert!(!d.get_open().unwrap());
    }

    #[test]
    fn fire_opens_then_closes() {
        let t = MockTransport::new()
            .expect("*idn?", "SC10 ver1.0")
            .expect("mode=1", "1")
            .expect("ens", "1") // open
            .expect("ens", "0"); // close
        let mut d = ThorlabsSC10::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.fire(10.0).unwrap();
        assert!(!d.get_open().unwrap());
    }

    #[test]
    fn device_type_is_shutter() {
        assert_eq!(ThorlabsSC10::new().device_type(), DeviceType::Shutter);
    }
}
