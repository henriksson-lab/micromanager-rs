//! MicroFPGA Analog Input generic device.
use super::{read_register, MAX_ANALOG_INPUT, OFFSET_ANALOG_INPUT};
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use parking_lot::Mutex;

pub struct AnalogInput {
    props: PropertyMap,
    transport: Mutex<Option<Box<dyn Transport>>>,
    initialized: bool,
    num_channels: u32,
}

impl AnalogInput {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Number of channels", PropertyValue::Integer(3))
            .unwrap();
        props
            .set_property_limits("Number of channels", 1.0, MAX_ANALOG_INPUT as f64)
            .unwrap();
        for i in 0..3 {
            props
                .define_property(
                    &format!("AnalogInput{}", i),
                    PropertyValue::Integer(0),
                    true,
                )
                .unwrap();
        }
        Self {
            props,
            transport: Mutex::new(None),
            initialized: false,
            num_channels: 3,
        }
    }

    fn define_input_property(props: &mut PropertyMap, i: u32) -> MmResult<()> {
        props.define_property(
            &format!("AnalogInput{}", i),
            PropertyValue::Integer(0),
            true,
        )
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

    fn read_reg(&self, addr: u32) -> MmResult<u32> {
        self.call_transport(|t| read_register(t, addr))
    }

    fn live_input_property(&self, name: &str) -> MmResult<Option<PropertyValue>> {
        if !self.initialized {
            return Ok(None);
        }
        for i in 0..self.num_channels {
            if name == format!("AnalogInput{}", i) {
                return Ok(Some(PropertyValue::Integer(
                    self.read_reg(OFFSET_ANALOG_INPUT + i)? as i64,
                )));
            }
        }
        Ok(None)
    }

    pub fn refresh(&mut self) -> MmResult<()> {
        for i in 0..self.num_channels {
            let v = self.read_reg(OFFSET_ANALOG_INPUT + i)?;
            self.props
                .entry_mut(&format!("AnalogInput{}", i))
                .map(|e| e.value = PropertyValue::Integer(v as i64));
        }
        Ok(())
    }
}

impl Default for AnalogInput {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for AnalogInput {
    fn name(&self) -> &str {
        "Analog Input"
    }
    fn description(&self) -> &str {
        "Analog Input"
    }
    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.lock().is_none() {
            return Err(MmError::NotConnected);
        }
        self.initialized = true;
        self.refresh()
    }
    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if let Some(value) = self.live_input_property(name)? {
            return Ok(value);
        }
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Number of channels" {
            let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32;
            if !(1..=MAX_ANALOG_INPUT).contains(&v) {
                return Err(MmError::InvalidPropertyValue);
            }
            for i in 0..v {
                if !self.props.has_property(&format!("AnalogInput{}", i)) {
                    Self::define_input_property(&mut self.props, i)?;
                }
            }
            self.num_channels = v;
            return self.props.set(name, PropertyValue::Integer(v as i64));
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
impl Generic for AnalogInput {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn le4(v: u32) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }

    #[test]
    fn initialize_reads_all_channels() {
        // Each read_reg call triggers one receive_bytes(4).
        let mut t = MockTransport::new();
        for i in 0..3 {
            t = t.expect_binary(&le4(i * 100));
        }
        let mut ai = AnalogInput::new().with_transport(Box::new(t));
        ai.initialize().unwrap();
        assert!(ai.initialized);
    }

    #[test]
    fn no_transport_returns_error() {
        let mut ai = AnalogInput::new();
        assert!(ai.initialize().is_err());
    }

    #[test]
    fn properties_are_read_only() {
        let ai = AnalogInput::new();
        assert!(ai.is_property_read_only("AnalogInput0"));
        assert!(ai.is_property_read_only("AnalogInput2"));
    }

    #[test]
    fn get_property_reads_live_register_after_initialize() {
        let t = MockTransport::new()
            .expect_binary(&le4(0))
            .expect_binary(&le4(0))
            .expect_binary(&le4(0))
            .expect_binary(&le4(987));
        let mut ai = AnalogInput::new().with_transport(Box::new(t));
        ai.initialize().unwrap();

        assert_eq!(
            ai.get_property("AnalogInput0").unwrap(),
            PropertyValue::Integer(987)
        );
    }
}
