use super::common::{SharedXLightTransport, XLightSpec, XLightStateCore};
use crate::error::MmResult;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

const LABELS: [&str; 2] = ["Screen active", "Screen locked"];

const SPEC: XLightSpec = XLightSpec {
    name: "XLIGHT Touchscreen",
    description: "XLIGHT Touchscreen lockout",
    query: "rM",
    command: "M",
    num_positions: 2,
    one_based: false,
    initial_position: 1,
    labels: &LABELS,
};

pub struct XLightTouchScreen(XLightStateCore);

impl XLightTouchScreen {
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

impl Default for XLightTouchScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for XLightTouchScreen {
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
        match name {
            "State" => {
                let pos = val
                    .as_i64()
                    .ok_or(crate::error::MmError::InvalidPropertyValue)?;
                let pos = pos.clamp(0, 1) as u64;
                self.0.set_position(pos)
            }
            _ => self.0.set_property(name, val),
        }
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

impl StateDevice for XLightTouchScreen {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        self.0.set_position(pos.min(1))
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
    fn initialize_locked() {
        let t = MockTransport::new().expect("rM\r", "rM1");
        let mut d = XLightTouchScreen::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 1);
    }

    #[test]
    fn unlock_screen() {
        let t = MockTransport::new()
            .expect("rM\r", "rM1")
            .expect("M0\r", "M0");
        let mut d = XLightTouchScreen::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position(0).unwrap();
        assert_eq!(d.get_position().unwrap(), 0);
    }

    #[test]
    fn state_writes_clamp_like_upstream() {
        let t = MockTransport::new()
            .expect("rM\r", "rM1")
            .expect("M0\r", "M0")
            .expect("M1\r", "M1");
        let mut d = XLightTouchScreen::new().with_transport(Box::new(t));
        d.initialize().unwrap();

        d.set_property("State", PropertyValue::Integer(-4)).unwrap();
        assert_eq!(d.get_position().unwrap(), 0);

        d.set_position(9).unwrap();
        assert_eq!(d.get_position().unwrap(), 1);
    }
}
