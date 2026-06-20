//! MicroFPGA Laser Trigger generic device.
use super::{
    read_register, write_register, MAX_LASERS, OFFSET_LASER_DURATION, OFFSET_LASER_MODE,
    OFFSET_LASER_SEQUENCE,
};
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use parking_lot::Mutex;

pub struct LaserTrigger {
    props: PropertyMap,
    transport: Mutex<Option<Box<dyn Transport>>>,
    initialized: bool,
    num_lasers: u32,
}

impl LaserTrigger {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Number of lasers", PropertyValue::Integer(4))
            .unwrap();
        props
            .set_property_limits("Number of lasers", 1.0, MAX_LASERS as f64)
            .unwrap();
        for i in 0..4 {
            Self::define_laser_properties(&mut props, i).unwrap();
        }
        Self {
            props,
            transport: Mutex::new(None),
            initialized: false,
            num_lasers: 4,
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

    fn define_laser_properties(props: &mut PropertyMap, i: u32) -> MmResult<()> {
        let duration = format!("Duration{} (us)", i);
        props.define_property(&duration, PropertyValue::Integer(0), false)?;
        props.set_property_limits(&duration, 0.0, 1048575.0)?;

        let mode = format!("Mode{}", i);
        props.define_property(&mode, PropertyValue::String("0 - Off".into()), false)?;
        props.set_allowed_values(
            &mode,
            &[
                "0 - Off",
                "1 - On",
                "2 - Rising",
                "3 - Falling",
                "4 - Follow",
            ],
        )?;

        let sequence = format!("Sequence{}", i);
        props.define_property(&sequence, PropertyValue::Integer(65535), false)?;
        props.set_property_limits(&sequence, 0.0, 65535.0)
    }

    fn mode_value(val: &PropertyValue) -> MmResult<u32> {
        match val {
            PropertyValue::String(s) if s == "0 - Off" => Ok(0),
            PropertyValue::String(s) if s == "1 - On" => Ok(1),
            PropertyValue::String(s) if s == "2 - Rising" => Ok(2),
            PropertyValue::String(s) if s == "3 - Falling" => Ok(3),
            PropertyValue::String(s) if s == "4 - Follow" => Ok(4),
            _ => Err(MmError::InvalidPropertyValue),
        }
    }

    fn mode_label(value: u32) -> MmResult<PropertyValue> {
        let label = match value {
            0 => "0 - Off",
            1 => "1 - On",
            2 => "2 - Rising",
            3 => "3 - Falling",
            4 => "4 - Follow",
            _ => "0 - Off",
        };
        Ok(PropertyValue::String(label.into()))
    }

    fn live_laser_property(&self, name: &str) -> MmResult<Option<PropertyValue>> {
        if !self.initialized {
            return Ok(None);
        }
        for i in 0..self.num_lasers {
            if name == format!("Mode{}", i) {
                return Ok(Some(Self::mode_label(
                    self.read_reg(OFFSET_LASER_MODE + i)?,
                )?));
            }
            if name == format!("Duration{} (us)", i) {
                return Ok(Some(PropertyValue::Integer(
                    self.read_reg(OFFSET_LASER_DURATION + i)? as i64,
                )));
            }
            if name == format!("Sequence{}", i) {
                return Ok(Some(PropertyValue::Integer(
                    self.read_reg(OFFSET_LASER_SEQUENCE + i)? as i64,
                )));
            }
        }
        Ok(None)
    }
}

impl Default for LaserTrigger {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for LaserTrigger {
    fn name(&self) -> &str {
        "Laser Trigger"
    }
    fn description(&self) -> &str {
        "Laser Trigger"
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
        if let Some(value) = self.live_laser_property(name)? {
            return Ok(value);
        }
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Number of lasers" {
            let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32;
            if !(1..=MAX_LASERS).contains(&v) {
                return Err(MmError::InvalidPropertyValue);
            }
            for i in 0..v {
                if !self.props.has_property(&format!("Mode{}", i)) {
                    Self::define_laser_properties(&mut self.props, i)?;
                }
            }
            self.num_lasers = v;
            return self.props.set(name, PropertyValue::Integer(v as i64));
        }
        for i in 0..self.num_lasers {
            let (key, offset) = (format!("Mode{}", i), OFFSET_LASER_MODE + i);
            if name == key {
                let v = Self::mode_value(&val)?;
                if self.initialized {
                    self.write_reg(offset, v)?;
                }
                return self.props.set(name, val);
            }
            let (key, offset) = (format!("Duration{} (us)", i), OFFSET_LASER_DURATION + i);
            if name == key {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32;
                if self.initialized {
                    self.write_reg(offset, v)?;
                }
                return self.props.set(name, PropertyValue::Integer(v as i64));
            }
            let (key, offset) = (format!("Sequence{}", i), OFFSET_LASER_SEQUENCE + i);
            if name == key {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32;
                if self.initialized {
                    self.write_reg(offset, v)?;
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
impl Generic for LaserTrigger {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_ok() {
        let t = MockTransport::new();
        let mut lt = LaserTrigger::new().with_transport(Box::new(t));
        lt.initialize().unwrap();
        assert!(lt.initialized);
    }

    #[test]
    fn set_property_after_init_writes_transport() {
        let t = MockTransport::new().expect_binary(&2u32.to_le_bytes());
        let mut lt = LaserTrigger::new().with_transport(Box::new(t));
        lt.initialize().unwrap();
        // Setting a mode after init should succeed (write_reg sends bytes)
        lt.set_property("Mode1", PropertyValue::String("2 - Rising".into()))
            .unwrap();
        assert_eq!(
            lt.get_property("Mode1").unwrap(),
            PropertyValue::String("2 - Rising".into())
        );
    }

    #[test]
    fn set_mode_before_init_does_not_write() {
        let t = MockTransport::new();
        let mut lt = LaserTrigger::new().with_transport(Box::new(t));
        // Not initialized yet — should succeed without sending bytes
        lt.set_property("Mode0", PropertyValue::String("1 - On".into()))
            .unwrap();
    }

    #[test]
    fn get_mode_reads_live_register_after_initialize() {
        let t = MockTransport::new().expect_binary(&4u32.to_le_bytes());
        let mut lt = LaserTrigger::new().with_transport(Box::new(t));
        lt.initialize().unwrap();

        assert_eq!(
            lt.get_property("Mode0").unwrap(),
            PropertyValue::String("4 - Follow".into())
        );
    }

    #[test]
    fn get_duration_reads_live_register_after_initialize() {
        let t = MockTransport::new().expect_binary(&12345u32.to_le_bytes());
        let mut lt = LaserTrigger::new().with_transport(Box::new(t));
        lt.initialize().unwrap();

        assert_eq!(
            lt.get_property("Duration0 (us)").unwrap(),
            PropertyValue::Integer(12345)
        );
    }

    #[test]
    fn get_mode_unknown_live_value_defaults_off() {
        let t = MockTransport::new().expect_binary(&99u32.to_le_bytes());
        let mut lt = LaserTrigger::new().with_transport(Box::new(t));
        lt.initialize().unwrap();

        assert_eq!(
            lt.get_property("Mode0").unwrap(),
            PropertyValue::String("0 - Off".into())
        );
    }
}
