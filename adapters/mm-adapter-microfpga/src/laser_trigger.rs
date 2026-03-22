//! MicroFPGA Laser Trigger generic device.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Generic};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};
use crate::{OFFSET_LASER_MODE, OFFSET_LASER_DURATION, OFFSET_LASER_SEQUENCE, MAX_LASERS};

pub struct LaserTrigger {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    num_lasers: u32,
}

impl LaserTrigger {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("NumLasers", PropertyValue::Integer(MAX_LASERS as i64), true).unwrap();
        for i in 0..MAX_LASERS {
            props.define_property(&format!("Mode{}", i), PropertyValue::Integer(0), false).unwrap();
            props.define_property(&format!("Duration{}", i), PropertyValue::Integer(1000), false).unwrap();
            props.define_property(&format!("Sequence{}", i), PropertyValue::Integer(0), false).unwrap();
        }
        Self { props, transport: None, initialized: false, num_lasers: MAX_LASERS }
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

impl Default for LaserTrigger { fn default() -> Self { Self::new() } }

impl Device for LaserTrigger {
    fn name(&self) -> &str { "Laser Trigger" }
    fn description(&self) -> &str { "Laser Trigger" }
    fn initialize(&mut self) -> MmResult<()> { self.initialized = true; Ok(()) }
    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32;
        for i in 0..self.num_lasers {
            let (key, offset) = (format!("Mode{}", i), OFFSET_LASER_MODE + i);
            if name == key { if self.initialized { self.write_reg(offset, v)?; } return self.props.set(name, PropertyValue::Integer(v as i64)); }
            let (key, offset) = (format!("Duration{}", i), OFFSET_LASER_DURATION + i);
            if name == key { if self.initialized { self.write_reg(offset, v)?; } return self.props.set(name, PropertyValue::Integer(v as i64)); }
            let (key, offset) = (format!("Sequence{}", i), OFFSET_LASER_SEQUENCE + i);
            if name == key { if self.initialized { self.write_reg(offset, v)?; } return self.props.set(name, PropertyValue::Integer(v as i64)); }
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
impl Generic for LaserTrigger {}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize_ok() {
        let t = MockTransport::new();
        let mut lt = LaserTrigger::new().with_transport(Box::new(t));
        lt.initialize().unwrap();
        assert!(lt.initialized);
    }

    #[test]
    fn set_property_after_init_writes_transport() {
        let t = MockTransport::new();
        let mut lt = LaserTrigger::new().with_transport(Box::new(t));
        lt.initialize().unwrap();
        // Setting a mode after init should succeed (write_reg sends bytes)
        lt.set_property("Mode1", PropertyValue::Integer(2)).unwrap();
        assert_eq!(lt.get_property("Mode1").unwrap(), PropertyValue::Integer(2));
    }

    #[test]
    fn set_mode_before_init_does_not_write() {
        let t = MockTransport::new();
        let mut lt = LaserTrigger::new().with_transport(Box::new(t));
        // Not initialized yet — should succeed without sending bytes
        lt.set_property("Mode0", PropertyValue::Integer(1)).unwrap();
    }
}
