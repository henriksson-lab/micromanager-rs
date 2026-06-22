/// Sutter Lambda shutter — binary serial protocol.
///
/// Binary protocol:
///   Shutter A open:  send `[0xAA]`  → response `[0xAA|0xAC, 0x0D]`
///   Shutter A close: send `[0xAC]`  → response `[0xAC|0xAA, 0x0D]`
///   Shutter B open:  send `[0xBA]`  → response `[0xBA|0xBC, 0x0D]`
///   Shutter B close: send `[0xBC]`  → response `[0xBC|0xBA, 0x0D]`
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

/// Which shutter on the Lambda controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutterId {
    A,
    B,
}

pub struct LambdaShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    shutter: ShutterId,
    is_open: bool,
}

impl LambdaShutter {
    pub fn new(shutter: ShutterId) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        let shutter_name = match shutter {
            ShutterId::A => "A",
            ShutterId::B => "B",
        };
        props
            .define_property("Shutter", PropertyValue::String(shutter_name.into()), true)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            shutter,
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

    fn send_shutter_cmd(&mut self, open: bool) -> MmResult<()> {
        let shutter = self.shutter;
        let (cmd_byte, alternate_echo): (u8, u8) = match (shutter, open) {
            (ShutterId::A, true) => (0xAA, 0xAC),
            (ShutterId::A, false) => (0xAC, 0xAA),
            (ShutterId::B, true) => (0xBA, 0xBC),
            (ShutterId::B, false) => (0xBC, 0xBA),
        };
        self.call_transport(|t| {
            t.send_bytes(&[cmd_byte])?;
            let resp = t.receive_bytes(2)?;
            if resp.len() != 2
                || (resp[0] != cmd_byte && resp[0] != alternate_echo)
                || resp[1] != 0x0D
            {
                return Err(MmError::SerialInvalidResponse);
            }
            Ok(())
        })
    }
}

impl Device for LambdaShutter {
    fn name(&self) -> &str {
        match self.shutter {
            ShutterId::A => "Shutter-A",
            ShutterId::B => "Shutter-B",
        }
    }
    fn description(&self) -> &str {
        "Sutter Lambda shutter adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        if !self.props.has_property("State") {
            self.props
                .define_property("State", PropertyValue::Integer(0), false)?;
            self.props.set_allowed_values("State", &["0", "1"])?;
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
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
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

impl Shutter for LambdaShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.send_shutter_cmd(open)?;
        self.is_open = open;
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
    fn shutter_a_open_close() {
        let t = MockTransport::new()
            .expect_binary(&[0xAA, 0x0D]) // open
            .expect_binary(&[0xAC, 0x0D]); // close
        let mut s = LambdaShutter::new(ShutterId::A).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn shutter_b_open_close() {
        let t = MockTransport::new().expect_binary(&[0xBA, 0x0D]);
        let mut s = LambdaShutter::new(ShutterId::B).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn accepts_inverted_echo_without_mutating_on_bad_echo() {
        let t = MockTransport::new()
            .expect_binary(&[0xAC, 0x0D])
            .expect_binary(&[0xBA, 0x0D]);
        let mut s = LambdaShutter::new(ShutterId::A).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        assert_eq!(s.set_open(false), Err(MmError::SerialInvalidResponse));
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn initialized_port_change_is_rejected_and_preserved() {
        let t = MockTransport::new();
        let mut s = LambdaShutter::new(ShutterId::A).with_transport(Box::new(t));
        s.set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        s.initialize().unwrap();
        assert_eq!(
            s.set_property("Port", PropertyValue::String("COM2".into())),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            s.get_property("Port").unwrap(),
            PropertyValue::String("COM1".into())
        );
    }

    #[test]
    fn no_transport_error() {
        let mut s = LambdaShutter::new(ShutterId::A);
        assert!(s.initialize().is_err());
    }

    #[test]
    fn fire_is_unsupported() {
        let t = MockTransport::new();
        let mut s = LambdaShutter::new(ShutterId::A).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.fire(10.0), Err(MmError::UnsupportedCommand));
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn initialize_and_shutdown_do_not_force_close() {
        let t = MockTransport::new();
        let mut s = LambdaShutter::new(ShutterId::A).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.shutdown().unwrap();
        assert!(!s.initialized);
    }

    #[test]
    fn state_property_is_created_on_initialize_and_routes_to_shutter_command() {
        let t = MockTransport::new().expect_binary(&[0xAA, 0x0D]);
        let mut s = LambdaShutter::new(ShutterId::A).with_transport(Box::new(t));
        assert!(!s.has_property("State"));
        s.initialize().unwrap();
        assert!(s.has_property("State"));
        assert_eq!(s.get_property("State").unwrap(), PropertyValue::Integer(0));
        s.set_property("State", PropertyValue::Integer(1)).unwrap();
        assert!(s.get_open().unwrap());
        assert_eq!(s.get_property("State").unwrap(), PropertyValue::Integer(1));
    }

    #[test]
    fn bad_state_property_write_does_not_mutate_cached_state() {
        let t = MockTransport::new();
        let mut s = LambdaShutter::new(ShutterId::A).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.set_property("State", PropertyValue::Integer(2)),
            Err(MmError::InvalidPropertyValue)
        );
        assert!(!s.get_open().unwrap());
    }
}
