//! MicroFPGA PWM Output generic device.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Generic};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};
use crate::{OFFSET_PWM, MAX_PWM};

pub struct FpgaPwm {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
}

impl FpgaPwm {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        for i in 0..MAX_PWM {
            props.define_property(&format!("PWM{}", i), PropertyValue::Integer(0), false).unwrap();
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

impl Default for FpgaPwm { fn default() -> Self { Self::new() } }

impl Device for FpgaPwm {
    fn name(&self) -> &str { "PWM" }
    fn description(&self) -> &str { "PWM Output" }
    fn initialize(&mut self) -> MmResult<()> { self.initialized = true; Ok(()) }
    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32;
        for i in 0..MAX_PWM {
            let key = format!("PWM{}", i);
            if name == key {
                if self.initialized { self.write_reg(OFFSET_PWM + i, v)?; }
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
impl Generic for FpgaPwm {}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn set_pwm_channel_writes_register() {
        let t = MockTransport::new();
        let mut pwm = FpgaPwm::new().with_transport(Box::new(t));
        pwm.initialize().unwrap();
        pwm.set_property("PWM0", PropertyValue::Integer(512)).unwrap();
        assert_eq!(
            pwm.get_property("PWM0").unwrap(),
            PropertyValue::Integer(512)
        );
    }

    #[test]
    fn set_property_before_init_does_not_write() {
        let t = MockTransport::new();
        let mut pwm = FpgaPwm::new().with_transport(Box::new(t));
        pwm.set_property("PWM2", PropertyValue::Integer(100)).unwrap();
        assert_eq!(pwm.get_property("PWM2").unwrap(), PropertyValue::Integer(100));
    }
}
