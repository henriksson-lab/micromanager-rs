//! SangaBoardHub — manages serial port for the Sangaboard.

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Hub};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct SangaBoardHub {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
}

impl SangaBoardHub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("StepDelay", PropertyValue::Integer(1000), false).unwrap();
        props.define_property("RampTime", PropertyValue::Integer(0), false).unwrap();
        Self { props, transport: None, initialized: false }
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

    /// Send a command and read the response line.
    pub fn send_command(&mut self, cmd: &str) -> MmResult<String> {
        self.call_transport(|t| Ok(t.send_recv(cmd)?.trim().to_string()))
    }
}

impl Default for SangaBoardHub {
    fn default() -> Self { Self::new() }
}

impl Device for SangaBoardHub {
    fn name(&self) -> &str { "SangaBoardHub" }
    fn description(&self) -> &str { "Sangaboard Hub" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }

        // Version check
        let resp = self.send_command("version")?;
        if !resp.contains("Sangaboard") {
            return Err(MmError::LocallyDefined(
                "Sangaboard not found — unexpected version response".into(),
            ));
        }

        // Enable non-blocking moves
        let done = self.send_command("blocking_moves false")?;
        if !done.contains("done") {
            return Err(MmError::LocallyDefined(
                "blocking_moves false did not return 'done'".into(),
            ));
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send_command("release");
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "StepDelay" && self.initialized {
            let n = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            let cmd = format!("dt {}", n);
            let _ = self.send_command(&cmd);
        }
        if name == "RampTime" && self.initialized {
            let n = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            let cmd = format!("ramp_time {}", n);
            let _ = self.send_command(&cmd);
        }
        self.props.set(name, val)
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Hub }
    fn busy(&self) -> bool { false }
}

impl Hub for SangaBoardHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        Ok(vec![
            "OFXYStage".into(),
            "OFZStage".into(),
            "OFShutter".into(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize_ok() {
        let t = MockTransport::new()
            .expect("version", "Sangaboard v0.5.1")
            .expect("blocking_moves false", "done");
        let mut hub = SangaBoardHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
    }

    #[test]
    fn wrong_device_rejected() {
        let t = MockTransport::new()
            .expect("version", "UnknownDevice v1.0")
            .any("done");
        let mut hub = SangaBoardHub::new().with_transport(Box::new(t));
        assert!(hub.initialize().is_err());
    }
}
