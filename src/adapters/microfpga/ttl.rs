//! MicroFPGA TTL Output generic device.
use super::{read_register, write_register, MAX_TTL, OFFSET_TTL};
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use parking_lot::Mutex;

pub struct FpgaTtl {
    props: PropertyMap,
    transport: Mutex<Option<Box<dyn Transport>>>,
    initialized: bool,
    num_channels: u32,
}

impl FpgaTtl {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Number of channels", PropertyValue::Integer(4))
            .unwrap();
        props
            .set_property_limits("Number of channels", 1.0, MAX_TTL as f64)
            .unwrap();
        for i in 0..MAX_TTL {
            let name = format!("State{}", i);
            props
                .define_property(&name, PropertyValue::Integer(0), false)
                .unwrap();
            props.set_allowed_values(&name, &["0", "1"]).unwrap();
        }
        Self {
            props,
            transport: Mutex::new(None),
            initialized: false,
            num_channels: 4,
        }
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

    fn live_state_property(&self, name: &str) -> MmResult<Option<PropertyValue>> {
        if !self.initialized {
            return Ok(None);
        }
        for i in 0..self.num_channels {
            if name == format!("State{}", i) {
                return Ok(Some(PropertyValue::Integer(
                    self.read_reg(OFFSET_TTL + i)? as i64
                )));
            }
        }
        Ok(None)
    }
}

impl Default for FpgaTtl {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for FpgaTtl {
    fn name(&self) -> &str {
        "TTL"
    }
    fn description(&self) -> &str {
        "TTL Output"
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
        if let Some(value) = self.live_state_property(name)? {
            return Ok(value);
        }
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Number of channels" {
            let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32;
            if !(1..=MAX_TTL).contains(&v) {
                return Err(MmError::InvalidPropertyValue);
            }
            self.num_channels = v;
            return self.props.set(name, PropertyValue::Integer(v as i64));
        }
        let v = if val.as_i64().ok_or(MmError::InvalidPropertyValue)? == 1 {
            1
        } else {
            0
        };
        for i in 0..self.num_channels {
            let key = format!("State{}", i);
            if name == key {
                if self.initialized {
                    self.write_reg(OFFSET_TTL + i, v)?;
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
impl Generic for FpgaTtl {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn set_ttl_channel_writes_register() {
        let t = MockTransport::new().expect_binary(&1u32.to_le_bytes());
        let mut ttl = FpgaTtl::new().with_transport(Box::new(t));
        ttl.initialize().unwrap();
        ttl.set_property("State0", PropertyValue::Integer(1))
            .unwrap();
        assert_eq!(
            ttl.get_property("State0").unwrap(),
            PropertyValue::Integer(1)
        );
    }

    #[test]
    fn has_four_channels() {
        let ttl = FpgaTtl::new();
        assert!(ttl.has_property("State0"));
        assert!(ttl.has_property("State3"));
        assert!(!ttl.has_property("State4"));
    }

    #[test]
    fn get_state_reads_live_register_after_initialize() {
        let t = MockTransport::new().expect_binary(&1u32.to_le_bytes());
        let mut ttl = FpgaTtl::new().with_transport(Box::new(t));
        ttl.initialize().unwrap();

        assert_eq!(
            ttl.get_property("State0").unwrap(),
            PropertyValue::Integer(1)
        );
    }

    #[test]
    fn set_ttl_non_one_state_writes_zero() {
        let t = MockTransport::new().expect_binary(&0u32.to_le_bytes());
        let mut ttl = FpgaTtl::new().with_transport(Box::new(t));
        ttl.initialize().unwrap();

        ttl.set_property("State0", PropertyValue::Integer(2))
            .unwrap();

        assert_eq!(
            ttl.get_property("State0").unwrap(),
            PropertyValue::Integer(0)
        );
    }
}
