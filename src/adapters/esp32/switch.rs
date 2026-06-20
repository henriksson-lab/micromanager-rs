//! ESP32Switch — StateDevice with 256 bit-mapped positions.
//! Send ASCII command `S,<val>` to hub.

use parking_lot::Mutex;
use std::sync::Arc;

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::types::{DeviceType, PropertyValue};

use super::hub::HubState;
use super::shutter::SwitchWriter;

const NUM_POSITIONS: u64 = 256;
pub type CommandWriter = Arc<dyn Fn(&str) -> MmResult<()> + Send + Sync>;

pub struct Esp32Switch {
    props: PropertyMap,
    initialized: bool,
    shared: Option<Arc<Mutex<HubState>>>,
    writer: Option<SwitchWriter>,
    command_writer: Option<CommandWriter>,
    labels: Vec<String>,
    gate_open: bool,
    blanking: bool,
}

impl Esp32Switch {
    pub fn new() -> Self {
        let labels: Vec<String> = (0..NUM_POSITIONS).map(|i| i.to_string()).collect();
        let mut props = PropertyMap::new();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("Label", PropertyValue::String("0".into()), false)
            .unwrap();
        props
            .define_property("Sequence", PropertyValue::String("On".into()), false)
            .unwrap();
        props
            .set_allowed_values("Sequence", &["On", "Off"])
            .unwrap();
        props
            .define_property("Blanking Mode", PropertyValue::String("Idle".into()), false)
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

    fn send_command(&self, cmd: &str) -> MmResult<()> {
        let writer = self.command_writer.as_ref().ok_or(MmError::NotConnected)?;
        writer(cmd)
    }
}

impl Default for Esp32Switch {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Esp32Switch {
    fn name(&self) -> &str {
        "ESP32-Switch"
    }
    fn description(&self) -> &str {
        "ESP32 digital output switch"
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
            "State" => Ok(PropertyValue::Integer(
                self.shared
                    .as_ref()
                    .map(|s| s.lock().switch_state as i64)
                    .unwrap_or(0),
            )),
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
            "Blanking Mode" => Ok(PropertyValue::String(
                if self.blanking { "On" } else { "Off" }.into(),
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0..=255).contains(&pos) {
                    return Err(MmError::UnknownPosition);
                }
                self.set_position(pos as u64)?;
                self.props.set(name, PropertyValue::Integer(pos))?;
                return Ok(());
            }
            "Label" => {
                let label = val.as_str().to_string();
                let pos = self
                    .labels
                    .iter()
                    .position(|l| l == &label)
                    .ok_or_else(|| MmError::UnknownLabel(label.clone()))?
                    as u64;
                self.set_position(pos)?;
                self.props.set(name, val)?;
                return Ok(());
            }
            "Blanking Mode" => {
                let mode = val.as_str();
                match mode {
                    "On" if !self.blanking => {
                        if self.initialized {
                            self.send_command("B,1")?;
                        }
                        self.blanking = true;
                    }
                    "Off" if self.blanking => {
                        if self.initialized {
                            self.send_command("B,0")?;
                        }
                        self.blanking = false;
                    }
                    "On" | "Off" => {}
                    _ => return Err(MmError::InvalidPropertyValue),
                }
                self.props.set(name, PropertyValue::String(mode.into()))?;
                return Ok(());
            }
            "Blank On" => {
                let direction = val.as_str();
                let dir = match direction {
                    "Low" => 1,
                    "High" => 0,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                if self.initialized {
                    self.send_command(&format!("F,{}", dir))?;
                }
                self.props
                    .set(name, PropertyValue::String(direction.into()))?;
                return Ok(());
            }
            _ => {}
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
        DeviceType::State
    }
    fn busy(&self) -> bool {
        false
    }
}

impl StateDevice for Esp32Switch {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= NUM_POSITIONS {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized {
            let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
            writer(pos as u8)?;
        }
        if let Some(s) = &self.shared {
            s.lock().switch_state = pos as u8;
        }
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        Ok(self
            .shared
            .as_ref()
            .map(|s| s.lock().switch_state as u64)
            .unwrap_or(0))
    }

    fn get_number_of_positions(&self) -> u64 {
        NUM_POSITIONS
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

    fn make_switch() -> Esp32Switch {
        let shared = Arc::new(Mutex::new(HubState::default()));
        let shared2 = shared.clone();
        let writer: SwitchWriter = Arc::new(move |s| {
            shared2.lock().switch_state = s;
            Ok(())
        });
        Esp32Switch::new().connect(shared, writer)
    }

    fn make_switch_with_commands(log: Arc<std::sync::Mutex<Vec<String>>>) -> Esp32Switch {
        let command_writer: CommandWriter = Arc::new(move |cmd| {
            log.lock().unwrap().push(cmd.to_string());
            Ok(())
        });
        make_switch().with_command_writer(command_writer)
    }

    #[test]
    fn set_get_position() {
        let mut sw = make_switch();
        sw.initialize().unwrap();
        sw.set_position(13).unwrap();
        assert_eq!(sw.get_position().unwrap(), 13);
    }

    #[test]
    fn upstream_sequence_and_blanking_properties() {
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut sw = make_switch_with_commands(log.clone());
        sw.initialize().unwrap();

        assert_eq!(
            sw.get_property("Sequence").unwrap(),
            PropertyValue::String("On".into())
        );
        assert_eq!(
            sw.get_property("Blanking Mode").unwrap(),
            PropertyValue::String("Off".into())
        );
        sw.set_property("Blanking Mode", PropertyValue::String("On".into()))
            .unwrap();
        sw.set_property("Blanking Mode", PropertyValue::String("Off".into()))
            .unwrap();
        sw.set_property("Blank On", PropertyValue::String("Low".into()))
            .unwrap();
        sw.set_property("Blank On", PropertyValue::String("High".into()))
            .unwrap();

        assert_eq!(&*log.lock().unwrap(), &["B,1", "B,0", "F,1", "F,0"]);
    }

    #[test]
    fn negative_state_is_rejected_before_u8_wrap() {
        let mut sw = make_switch();
        sw.initialize().unwrap();
        assert_eq!(
            sw.set_property("State", PropertyValue::Integer(-1))
                .unwrap_err(),
            MmError::UnknownPosition
        );
    }
}
