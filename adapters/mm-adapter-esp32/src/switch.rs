//! ESP32Switch — StateDevice with 256 bit-mapped positions.
//! Send ASCII command `S,<val>` to hub.

use parking_lot::Mutex;
use std::sync::Arc;

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::types::{DeviceType, PropertyValue};

use crate::hub::HubState;
use crate::shutter::SwitchWriter;

const NUM_POSITIONS: u64 = 256;

pub struct Esp32Switch {
    props: PropertyMap,
    initialized: bool,
    shared: Option<Arc<Mutex<HubState>>>,
    writer: Option<SwitchWriter>,
    labels: Vec<String>,
    gate_open: bool,
}

impl Esp32Switch {
    pub fn new() -> Self {
        let labels: Vec<String> = (0..NUM_POSITIONS).map(|i| i.to_string()).collect();
        let mut props = PropertyMap::new();
        props.define_property("State", PropertyValue::Integer(0), false).unwrap();
        props.define_property("Label", PropertyValue::String("0".into()), false).unwrap();
        Self { props, initialized: false, shared: None, writer: None, labels, gate_open: true }
    }

    pub fn connect(mut self, shared: Arc<Mutex<HubState>>, writer: SwitchWriter) -> Self {
        self.shared = Some(shared);
        self.writer = Some(writer);
        self
    }
}

impl Default for Esp32Switch {
    fn default() -> Self { Self::new() }
}

impl Device for Esp32Switch {
    fn name(&self) -> &str { "ESP32-Switch" }
    fn description(&self) -> &str { "ESP32 digital output switch" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.shared.is_none() { return Err(MmError::CommHubMissing); }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::Integer(
                self.shared.as_ref().map(|s| s.lock().switch_state as i64).unwrap_or(0)
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "State" {
            let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u8;
            if self.initialized {
                let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
                writer(pos)?;
            }
            if let Some(s) = &self.shared { s.lock().switch_state = pos; }
        }
        self.props.set(name, val)
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::State }
    fn busy(&self) -> bool { false }
}

impl StateDevice for Esp32Switch {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= NUM_POSITIONS { return Err(MmError::UnknownPosition); }
        if self.initialized {
            let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
            writer(pos as u8)?;
        }
        if let Some(s) = &self.shared { s.lock().switch_state = pos as u8; }
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        Ok(self.shared.as_ref().map(|s| s.lock().switch_state as u64).unwrap_or(0))
    }

    fn get_number_of_positions(&self) -> u64 { NUM_POSITIONS }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels.get(pos as usize).cloned().ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self.labels.iter().position(|l| l == label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64;
        self.set_position(pos)
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        if pos >= NUM_POSITIONS { return Err(MmError::UnknownPosition); }
        self.labels[pos as usize] = label.to_string();
        Ok(())
    }

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> { self.gate_open = open; Ok(()) }
    fn get_gate_open(&self) -> MmResult<bool> { Ok(self.gate_open) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_switch() -> Esp32Switch {
        let shared = Arc::new(Mutex::new(HubState::default()));
        let shared2 = shared.clone();
        let writer: SwitchWriter = Arc::new(move |s| { shared2.lock().switch_state = s; Ok(()) });
        Esp32Switch::new().connect(shared, writer)
    }

    #[test]
    fn set_get_position() {
        let mut sw = make_switch();
        sw.initialize().unwrap();
        sw.set_position(13).unwrap();
        assert_eq!(sw.get_position().unwrap(), 13);
    }
}
