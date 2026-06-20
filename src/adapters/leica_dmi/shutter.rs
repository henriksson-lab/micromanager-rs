/// Leica DMI shutter (TL or IL).
///
/// Protocol (ASCII, `\r` terminated):
///   Lamp/shutter device address: "77"
///
///   TL shutter: `77032 0 <0|1>\r`
///   IL shutter: `77032 1 <0|1>\r` on DMI 4000/5000/6000 family
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutterType {
    TransmittedLight, // lamp channel 0
    IncidentLight,    // lamp channel 1
}

pub struct LeicaDMIShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    shutter_type: ShutterType,
    is_open: bool,
}

impl LeicaDMIShutter {
    pub fn new(shutter_type: ShutterType) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        let type_str = match shutter_type {
            ShutterType::TransmittedLight => "TL",
            ShutterType::IncidentLight => "IL",
        };
        props
            .define_property("ShutterType", PropertyValue::String(type_str.into()), true)
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("State", &["0", "1"]).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            shutter_type,
            is_open: false,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn shutter_channel(&self) -> u8 {
        match self.shutter_type {
            ShutterType::TransmittedLight => 0,
            ShutterType::IncidentLight => 1,
        }
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

    fn send_no_response(&mut self, cmd: &str) -> MmResult<()> {
        self.call_transport(|t| t.send(cmd))
    }

    fn send_open(&mut self, open: bool) -> MmResult<()> {
        let dev = "77";
        let channel = self.shutter_channel();
        let val = if open { 1 } else { 0 };
        let cmd = format!("{}032 {} {}\r", dev, channel, val);
        self.send_no_response(&cmd)
    }

    pub fn query_state(&mut self) -> MmResult<bool> {
        let dev = "77";
        let cmd = format!("{}033\r", dev);
        let resp = self.send_recv(&cmd)?;
        let prefix = format!("{}033", dev);
        if !resp.starts_with(&prefix) {
            return Err(MmError::SerialInvalidResponse);
        }
        let val: &str = resp[prefix.len()..].trim();
        Ok(val == "1")
    }
}

impl Device for LeicaDMIShutter {
    fn name(&self) -> &str {
        "LeicaDMIShutter"
    }
    fn description(&self) -> &str {
        "Leica DMI shutter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.send_open(false)?;
        self.is_open = false;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send_open(false);
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

impl Shutter for LeicaDMIShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.send_open(open)?;
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
    fn tl_shutter_open_close() {
        let t = MockTransport::new()
            .expect("77032 0 0\r", "") // init close
            .expect("77032 0 1\r", "") // open
            .expect("77032 0 0\r", ""); // close
        let mut s = LeicaDMIShutter::new(ShutterType::TransmittedLight).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn il_shutter_open_close() {
        let t = MockTransport::new()
            .expect("77032 1 0\r", "")
            .expect("77032 1 1\r", "");
        let mut s = LeicaDMIShutter::new(ShutterType::IncidentLight).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn fire_is_unsupported() {
        let t = MockTransport::new().expect("77032 0 0\r", ""); // init
        let mut s = LeicaDMIShutter::new(ShutterType::TransmittedLight).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.fire(5.0), Err(MmError::UnsupportedCommand));
        assert!(!s.get_open().unwrap());
    }
}
