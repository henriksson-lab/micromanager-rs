//! MicroFPGA Analog Input generic device.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Generic};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};
use crate::{OFFSET_ANALOG_INPUT, MAX_LASERS};

pub struct AnalogInput {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
}

impl AnalogInput {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        for i in 0..MAX_LASERS {
            props.define_property(&format!("AI{}", i), PropertyValue::Integer(0), true).unwrap();
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

    fn read_reg(&mut self, addr: u32) -> MmResult<u32> {
        let req = [0x00u8, (addr & 0xFF) as u8, ((addr >> 8) & 0xFF) as u8,
                   ((addr >> 16) & 0xFF) as u8, ((addr >> 24) & 0xFF) as u8];
        // Send request then receive response in a single transport closure to avoid
        // double-borrowing self via call_transport.
        let raw = self.call_transport(|t| {
            t.send_bytes(&req)?;
            t.receive_bytes(4)
        })?;
        if raw.len() < 4 { return Err(MmError::SerialInvalidResponse); }
        Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
    }

    pub fn refresh(&mut self) -> MmResult<()> {
        for i in 0..MAX_LASERS {
            let v = self.read_reg(OFFSET_ANALOG_INPUT + i)?;
            self.props.entry_mut(&format!("AI{}", i))
                .map(|e| e.value = PropertyValue::Integer(v as i64));
        }
        Ok(())
    }
}

impl Default for AnalogInput { fn default() -> Self { Self::new() } }

impl Device for AnalogInput {
    fn name(&self) -> &str { "Analog Input" }
    fn description(&self) -> &str { "Analog Input" }
    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        self.initialized = true;
        self.refresh()
    }
    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> { self.props.set(name, val) }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Generic }
    fn busy(&self) -> bool { false }
}
impl Generic for AnalogInput {}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn le4(v: u32) -> Vec<u8> { v.to_le_bytes().to_vec() }

    #[test]
    fn initialize_reads_all_channels() {
        // Each read_reg call triggers one receive_bytes(4) — script 8 responses.
        let mut t = MockTransport::new();
        for i in 0..MAX_LASERS {
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
        assert!(ai.is_property_read_only("AI0"));
        assert!(ai.is_property_read_only("AI7"));
    }
}
