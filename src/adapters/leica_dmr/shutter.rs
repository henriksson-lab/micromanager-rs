/// Leica DMR reflected light shutter.
///
/// Protocol:
///   Set shutter open:  device=rLFA(67 or 12), command=12, data=1 -> `"<DD>0121\r"`
///   Set shutter close: device=rLFA(67 or 12), command=12, data=0 -> `"<DD>0120\r"`
///   Get shutter state: device=rLFA, command=13                  -> `"<DD>013<0|1>\r"`
///
/// The rLFA device id is rLFA4=67 or rLFA8=12 upstream.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct LeicaDMRShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    device_id: u8,
    is_open: bool,
}

impl LeicaDMRShutter {
    /// `device_id`: rLFA4 = 67, rLFA8 = 12
    pub fn new(device_id: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("DeviceID", PropertyValue::Integer(device_id as i64), true)
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("State", &["0", "1"]).unwrap();
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
    fn name(&self) -> &str {
        "Shutter"
    }
    fn description(&self) -> &str {
        "LeicaDMR Reflected Light Shutter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
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
            "State" => Ok(PropertyValue::Integer(if self.is_open { 1 } else { 0 })),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                match state {
                    0 => self.set_open(false),
                    1 => self.set_open(true),
                    _ => Err(MmError::InvalidPropertyValue),
                }
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

impl Shutter for LeicaDMRShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.send_open_cmd(open)?;
        self.is_open = open;
        self.props
            .set("State", PropertyValue::Integer(if open { 1 } else { 0 }))?;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
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

    #[test]
    fn shutter_open_close() {
        // device_id=67 (rLFA4), command=12
        let t = MockTransport::new()
            .expect("670121\r", "670121") // open
            .expect("670120\r", "670120"); // close
        let mut s = LeicaDMRShutter::new(67).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn fire_is_unsupported() {
        let t = MockTransport::new();
        let mut s = LeicaDMRShutter::new(67).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.fire(5.0), Err(MmError::UnsupportedCommand));
        assert!(!s.get_open().unwrap());
    }
}
