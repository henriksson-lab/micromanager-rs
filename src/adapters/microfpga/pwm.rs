//! MicroFPGA PWM Output generic device.
use super::{read_register, write_register, MAX_PWM, OFFSET_PWM};
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use parking_lot::Mutex;

pub struct FpgaPwm {
    props: PropertyMap,
    transport: Mutex<Option<Box<dyn Transport>>>,
    initialized: bool,
    num_channels: u32,
}

impl FpgaPwm {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Number of PWM", PropertyValue::Integer(1))
            .unwrap();
        props
            .set_property_limits("Number of PWM", 1.0, MAX_PWM as f64)
            .unwrap();
        for i in 0..1 {
            let name = format!("Position{}", i);
            props
                .define_property(&name, PropertyValue::Integer(0), false)
                .unwrap();
            props.set_property_limits(&name, 0.0, 255.0).unwrap();
        }
        Self {
            props,
            transport: Mutex::new(None),
            initialized: false,
            num_channels: 1,
        }
    }

    fn define_pwm_property(props: &mut PropertyMap, i: u32) -> MmResult<()> {
        let name = format!("Position{}", i);
        props.define_property(&name, PropertyValue::Integer(0), false)?;
        props.set_property_limits(&name, 0.0, 255.0)
    }

    pub fn with_transport(self, t: Box<dyn Transport>) -> Self {
        *self.transport.lock() = Some(t);
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        let mut transport = self.transport.lock();
        match transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn write_reg(&self, addr: u32, value: u32) -> MmResult<()> {
        self.call_transport(|t| write_register(t, addr, value))
    }

    fn read_reg(&self, addr: u32) -> MmResult<u32> {
        self.call_transport(|t| read_register(t, addr))
    }

    fn live_position_property(&self, name: &str) -> MmResult<Option<PropertyValue>> {
        if !self.initialized {
            return Ok(None);
        }
        for i in 0..self.num_channels {
            if name == format!("Position{}", i) {
                return Ok(Some(PropertyValue::Integer(
                    self.read_reg(OFFSET_PWM + i)? as i64
                )));
            }
        }
        Ok(None)
    }
}

impl Default for FpgaPwm {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for FpgaPwm {
    fn name(&self) -> &str {
        "PWM"
    }
    fn description(&self) -> &str {
        "PWM Output"
    }
    fn initialize(&mut self) -> MmResult<()> {
        self.initialized = true;
        Ok(())
    }
    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if let Some(value) = self.live_position_property(name)? {
            return Ok(value);
        }
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Number of PWM" {
            let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32;
            if !(1..=MAX_PWM).contains(&v) {
                return Err(MmError::InvalidPropertyValue);
            }
            for i in 0..v {
                if !self.props.has_property(&format!("Position{}", i)) {
                    Self::define_pwm_property(&mut self.props, i)?;
                }
            }
            self.num_channels = v;
            return self.props.set(name, PropertyValue::Integer(v as i64));
        }
        let mut v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
        if !(0..=255).contains(&v) {
            v = 0;
        }
        let v = v as u32;
        for i in 0..self.num_channels {
            let key = format!("Position{}", i);
            if name == key {
                if self.initialized {
                    self.write_reg(OFFSET_PWM + i, v)?;
                }
                return self.props.set(name, PropertyValue::Integer(v as i64));
            }
        }
        self.props.set(name, val)
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
        DeviceType::Generic
    }
    fn busy(&self) -> bool {
        false
    }
}
impl Generic for FpgaPwm {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn set_pwm_channel_writes_register() {
        let t = MockTransport::new().expect_binary(&255u32.to_le_bytes());
        let mut pwm = FpgaPwm::new().with_transport(Box::new(t));
        pwm.initialize().unwrap();
        pwm.set_property("Position0", PropertyValue::Integer(255))
            .unwrap();
        assert_eq!(
            pwm.get_property("Position0").unwrap(),
            PropertyValue::Integer(255)
        );
    }

    #[test]
    fn set_property_before_init_does_not_write() {
        let t = MockTransport::new();
        let mut pwm = FpgaPwm::new().with_transport(Box::new(t));
        pwm.set_property("Position0", PropertyValue::Integer(100))
            .unwrap();
        assert_eq!(
            pwm.get_property("Position0").unwrap(),
            PropertyValue::Integer(100)
        );
    }

    #[test]
    fn get_position_reads_live_register_after_initialize() {
        let t = MockTransport::new().expect_binary(&77u32.to_le_bytes());
        let mut pwm = FpgaPwm::new().with_transport(Box::new(t));
        pwm.initialize().unwrap();

        assert_eq!(
            pwm.get_property("Position0").unwrap(),
            PropertyValue::Integer(77)
        );
    }
}
