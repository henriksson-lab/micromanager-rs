//! MicroFPGA Camera Trigger generic device.
use super::{
    read_register, write_register, OFFSET_CAM_EXPOSURE, OFFSET_CAM_PULSE, OFFSET_CAM_READOUT,
    OFFSET_CAM_SYNC_MODE, OFFSET_CAM_TRIGGER_START, OFFSET_LASER_DELAY,
};
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use parking_lot::Mutex;

pub struct CameraTrigger {
    props: PropertyMap,
    transport: Mutex<Option<Box<dyn Transport>>>,
    initialized: bool,
}

impl CameraTrigger {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Start", PropertyValue::String("Start".into()), false)
            .unwrap();
        props
            .set_allowed_values("Start", &["Start", "Stop"])
            .unwrap();
        props
            .define_property("Pulse (us)", PropertyValue::Integer(2000), false)
            .unwrap();
        props
            .set_property_limits("Pulse (us)", 0.0, 1048575.0)
            .unwrap();
        props
            .define_property("Read-out (us)", PropertyValue::Integer(1000), false)
            .unwrap();
        props
            .set_property_limits("Read-out (us)", 0.0, 65535.0)
            .unwrap();
        props
            .define_property("Exposure (us)", PropertyValue::Integer(25000), false)
            .unwrap();
        props
            .set_property_limits("Exposure (us)", 0.0, 1048575.0)
            .unwrap();
        props
            .define_property("Delay (us)", PropertyValue::Integer(500), false)
            .unwrap();
        props
            .set_property_limits("Delay (us)", 0.0, 65535.0)
            .unwrap();
        Self {
            props,
            transport: Mutex::new(None),
            initialized: false,
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

    fn live_property(&self, name: &str) -> MmResult<Option<PropertyValue>> {
        if !self.initialized {
            return Ok(None);
        }
        let value = match name {
            "Start" => {
                let value = self.read_reg(OFFSET_CAM_TRIGGER_START)?;
                PropertyValue::String(if value == 1 { "Start" } else { "Stop" }.into())
            }
            "Pulse (us)" => PropertyValue::Integer(self.read_reg(OFFSET_CAM_PULSE)? as i64),
            "Read-out (us)" => PropertyValue::Integer(self.read_reg(OFFSET_CAM_READOUT)? as i64),
            "Exposure (us)" => PropertyValue::Integer(self.read_reg(OFFSET_CAM_EXPOSURE)? as i64),
            "Delay (us)" => PropertyValue::Integer(self.read_reg(OFFSET_LASER_DELAY)? as i64),
            _ => return Ok(None),
        };
        Ok(Some(value))
    }
}

impl Default for CameraTrigger {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CameraTrigger {
    fn name(&self) -> &str {
        "Camera Trigger"
    }
    fn description(&self) -> &str {
        "Camera Trigger"
    }
    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.lock().is_some() {
            self.write_reg(OFFSET_CAM_SYNC_MODE, 1)?;
        }
        self.initialized = true;
        Ok(())
    }
    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if let Some(value) = self.live_property(name)? {
            return Ok(value);
        }
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        let v = match name {
            "Start" => match &val {
                PropertyValue::String(s) if s == "Start" => 1,
                PropertyValue::String(s) if s == "Stop" => 0,
                _ => return Err(MmError::InvalidPropertyValue),
            },
            _ => val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32,
        };
        let addr = match name {
            "Start" => OFFSET_CAM_TRIGGER_START,
            "Pulse (us)" => OFFSET_CAM_PULSE,
            "Read-out (us)" => OFFSET_CAM_READOUT,
            "Exposure (us)" => OFFSET_CAM_EXPOSURE,
            "Delay (us)" => OFFSET_LASER_DELAY,
            _ => return self.props.set(name, val),
        };
        if self.initialized {
            self.write_reg(addr, v)?;
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
impl Generic for CameraTrigger {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn le4(v: u32) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }

    #[test]
    fn get_property_reads_live_register_after_initialize() {
        let t = MockTransport::new().expect_binary(&le4(4321));
        let mut cam = CameraTrigger::new().with_transport(Box::new(t));
        cam.initialize().unwrap();

        assert_eq!(
            cam.get_property("Pulse (us)").unwrap(),
            PropertyValue::Integer(4321)
        );
    }

    #[test]
    fn get_start_maps_live_zero_to_stop() {
        let t = MockTransport::new().expect_binary(&le4(0));
        let mut cam = CameraTrigger::new().with_transport(Box::new(t));
        cam.initialize().unwrap();

        assert_eq!(
            cam.get_property("Start").unwrap(),
            PropertyValue::String("Stop".into())
        );
    }

    #[test]
    fn get_start_maps_unknown_live_value_to_stop() {
        let t = MockTransport::new().expect_binary(&le4(2));
        let mut cam = CameraTrigger::new().with_transport(Box::new(t));
        cam.initialize().unwrap();

        assert_eq!(
            cam.get_property("Start").unwrap(),
            PropertyValue::String("Stop".into())
        );
    }
}
