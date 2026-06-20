/// Yokogawa CSU-X1 dichroic mirror selector.
///
/// Positions are 1-based on wire.
/// Protocol:
///   `DM_POS, <p>\r`  → `A`       set position
///   `DM_POS, ?\r`    → `<p>\rA`  query position
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};

pub struct CsuXDichroic {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    position: Cell<u64>,
    num_positions: u64,
    labels: Vec<String>,
    gate_open: bool,
}

impl CsuXDichroic {
    pub fn new() -> Self {
        let num_positions: u64 = 3;
        let labels = (1..=num_positions)
            .map(|i| format!("Dichroic-{}", i))
            .collect();
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("State", &["0", "1", "2"]).unwrap();
        props
            .define_property("Label", PropertyValue::String("Dichroic-1".into()), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            position: Cell::new(0),
            num_positions,
            labels,
            gate_open: true,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(RefCell::new(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_ref() {
            Some(t) => f(t.borrow_mut().as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }

    fn parse_position_response(&self, resp: &str) -> MmResult<u64> {
        let wire_pos = resp
            .split(|c: char| c.is_whitespace() || c == '\r' || c == '\n')
            .filter(|s| !s.is_empty())
            .next()
            .and_then(|s| s.trim().parse().ok())
            .ok_or(MmError::SerialInvalidResponse)?;
        if !(1..=self.num_positions).contains(&wire_pos) {
            return Err(MmError::UnknownPosition);
        }
        Ok(wire_pos - 1)
    }

    fn check_ack(resp: &str) -> MmResult<()> {
        match resp.trim_end().chars().last() {
            Some('A') => Ok(()),
            Some('N') => Err(MmError::LocallyDefined(format!(
                "CSU-X dichroic NAK: {}",
                resp
            ))),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn query_position(&self) -> MmResult<u64> {
        let resp = self.cmd("DM_POS, ?")?;
        Self::check_ack(&resp)?;
        let pos = self.parse_position_response(&resp)?;
        self.position.set(pos);
        Ok(pos)
    }
}

impl Default for CsuXDichroic {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CsuXDichroic {
    fn name(&self) -> &str {
        "CSUX-Dichroic Mirror"
    }
    fn description(&self) -> &str {
        "CSUX Dichroics"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.query_position()?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::Integer(self.get_position()? as i64)),
            "Label" => Ok(PropertyValue::String(
                self.labels
                    .get(self.get_position()? as usize)
                    .cloned()
                    .unwrap_or_default(),
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if pos < 0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                let pos = pos as u64;
                self.set_position(pos)
            }
            "Label" => {
                let label = val.as_str().to_string();
                self.set_position_by_label(&label)
            }
            _ => self.props.set(name, val),
        }
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
        DeviceType::State
    }
    fn busy(&self) -> bool {
        false
    }
}

impl StateDevice for CsuXDichroic {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized {
            let resp = self.cmd(&format!("DM_POS, {}", pos + 1))?;
            Self::check_ack(&resp)?;
        }
        self.position.set(pos);
        self.props
            .set("State", PropertyValue::Integer(pos as i64))?;
        self.props.set(
            "Label",
            PropertyValue::String(
                self.labels
                    .get(self.position.get() as usize)
                    .cloned()
                    .unwrap_or_default(),
            ),
        )?;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        if self.initialized && self.transport.is_some() {
            self.query_position()
        } else {
            Ok(self.position.get())
        }
    }
    fn get_number_of_positions(&self) -> u64 {
        self.num_positions
    }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels
            .get(pos as usize)
            .cloned()
            .ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self
            .labels
            .iter()
            .position(|l| l == label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64;
        self.set_position(pos)
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        self.labels[pos as usize] = label.to_string();
        if pos == self.position.get() {
            self.props
                .set("Label", PropertyValue::String(label.to_string()))?;
        }
        Ok(())
    }

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
        Ok(())
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(self.gate_open)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_position() {
        let t = MockTransport::new()
            .expect("DM_POS, ?\r", "2\rA")
            .expect("DM_POS, ?\r", "2\rA");
        let mut d = CsuXDichroic::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 1);
    }

    #[test]
    fn set_position() {
        let t = MockTransport::new()
            .expect("DM_POS, ?\r", "1\rA")
            .expect("DM_POS, 3\r", "A")
            .expect("DM_POS, ?\r", "3\rA");
        let mut d = CsuXDichroic::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position(2).unwrap();
        assert_eq!(d.get_position().unwrap(), 2);
    }

    #[test]
    fn labels_and_position_count_match_csux() {
        let d = CsuXDichroic::new();
        assert!(d.has_property("State"));
        assert!(d.has_property("Label"));
        assert_eq!(d.get_number_of_positions(), 3);
        assert_eq!(d.get_position_label(0).unwrap(), "Dichroic-1");
        assert_eq!(d.get_position_label(2).unwrap(), "Dichroic-3");
        assert!(d.get_position_label(3).is_err());
    }

    #[test]
    fn query_rejects_trailing_nak() {
        let t = MockTransport::new().expect("DM_POS, ?\r", "2\rN");
        let mut d = CsuXDichroic::new().with_transport(Box::new(t));
        assert!(d.initialize().is_err());
    }

    #[test]
    fn query_rejects_out_of_range_wire_position() {
        let t = MockTransport::new().expect("DM_POS, ?\r", "4\rA");
        let mut d = CsuXDichroic::new().with_transport(Box::new(t));
        assert_eq!(d.initialize().unwrap_err(), MmError::UnknownPosition);
    }

    #[test]
    fn query_rejects_zero_wire_position() {
        let t = MockTransport::new().expect("DM_POS, ?\r", "0\rA");
        let mut d = CsuXDichroic::new().with_transport(Box::new(t));
        assert_eq!(d.initialize().unwrap_err(), MmError::UnknownPosition);
    }

    #[test]
    fn set_rejects_malformed_ack() {
        let t = MockTransport::new()
            .expect("DM_POS, ?\r", "1\rA")
            .expect("DM_POS, 2\r", "bad");
        let mut d = CsuXDichroic::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(
            d.set_position(1).unwrap_err(),
            MmError::SerialInvalidResponse
        );
    }
}
