//! ArduinoCounter — Generic device for the Arduino pulse-counter firmware.
//!
//! The firmware responds to ASCII commands:
//!   `i`      → "ArduinoCounter ... 2.0\r\n"  (identification + version)
//!   `g<N>\n` → ack line (start counting N pulses; N=0 means run until stopped)
//!   `s`      → ack line (stop counting)
//!   `p?`     → "Direct\r\n" or "Invert\r\n"
//!   `pi`     → "Invert\r\n"
//!   `pd`     → "Direct\r\n"

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Generic};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const VERSION_MIN: f64 = 2.0;
const VERSION_MAX: f64 = 3.0;

pub struct ArduinoCounter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    version: f64,
    inverted: bool,
}

impl ArduinoCounter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Version", PropertyValue::Float(0.0), true).unwrap();
        props.define_property("OutputLogic", PropertyValue::String("Direct".into()), false).unwrap();
        props.set_allowed_values("OutputLogic", &["Direct", "Invert"]).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            version: 0.0,
            inverted: false,
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

    fn send_recv(&mut self, cmd: &str) -> MmResult<String> {
        self.call_transport(|t| Ok(t.send_recv(cmd)?.trim().to_string()))
    }

    /// Start counting `n` pulses (send `g<n>\n`).
    pub fn start_counting(&mut self, n: u32) -> MmResult<()> {
        let cmd = format!("g{}\n", n);
        self.send_recv(&cmd)?;
        Ok(())
    }

    /// Stop counting (send `s`).
    pub fn stop_counting(&mut self) -> MmResult<()> {
        self.send_recv("s")?;
        Ok(())
    }

    /// Set logic polarity.
    pub fn set_logic(&mut self, invert: bool) -> MmResult<()> {
        let cmd = if invert { "pi" } else { "pd" };
        let resp = self.send_recv(cmd)?;
        let expected = if invert { "Invert" } else { "Direct" };
        if !resp.starts_with(expected) {
            return Err(MmError::SerialInvalidResponse);
        }
        self.inverted = invert;
        Ok(())
    }

    pub fn version(&self) -> f64 { self.version }
}

impl Default for ArduinoCounter {
    fn default() -> Self { Self::new() }
}

impl Device for ArduinoCounter {
    fn name(&self) -> &str { "ArduinoCounter" }
    fn description(&self) -> &str { "Arduino pulse counter device" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }

        // Identify: send `i`, expect "ArduinoCounter ... <version>"
        let resp = self.send_recv("i")?;

        if !resp.starts_with("ArduinoCounter") {
            return Err(MmError::LocallyDefined("Board not found or wrong firmware".into()));
        }

        // Parse version from tail of the response string (last numeric token)
        let ver: f64 = resp.split_whitespace()
            .filter_map(|tok| tok.parse().ok())
            .last()
            .unwrap_or(0.0);

        if ver < VERSION_MIN || ver > VERSION_MAX {
            return Err(MmError::LocallyDefined(format!(
                "Firmware version {} not supported (expected {}-{})",
                ver, VERSION_MIN, VERSION_MAX
            )));
        }

        self.version = ver;
        self.props.entry_mut("Version").map(|e| e.value = PropertyValue::Float(ver));

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.stop_counting();
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "OutputLogic" && self.initialized {
            self.set_logic(val.as_str() == "Invert")?;
        }
        self.props.set(name, val)
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Generic }
    fn busy(&self) -> bool { false }
}

impl Generic for ArduinoCounter {}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_counter() -> ArduinoCounter {
        let t = MockTransport::new()
            .expect("i", "ArduinoCounter firmware version 2.0");
        ArduinoCounter::new().with_transport(Box::new(t))
    }

    #[test]
    fn initialize_ok() {
        let mut c = make_counter();
        c.initialize().unwrap();
        assert!((c.version() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn bad_firmware_rejected() {
        let t = MockTransport::new().any("WrongDevice 1.0");
        let mut c = ArduinoCounter::new().with_transport(Box::new(t));
        assert!(c.initialize().is_err());
    }

    #[test]
    fn start_stop_counting() {
        let t = MockTransport::new()
            .expect("i", "ArduinoCounter firmware version 2.0")
            .expect("g10\n", "ok")
            .expect("s", "ok");
        let mut c = ArduinoCounter::new().with_transport(Box::new(t));
        c.initialize().unwrap();
        c.start_counting(10).unwrap();
        c.stop_counting().unwrap();
    }
}
