/// ArduinoSwitch — digital output pattern as a StateDevice.
///
/// Each "position" maps to one output state. The classic Arduino firmware
/// exposes six digital output pins by default.
use parking_lot::Mutex;
use std::sync::Arc;

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::types::{DeviceType, PropertyValue};

use super::hub::HubState;
use super::shutter::SwitchWriter;

pub type CommandWriter = Arc<dyn Fn(&[u8], usize) -> MmResult<Vec<u8>> + Send + Sync>;

pub struct ArduinoSwitch {
    props: PropertyMap,
    initialized: bool,
    shared: Option<Arc<Mutex<HubState>>>,
    writer: Option<SwitchWriter>,
    command_writer: Option<CommandWriter>,
    labels: Vec<String>,
    gate_open: bool,
    blanking: bool,
    num_positions: u64,
}

const DEFAULT_NUM_DIGITAL_PINS: u8 = 6;

impl ArduinoSwitch {
    pub fn new() -> Self {
        let num_positions = positions_for_pins(DEFAULT_NUM_DIGITAL_PINS);
        let labels: Vec<String> = (0..num_positions).map(|i| i.to_string()).collect();
        let mut props = PropertyMap::new();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("Label", PropertyValue::String("0".into()), false)
            .unwrap();
        props
            .define_property("Sequence", PropertyValue::String("Off".into()), false)
            .unwrap();
        props
            .set_allowed_values("Sequence", &["On", "Off"])
            .unwrap();
        props
            .define_property("Blanking Mode", PropertyValue::String("Off".into()), false)
            .unwrap();
        props
            .set_allowed_values("Blanking Mode", &["On", "Off"])
            .unwrap();
        props
            .define_property("Blank On", PropertyValue::String("Low".into()), false)
            .unwrap();
        props
            .set_allowed_values("Blank On", &["Low", "High"])
            .unwrap();

        Self {
            props,
            initialized: false,
            shared: None,
            writer: None,
            command_writer: None,
            labels,
            gate_open: true,
            blanking: false,
            num_positions,
        }
    }

    pub fn connect(mut self, shared: Arc<Mutex<HubState>>, writer: SwitchWriter) -> Self {
        self.shared = Some(shared);
        self.writer = Some(writer);
        self
    }

    pub fn with_command_writer(mut self, writer: CommandWriter) -> Self {
        self.command_writer = Some(writer);
        self
    }

    pub fn connect_with_num_digital_pins(
        mut self,
        shared: Arc<Mutex<HubState>>,
        writer: SwitchWriter,
        num_digital_pins: u8,
    ) -> Self {
        self.shared = Some(shared);
        self.writer = Some(writer);
        self.configure_num_digital_pins(num_digital_pins);
        self
    }

    pub fn configure_num_digital_pins(&mut self, num_digital_pins: u8) {
        self.num_positions = positions_for_pins(num_digital_pins);
        self.labels = (0..self.num_positions).map(|i| i.to_string()).collect();
    }

    fn write_state(&self, state: u16) -> MmResult<()> {
        let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
        writer(state)
    }

    fn send_command(&self, command: &[u8], response_len: usize, expected: u8) -> MmResult<Vec<u8>> {
        let writer = self.command_writer.as_ref().ok_or(MmError::NotConnected)?;
        let response = writer(command, response_len)?;
        if response.first() != Some(&expected) {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(response)
    }

    fn set_blanking_mode(&mut self, on: bool) -> MmResult<()> {
        if self.initialized {
            if on && !self.blanking {
                self.send_command(&[20], 1, 20)?;
            } else if !on && self.blanking {
                self.send_command(&[21], 2, 21)?;
            }
        }
        self.blanking = on;
        self.props.set(
            "Blanking Mode",
            PropertyValue::String(if on { "On" } else { "Off" }.into()),
        )
    }

    fn set_blanking_trigger_direction(&mut self, direction: &str) -> MmResult<()> {
        let command_value = match direction {
            "Low" => 1,
            "High" => 0,
            _ => return Err(MmError::InvalidPropertyValue),
        };
        if self.initialized {
            self.send_command(&[22, command_value], 1, 22)?;
        }
        self.props
            .set("Blank On", PropertyValue::String(direction.into()))
    }

    fn set_selected_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }

        let state = pos as u16;
        let shutter_open = self
            .shared
            .as_ref()
            .map(|s| s.lock().shutter_state)
            .unwrap_or(false);
        if self.initialized && shutter_open {
            self.write_state(state)?;
        }
        if let Some(shared) = &self.shared {
            shared.lock().switch_state = state;
        }
        self.props
            .entry_mut("State")
            .map(|e| e.value = PropertyValue::Integer(pos as i64));
        self.props
            .entry_mut("Label")
            .map(|e| e.value = PropertyValue::String(self.labels[pos as usize].clone()));
        Ok(())
    }
}

