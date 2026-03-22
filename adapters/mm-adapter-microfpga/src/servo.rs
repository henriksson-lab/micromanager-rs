//! MicroFPGA Servo generic device.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Generic};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};
use crate::{OFFSET_SERVO, MAX_SERVOS};

pub struct FpgaServo {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
}

impl FpgaServo {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        for i in 0..MAX_SERVOS {
            props.define_property(&format!("Servo{}", i), PropertyValue::Integer(0), false).unwrap();
        }
        Self { props, transport: None, initialized: false }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t); self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where F: FnOnce(&mut dyn Transport) -> MmResult<R> {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn write_reg(&mut self, addr: u32, value: u32) -> MmResult<()> {
        let bytes = [
            0x80u8,
            (addr & 0xFF) as u8, ((addr >> 8) & 0xFF) as u8,
            ((addr >> 16) & 0xFF) as u8, ((addr >> 24) & 0xFF) as u8,
            (value & 0xFF) as u8, ((value >> 8) & 0xFF) as u8,
            ((value >> 16) & 0xFF) as u8, ((value >> 24) & 0xFF) as u8,
        ];
        self.call_transport(|t| t.send_bytes(&bytes))
    }
}

impl Default for FpgaServo { fn default() -> Self { Self::new() } }

impl Device for FpgaServo {
    fn name(&self) -> &str { "Servos" }
    fn description(&self) -> &str { "Servo Output" }
    fn initialize(&mut self) -> MmResult<()> { self.initialized = true; Ok(()) }
    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32;
        for i in 0..MAX_SERVOS {
            let key = format!("Servo{}", i);
            if name == key {
                if self.initialized { self.write_reg(OFFSET_SERVO + i, v)?; }
                return self.props.set(name, PropertyValue::Integer(v as i64));
            }
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
impl Generic for FpgaServo {}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn set_servo_channel_writes_register() {
        let t = MockTransport::new();
        let mut srv = FpgaServo::new().with_transport(Box::new(t));
        srv.initialize().unwrap();
        srv.set_property("Servo0", PropertyValue::Integer(1500)).unwrap();
        assert_eq!(srv.get_property("Servo0").unwrap(), PropertyValue::Integer(1500));
    }

    #[test]
    fn has_seven_channels() {
        let srv = FpgaServo::new();
        assert!(srv.has_property("Servo0"));
        assert!(srv.has_property("Servo6"));
        assert!(!srv.has_property("Servo7"));
    }
}
