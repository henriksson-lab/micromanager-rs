use super::common::{SharedXLightTransport, XLightSpec, XLightStateCore};
use crate::error::MmResult;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

const LABELS: [&str; 8] = [
    "Excitation-0",
    "Excitation-1",
    "Excitation-2",
    "Excitation-3",
    "Excitation-4",
    "Excitation-5",
    "Excitation-6",
    "Excitation-7",
];

const SPEC: XLightSpec = XLightSpec {
    name: "XLIGHT Excitation Wheel",
    description: "XLIGHT Excitation Wheel Position",
    query: "rE",
    command: "E",
    num_positions: 8,
    one_based: true,
    initial_position: 0,
    labels: &LABELS,
};

pub struct XLightExcitation {
    core: XLightStateCore,
    use_new_cmd: bool,
}

impl XLightExcitation {
    pub fn new() -> Self {
        Self {
            core: XLightStateCore::new(SPEC),
            use_new_cmd: false,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.core = self.core.with_transport(t);
        self
    }

    pub fn with_shared_transport(mut self, transport: SharedXLightTransport) -> Self {
        self.core = self.core.with_shared_transport(transport);
        self
    }
}

impl Default for XLightExcitation {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for XLightExcitation {
    fn name(&self) -> &str {
        self.core.name()
    }
    fn description(&self) -> &str {
        self.core.description()
    }
    fn initialize(&mut self) -> MmResult<()> {
        match self.core.initialize_with_command("rE", "E") {
            Ok(()) => {
                self.use_new_cmd = false;
                Ok(())
            }
            Err(_) => {
                self.core.initialize_with_command("rA", "A")?;
                self.use_new_cmd = true;
                Ok(())
            }
        }
    }
    fn shutdown(&mut self) -> MmResult<()> {
        self.core.shutdown()
    }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.core.get_property(name)
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        self.core.set_property(name, val)
    }
    fn property_names(&self) -> Vec<String> {
        self.core.property_names()
    }
    fn has_property(&self, name: &str) -> bool {
        self.core.has_property(name)
    }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.core.is_property_read_only(name)
    }
    fn device_type(&self) -> DeviceType {
        self.core.device_type()
    }
    fn busy(&self) -> bool {
        self.core.busy()
    }
}

impl StateDevice for XLightExcitation {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        self.core.set_position(pos)
    }
    fn get_position(&self) -> MmResult<u64> {
        self.core.get_position()
    }
    fn get_number_of_positions(&self) -> u64 {
        self.core.get_number_of_positions()
    }
    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.core.get_position_label(pos)
    }
    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        self.core.set_position_by_label(label)
    }
    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        self.core.set_position_label(pos, label)
    }
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.core.set_gate_open(open)
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        self.core.get_gate_open()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_old_firmware() {
        let t = MockTransport::new().expect("rE\r", "rE4");
        let mut d = XLightExcitation::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 3);
        assert!(!d.use_new_cmd);
    }

    #[test]
    fn set_position_old_cmd() {
        let t = MockTransport::new()
            .expect("rE\r", "rE1")
            .expect("E3\r", "E3");
        let mut d = XLightExcitation::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position(2).unwrap();
        assert_eq!(d.get_position().unwrap(), 2);
    }

    #[test]
    fn set_position_new_cmd() {
        let t = MockTransport::new()
            .expect("rE\r", "ERR")
            .expect("rA\r", "rA2")
            .expect("A5\r", "A5");
        let mut d = XLightExcitation::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert!(d.use_new_cmd);
        d.set_position(4).unwrap();
        assert_eq!(d.get_position().unwrap(), 4);
    }
}
