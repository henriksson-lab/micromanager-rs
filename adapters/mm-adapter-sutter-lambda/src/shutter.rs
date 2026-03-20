/// Sutter Lambda shutter — binary serial protocol.
///
/// Binary protocol:
///   Shutter A open:  send `[0xAA]`  → response `[0x0D]`
///   Shutter A close: send `[0xAC]`  → response `[0x0D]`
///   Shutter B open:  send `[0xBA]`  → response `[0x0D]`
///   Shutter B close: send `[0xBC]`  → response `[0x0D]`
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Shutter};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

/// Which shutter on the Lambda controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutterId { A, B }

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
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        let shutter_name = match shutter { ShutterId::A => "A", ShutterId::B => "B" };
        props.define_property("Shutter", PropertyValue::String(shutter_name.into()), true).unwrap();

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
            let resp = t.receive_bytes(1)?;
            if resp.first() != Some(&0x0D) {
                return Err(MmError::SerialInvalidResponse);
            }
            Ok(())
        })
    }
}

impl Device for LambdaShutter {
    fn name(&self) -> &str { "LambdaShutter" }
    fn description(&self) -> &str { "Sutter Lambda shutter" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Close shutter on init
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

impl Shutter for LambdaShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.send_shutter_cmd(open)?;
        self.is_open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.is_open) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        self.set_open(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    #[test]
    fn shutter_a_open_close() {
        let t = MockTransport::new()
            .expect_binary(&[0x0D])   // init → close (0xAC)
            .expect_binary(&[0x0D])   // open (0xAA)
            .expect_binary(&[0x0D]);  // close (0xAC)
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
        let t = MockTransport::new()
            .expect_binary(&[0x0D])   // init → close (0xBC)
            .expect_binary(&[0x0D]);  // open (0xBA)
        let mut s = LambdaShutter::new(ShutterId::B).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() {
        let mut s = LambdaShutter::new(ShutterId::A);
        assert!(s.initialize().is_err());
    }
}
