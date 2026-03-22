//! MicroFPGA Camera Trigger generic device.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Generic};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};
use crate::{OFFSET_CAM_SYNC_MODE, OFFSET_CAM_TRIGGER_START, OFFSET_CAM_PULSE,
            OFFSET_CAM_READOUT, OFFSET_CAM_EXPOSURE, OFFSET_LASER_DELAY};

pub struct CameraTrigger {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
}

impl CameraTrigger {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("SyncMode",   PropertyValue::Integer(0), false).unwrap();
        props.define_property("Pulse",      PropertyValue::Integer(10000), false).unwrap();
        props.define_property("Readout",    PropertyValue::Integer(10000), false).unwrap();
        props.define_property("Exposure",   PropertyValue::Integer(10000), false).unwrap();
        props.define_property("LaserDelay", PropertyValue::Integer(0), false).unwrap();
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

impl Default for CameraTrigger { fn default() -> Self { Self::new() } }

impl Device for CameraTrigger {
    fn name(&self) -> &str { "Camera Trigger" }
    fn description(&self) -> &str { "Camera Trigger" }
    fn initialize(&mut self) -> MmResult<()> { self.initialized = true; Ok(()) }
    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32;
        let addr = match name {
            "SyncMode"   => OFFSET_CAM_SYNC_MODE,
            "TrigStart"  => OFFSET_CAM_TRIGGER_START,
            "Pulse"      => OFFSET_CAM_PULSE,
            "Readout"    => OFFSET_CAM_READOUT,
            "Exposure"   => OFFSET_CAM_EXPOSURE,
            "LaserDelay" => OFFSET_LASER_DELAY,
            _ => return self.props.set(name, val),
        };
        if self.initialized { self.write_reg(addr, v)?; }
        self.props.set(name, PropertyValue::Integer(v as i64))
    }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Generic }
    fn busy(&self) -> bool { false }
}
impl Generic for CameraTrigger {}
