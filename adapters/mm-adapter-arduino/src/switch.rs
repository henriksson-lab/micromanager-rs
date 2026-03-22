/// ArduinoSwitch — 8-bit digital output port as a StateDevice.
///
/// Each "position" maps to one of 256 possible output states.
use parking_lot::Mutex;
use std::sync::Arc;

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::types::{DeviceType, PropertyValue};

use crate::hub::HubState;
use crate::shutter::SwitchWriter;

pub struct ArduinoSwitch {
    props: PropertyMap,
    initialized: bool,
    shared: Option<Arc<Mutex<HubState>>>,
    writer: Option<SwitchWriter>,
    labels: Vec<String>,
    gate_open: bool,
}

const NUM_POSITIONS: u64 = 256;

impl ArduinoSwitch {
    pub fn new() -> Self {
        let labels: Vec<String> = (0..NUM_POSITIONS).map(|i| format!("State-{}", i)).collect();
        let mut props = PropertyMap::new();
        props.define_property("State", PropertyValue::Integer(0), false).unwrap();
        props.define_property("Label", PropertyValue::String("State-0".into()), false).unwrap();
        props.define_property("Blanking", PropertyValue::String("Off".into()), false).unwrap();
        props.set_allowed_values("Blanking", &["On", "Off"]).unwrap();

        Self {
            props,
            initialized: false,
            shared: None,
            writer: None,
            labels,
            gate_open: true,
        }
    }

    pub fn connect(mut self, shared: Arc<Mutex<HubState>>, writer: SwitchWriter) -> Self {
        self.shared = Some(shared);
        self.writer = Some(writer);
        self
    }

    fn write_state(&self, state: u16) -> MmResult<()> {
        let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
        writer(state)
    }
}

impl Default for ArduinoSwitch {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ArduinoSwitch {
    fn name(&self) -> &str { "Arduino-Switch" }
    fn description(&self) -> &str { "Arduino 8-bit digital output" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.shared.is_none() {
            return Err(MmError::CommHubMissing);
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => {
                let pos = self.shared.as_ref()
                    .map(|s| s.lock().switch_state as i64)
                    .unwrap_or(0);
                Ok(PropertyValue::Integer(pos))
            }
            "Label" => {
                let pos = self.shared.as_ref()
                    .map(|s| s.lock().switch_state as usize)
                    .unwrap_or(0);
                Ok(PropertyValue::String(
                    self.labels.get(pos).cloned().unwrap_or_default()
                ))
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u16;
                if self.initialized {
                    self.write_state(pos)?;
                }
                if let Some(shared) = &self.shared {
                    shared.lock().switch_state = pos;
                }
                Ok(())
            }
            "Label" => {
                let label = val.as_str().to_string();
                let pos = self.labels.iter().position(|l| l == &label)
                    .ok_or_else(|| MmError::UnknownLabel(label.clone()))? as u16;
                if self.initialized {
                    self.write_state(pos)?;
                }
                if let Some(shared) = &self.shared {
                    shared.lock().switch_state = pos;
                }
                Ok(())
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

    fn device_type(&self) -> DeviceType { DeviceType::State }
    fn busy(&self) -> bool { false }
}

impl StateDevice for ArduinoSwitch {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= NUM_POSITIONS {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized {
            self.write_state(pos as u16)?;
        }
        if let Some(shared) = &self.shared {
            shared.lock().switch_state = pos as u16;
        }
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        Ok(self.shared.as_ref()
            .map(|s| s.lock().switch_state as u64)
            .unwrap_or(0))
    }

    fn get_number_of_positions(&self) -> u64 {
        NUM_POSITIONS
    }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels.get(pos as usize)
            .cloned()
            .ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self.labels.iter().position(|l| l == label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64;
        self.set_position(pos)
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        if pos >= NUM_POSITIONS {
            return Err(MmError::UnknownPosition);
        }
        self.labels[pos as usize] = label.to_string();
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

    fn make_switch() -> ArduinoSwitch {
        let shared = Arc::new(Mutex::new(HubState::default()));
        let shared2 = shared.clone();
        let writer: SwitchWriter = Arc::new(move |state| {
            shared2.lock().switch_state = state;
            Ok(())
        });
        ArduinoSwitch::new().connect(shared, writer)
    }

    #[test]
    fn set_get_position() {
        let mut sw = make_switch();
        sw.initialize().unwrap();
        sw.set_position(42).unwrap();
        assert_eq!(sw.get_position().unwrap(), 42);
    }

    #[test]
    fn out_of_range_rejected() {
        let mut sw = make_switch();
        sw.initialize().unwrap();
        assert!(sw.set_position(256).is_err());
    }

    #[test]
    fn label_navigation() {
        let mut sw = make_switch();
        sw.initialize().unwrap();
        sw.set_position_label(5, "DAPI").unwrap();
        sw.set_position_by_label("DAPI").unwrap();
        assert_eq!(sw.get_position().unwrap(), 5);
    }
}
