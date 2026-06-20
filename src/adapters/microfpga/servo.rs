//! MicroFPGA Servo generic device.
use super::{read_register, write_register, MAX_SERVOS, OFFSET_SERVO};
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use parking_lot::Mutex;

pub struct FpgaServo {
    props: PropertyMap,
    transport: Mutex<Option<Box<dyn Transport>>>,
    initialized: bool,
    num_servos: u32,
}

impl FpgaServo {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Number of Servos", PropertyValue::Integer(4))
            .unwrap();
        props
            .set_property_limits("Number of Servos", 1.0, MAX_SERVOS as f64)
            .unwrap();
        for i in 0..4 {
            let name = format!("Position{}", i);
            props
                .define_property(&name, PropertyValue::Integer(0), false)
                .unwrap();
            props.set_property_limits(&name, 0.0, 65535.0).unwrap();
        }
        Self {
            props,
            transport: Mutex::new(None),
            initialized: false,
            num_servos: 4,
        }
    }

    fn define_servo_property(props: &mut PropertyMap, i: u32) -> MmResult<()> {
        let name = format!("Position{}", i);
        props.define_property(&name, PropertyValue::Integer(0), false)?;
        props.set_property_limits(&name, 0.0, 65535.0)
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
        for i in 0..self.num_servos {
            if name == format!("Position{}", i) {
                return Ok(Some(PropertyValue::Integer(
                    self.read_reg(OFFSET_SERVO + i)? as i64,
                )));
            }
        }
        Ok(None)
    }
}

impl Default for FpgaServo {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for FpgaServo {
    fn name(&self) -> &str {
        "Servos"
    }
    fn description(&self) -> &str {
        "Servo Output"
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
        if name == "Number of Servos" {
            let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32;
            if !(1..=MAX_SERVOS).contains(&v) {
                return Err(MmError::InvalidPropertyValue);
            }
            for i in 0..v {
                if !self.props.has_property(&format!("Position{}", i)) {
                    Self::define_servo_property(&mut self.props, i)?;
                }
            }
            self.num_servos = v;
            return self.props.set(name, PropertyValue::Integer(v as i64));
        }
        let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32;
        for i in 0..self.num_servos {
            let key = format!("Position{}", i);
            if name == key {
                if self.initialized {
                    self.write_reg(OFFSET_SERVO + i, v)?;
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
impl Generic for FpgaServo {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn set_servo_channel_writes_register() {
        let t = MockTransport::new().expect_binary(&1500u32.to_le_bytes());
        let mut srv = FpgaServo::new().with_transport(Box::new(t));
        srv.initialize().unwrap();
        srv.set_property("Position0", PropertyValue::Integer(1500))
            .unwrap();
        assert_eq!(
            srv.get_property("Position0").unwrap(),
            PropertyValue::Integer(1500)
        );
    }

    #[test]
    fn has_seven_channels() {
        let srv = FpgaServo::new();
        assert!(srv.has_property("Position0"));
        assert!(srv.has_property("Position3"));
        assert!(!srv.has_property("Position4"));
    }

    #[test]
    fn get_position_reads_live_register_after_initialize() {
        let t = MockTransport::new().expect_binary(&1600u32.to_le_bytes());
        let mut srv = FpgaServo::new().with_transport(Box::new(t));
        srv.initialize().unwrap();

        assert_eq!(
            srv.get_property("Position0").unwrap(),
            PropertyValue::Integer(1600)
        );
    }
}
