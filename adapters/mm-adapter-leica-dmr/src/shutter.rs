/// Leica DMR reflected light shutter.
///
/// Protocol:
///   Set shutter open:  device=rLFA(8 or 9), command=12, data=1  → `"<DD>0121\r"`
///   Set shutter close: device=rLFA(8 or 9), command=12, data=0  → `"<DD>0120\r"`
///   Get shutter state: device=rLFA, command=13                  → `"<DD>013<0|1>\r"`
///
/// The rLFA device id defaults to 8 (4-position turret).
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct LeicaDMRShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    device_id: u8,
    is_open: bool,
}

impl LeicaDMRShutter {
    /// `device_id`: rLFA4 = 8, rLFA8 = 9
    pub fn new(device_id: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("DeviceID", PropertyValue::Integer(device_id as i64), true).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            device_id,
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

    fn send_recv(&mut self, cmd: &str) -> MmResult<String> {
        self.call_transport(|t| Ok(t.send_recv(cmd)?.trim().to_string()))
    }

    fn send_open_cmd(&mut self, open: bool) -> MmResult<()> {
        let dev = self.device_id;
        let val = if open { 1 } else { 0 };
        let cmd = format!("{:02}012{}\r", dev, val);
        let resp = self.send_recv(&cmd)?;
        let prefix = format!("{:02}012", dev);
        if !resp.starts_with(&prefix) {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(())
    }
}

impl Device for LeicaDMRShutter {
    fn name(&self) -> &str { "LeicaDMRShutter" }
    fn description(&self) -> &str { "Leica DMR reflected light shutter" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.send_open_cmd(false)?;
        self.is_open = false;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send_open_cmd(false);
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

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Shutter }
    fn busy(&self) -> bool { false }
}

impl Shutter for LeicaDMRShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.send_open_cmd(open)?;
        self.is_open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.is_open) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        self.set_open(true)?;
        self.set_open(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn shutter_open_close() {
        // device_id=8 (rLFA4), command=12
        let t = MockTransport::new()
            .expect("080120\r", "080120")   // init: close
            .expect("080121\r", "080121")   // open
            .expect("080120\r", "080120");  // close
        let mut s = LeicaDMRShutter::new(8).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn fire_opens_then_closes() {
        let t = MockTransport::new()
            .expect("080120\r", "080120")
            .expect("080121\r", "080121")
            .expect("080120\r", "080120");
        let mut s = LeicaDMRShutter::new(8).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.fire(5.0).unwrap();
        assert!(!s.get_open().unwrap());
    }
}
