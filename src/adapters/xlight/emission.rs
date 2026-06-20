use super::common::{SharedXLightTransport, XLightSpec, XLightStateCore};
use crate::error::MmResult;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

const LABELS: [&str; 8] = [
    "Emission-0",
    "Emission-1",
    "Emission-2",
    "Emission-3",
    "Emission-4",
    "Emission-5",
    "Emission-6",
    "Emission-7",
];

const SPEC: XLightSpec = XLightSpec {
    name: "XLIGHT Emission Wheel",
    description: "XLIGHT Emission Wheel Position",
    query: "rB",
    command: "B",
    num_positions: 8,
    one_based: true,
    initial_position: 0,
    labels: &LABELS,
};

pub struct XLightEmission(XLightStateCore);

impl XLightEmission {
    pub fn new() -> Self {
        Self(XLightStateCore::new(SPEC))
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.0 = self.0.with_transport(t);
        self
    }

    pub fn with_shared_transport(mut self, transport: SharedXLightTransport) -> Self {
        self.0 = self.0.with_shared_transport(transport);
        self
    }
}

impl Default for XLightEmission {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for XLightEmission {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn description(&self) -> &str {
        self.0.description()
    }
    fn initialize(&mut self) -> MmResult<()> {
        self.0.initialize()
    }
    fn shutdown(&mut self) -> MmResult<()> {
        self.0.shutdown()
    }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.0.get_property(name)
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        self.0.set_property(name, val)
    }
    fn property_names(&self) -> Vec<String> {
        self.0.property_names()
    }
    fn has_property(&self, name: &str) -> bool {
        self.0.has_property(name)
    }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.0.is_property_read_only(name)
    }
    fn device_type(&self) -> DeviceType {
        self.0.device_type()
    }
    fn busy(&self) -> bool {
        self.0.busy()
    }
}

impl StateDevice for XLightEmission {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        self.0.set_position(pos)
    }
    fn get_position(&self) -> MmResult<u64> {
        self.0.get_position()
    }
    fn get_number_of_positions(&self) -> u64 {
        self.0.get_number_of_positions()
    }
    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.0.get_position_label(pos)
    }
    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        self.0.set_position_by_label(label)
    }
    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        self.0.set_position_label(pos, label)
    }
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.0.set_gate_open(open)
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        self.0.get_gate_open()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_reads_position() {
        let t = MockTransport::new().expect("rB\r", "rB5");
        let mut d = XLightEmission::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 4);
    }

    #[test]
    fn set_position_sends_1based() {
        let t = MockTransport::new()
            .expect("rB\r", "rB1")
            .expect("B7\r", "B7");
        let mut d = XLightEmission::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position(6).unwrap();
        assert_eq!(d.get_position().unwrap(), 6);
    }

    #[test]
    fn out_of_range_rejected() {
        let t = MockTransport::new().expect("rB\r", "rB1");
        let mut d = XLightEmission::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert!(d.set_position(8).is_err());
    }
}
