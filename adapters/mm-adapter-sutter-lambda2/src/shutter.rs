/// Sutter Lambda 2 shutter — binary serial protocol.
///
/// Shutter A open:  send `[0xAA]`, response echo + CR
/// Shutter A close: send `[0xAC]`, response echo + CR
/// Shutter B open:  send `[0xBA]`, response echo + CR
/// Shutter B close: send `[0xBC]`, response echo + CR
///
/// (Same byte codes as Lambda 10-2 but the hub handles communication.)
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutterId { A, B }

pub struct Lambda2Shutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    shutter: ShutterId,
    is_open: bool,
}

impl Lambda2Shutter {
    pub fn new(shutter: ShutterId) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        let sname = match shutter { ShutterId::A => "A", ShutterId::B => "B" };
        props.define_property("Shutter", PropertyValue::String(sname.into()), true).unwrap();
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
        let cmd_byte: u8 = match (shutter, open) {
            (ShutterId::A, true)  => 0xAA,
            (ShutterId::A, false) => 0xAC,
            (ShutterId::B, true)  => 0xBA,
            (ShutterId::B, false) => 0xBC,
        };
        self.call_transport(|t| {
            t.send_bytes(&[cmd_byte])?;
            // Controller echoes the command byte then sends CR (0x0D)
            let resp = t.receive_bytes(2)?;
            if resp.last() != Some(&0x0D) {
                return Err(MmError::SerialInvalidResponse);
            }
            Ok(())
        })
    }
}

impl Device for Lambda2Shutter {
    fn name(&self) -> &str { "Lambda2Shutter" }
    fn description(&self) -> &str { "Sutter Lambda 2 shutter" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.send_shutter_cmd(false)?;
        self.is_open = false;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send_shutter_cmd(false);
            self.is_open = false;
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

impl Shutter for Lambda2Shutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.send_shutter_cmd(open)?;
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
    fn shutter_a_open_close() {
        let t = MockTransport::new()
            .expect_binary(&[0xAC, 0x0D])   // init → close
            .expect_binary(&[0xAA, 0x0D])   // open
            .expect_binary(&[0xAC, 0x0D]);  // close
        let mut s = Lambda2Shutter::new(ShutterId::A).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn shutter_b_open() {
        let t = MockTransport::new()
            .expect_binary(&[0xBC, 0x0D])
            .expect_binary(&[0xBA, 0x0D]);
        let mut s = Lambda2Shutter::new(ShutterId::B).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn fire_opens_then_closes() {
        let t = MockTransport::new()
            .expect_binary(&[0xAC, 0x0D])   // init close
            .expect_binary(&[0xAA, 0x0D])   // fire open
            .expect_binary(&[0xAC, 0x0D]);  // fire close
        let mut s = Lambda2Shutter::new(ShutterId::A).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.fire(10.0).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() {
        let mut s = Lambda2Shutter::new(ShutterId::A);
        assert!(s.initialize().is_err());
    }
}
