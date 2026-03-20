/// Neos Technologies acousto-optic device shutter controller.
///
/// Protocol (TX `\r`, NO response from device):
///   `CH <1-8>\r`     → select channel
///   `ON\r`           → open (enable) shutter
///   `OFF\r`          → close (disable) shutter
///   `AM <0-1023>\r`  → set amplitude/intensity (0–1023)
///
/// Device provides no acknowledgement; state is tracked internally.
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Shutter};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

pub struct NeosShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    channel: u8,
    amplitude: u16,
    is_open: bool,
}

impl NeosShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Channel", PropertyValue::Integer(1), false).unwrap();
        props.set_property_limits("Channel", 1.0, 8.0).unwrap();
        props.define_property("Amplitude", PropertyValue::Integer(512), false).unwrap();
        props.set_property_limits("Amplitude", 0.0, 1023.0).unwrap();
        Self { props, transport: None, initialized: false, channel: 1, amplitude: 512, is_open: false }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where F: FnOnce(&mut dyn Transport) -> MmResult<R> {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn send(&mut self, command: &str) -> MmResult<()> {
        let c = format!("{}\r", command);
        self.call_transport(|t| t.send(&c))
    }
}

impl Default for NeosShutter { fn default() -> Self { Self::new() } }

impl Device for NeosShutter {
    fn name(&self) -> &str { "NeosShutter" }
    fn description(&self) -> &str { "Neos Technologies AO shutter controller" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        self.send(&format!("CH {}", self.channel))?;
        self.send(&format!("AM {}", self.amplitude))?;
        self.send("OFF")?;
        self.is_open = false;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send("OFF");
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Channel" {
            let ch = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u8;
            if self.initialized { self.send(&format!("CH {}", ch))?; }
            self.channel = ch;
            return self.props.set(name, PropertyValue::Integer(ch as i64));
        }
        if name == "Amplitude" {
            let amp = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u16;
            if self.initialized { self.send(&format!("AM {}", amp))?; }
            self.amplitude = amp;
            return self.props.set(name, PropertyValue::Integer(amp as i64));
        }
        self.props.set(name, val)
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Shutter }
    fn busy(&self) -> bool { false }
}

impl Shutter for NeosShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.send(if open { "ON" } else { "OFF" })?;
        self.is_open = open;
        Ok(())
    }
    fn get_open(&self) -> MmResult<bool> { Ok(self.is_open) }
    fn fire(&mut self, _dt: f64) -> MmResult<()> { self.set_open(true) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    #[test]
    fn initialize() {
        // 3 sends: CH 1, AM 512, OFF — no responses
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
        s.set_property("Channel", PropertyValue::Integer(3)).unwrap();
        assert_eq!(s.channel, 3);
    }

    #[test]
    fn set_amplitude() {
        let t = MockTransport::new();
        let mut s = NeosShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("Amplitude", PropertyValue::Integer(800)).unwrap();
        assert_eq!(s.amplitude, 800);
    }

    #[test]
    fn no_transport_error() { assert!(NeosShutter::new().initialize().is_err()); }
}