impl Default for ArduinoSwitch {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ArduinoSwitch {
    fn name(&self) -> &str {
        "Arduino-Switch"
    }
    fn description(&self) -> &str {
        "Arduino 8-bit digital output"
    }

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
                let pos = self
                    .shared
                    .as_ref()
                    .map(|s| s.lock().switch_state as i64)
                    .unwrap_or(0);
                Ok(PropertyValue::Integer(pos))
            }
            "Label" => {
                let pos = self
                    .shared
                    .as_ref()
                    .map(|s| s.lock().switch_state as usize)
                    .unwrap_or(0);
                Ok(PropertyValue::String(
                    self.labels.get(pos).cloned().unwrap_or_default(),
                ))
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if pos < 0 {
                    return Err(MmError::UnknownPosition);
                }
                self.set_selected_position(pos as u64)
            }
            "Label" => {
                let label = val.as_str().to_string();
                let pos = self
                    .labels
                    .iter()
                    .position(|l| l == &label)
                    .ok_or_else(|| MmError::UnknownLabel(label.clone()))?
                    as u64;
                self.set_selected_position(pos)
            }
            "Blanking Mode" => {
                let state = val.as_str();
                match state {
                    "On" => self.set_blanking_mode(true),
                    "Off" => self.set_blanking_mode(false),
                    _ => Err(MmError::InvalidPropertyValue),
                }
            }
            "Blank On" => self.set_blanking_trigger_direction(val.as_str()),
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

impl StateDevice for ArduinoSwitch {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        self.set_selected_position(pos)
    }

    fn get_position(&self) -> MmResult<u64> {
        Ok(self
            .shared
            .as_ref()
            .map(|s| s.lock().switch_state as u64)
            .unwrap_or(0))
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

fn positions_for_pins(num_digital_pins: u8) -> u64 {
    1u64 << num_digital_pins.min(8)
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

    fn make_switch_with_commands(
        expected: Arc<std::sync::Mutex<Vec<(Vec<u8>, Vec<u8>)>>>,
    ) -> ArduinoSwitch {
        let shared = Arc::new(Mutex::new(HubState::default()));
        let state_writer: SwitchWriter = Arc::new(|_| Ok(()));
        let command_writer: CommandWriter = Arc::new(move |command, response_len| {
            let (expected_command, response) = expected.lock().unwrap().remove(0);
            assert_eq!(command, expected_command.as_slice());
            Ok(response[..response.len().min(response_len)].to_vec())
        });
        ArduinoSwitch::new()
            .connect(shared, state_writer)
            .with_command_writer(command_writer)
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
        assert!(sw.set_position(64).is_err());
    }

    #[test]
    fn number_of_positions_tracks_digital_pin_count() {
        let shared = Arc::new(Mutex::new(HubState::default()));
        let writer: SwitchWriter = Arc::new(|_| Ok(()));
        let mut sw = ArduinoSwitch::new().connect_with_num_digital_pins(shared, writer, 8);
        sw.initialize().unwrap();

        assert_eq!(sw.get_number_of_positions(), 256);
        sw.set_position(255).unwrap();
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

    #[test]
    fn selected_position_is_only_written_when_shutter_open() {
        let shared = Arc::new(Mutex::new(HubState::default()));
        let writes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let writes2 = writes.clone();
        let writer: SwitchWriter = Arc::new(move |state| {
            writes2.lock().unwrap().push(state);
            Ok(())
        });
        let mut sw = ArduinoSwitch::new().connect(shared.clone(), writer);
        sw.initialize().unwrap();

        sw.set_position(7).unwrap();
        shared.lock().shutter_state = true;
        sw.set_position(8).unwrap();

        assert_eq!(&*writes.lock().unwrap(), &[8]);
        assert_eq!(shared.lock().switch_state, 8);
    }

    #[test]
    fn blanking_mode_sends_start_and_stop_commands() {
        let expected = Arc::new(std::sync::Mutex::new(vec![
            (vec![20], vec![20]),
            (vec![21], vec![21, 0]),
        ]));
        let mut sw = make_switch_with_commands(expected.clone());
        sw.initialize().unwrap();

        sw.set_property("Blanking Mode", PropertyValue::String("On".into()))
            .unwrap();
        sw.set_property("Blanking Mode", PropertyValue::String("Off".into()))
            .unwrap();

        assert!(expected.lock().unwrap().is_empty());
    }

    #[test]
    fn blanking_trigger_direction_sends_upstream_encoding() {
        let expected = Arc::new(std::sync::Mutex::new(vec![
            (vec![22, 1], vec![22]),
            (vec![22, 0], vec![22]),
        ]));
        let mut sw = make_switch_with_commands(expected.clone());
        sw.initialize().unwrap();

        sw.set_property("Blank On", PropertyValue::String("Low".into()))
            .unwrap();
        sw.set_property("Blank On", PropertyValue::String("High".into()))
            .unwrap();

        assert!(expected.lock().unwrap().is_empty());
    }

    #[test]
    fn sequence_and_blanking_report_off_by_default_after_initialize() {
        let mut sw = make_switch();
        sw.initialize().unwrap();

        assert_eq!(
            sw.get_property("Sequence").unwrap(),
            PropertyValue::String("Off".into())
        );
        assert_eq!(
            sw.get_property("Blanking Mode").unwrap(),
            PropertyValue::String("Off".into())
        );
    }
}
