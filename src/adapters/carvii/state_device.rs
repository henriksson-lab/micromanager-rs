/// CARVII state devices (filter wheels, sliders, motors).
///
/// Protocol (TX `\r`):
///   `<CMD><POS>\r`  → echo               set position
///
/// Used for: ExFilter (A), EmFilter (B), Dichroic (C), DiskSlider (D),
///           SpinMotor (N), PrismSlider (P), TouchScreen (M).
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct CarviiStateDevice {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    cmd_char: char,
    num_positions: u64,
    position: u64,
    labels: Vec<String>,
}

impl CarviiStateDevice {
    pub fn new(cmd_char: char, num_positions: u64) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        let labels = default_labels(cmd_char, num_positions);
        let position = default_position(cmd_char);
        props
            .define_property("State", PropertyValue::Integer(position as i64), false)
            .unwrap();
        let states: Vec<String> = (0..num_positions).map(|i| i.to_string()).collect();
        let state_refs: Vec<&str> = states.iter().map(String::as_str).collect();
        props.set_allowed_values("State", &state_refs).unwrap();
        props
            .define_property("Label", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            cmd_char,
            num_positions,
            position,
            labels,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn send_cmd(&mut self, command: &str) -> MmResult<()> {
        let full = format!("{}\r", command);
        self.call_transport(|t| t.send(&full))
    }

    fn wire_position(&self, pos: u64) -> u64 {
        if uses_one_based_wire(self.cmd_char) {
            pos + 1
        } else {
            pos
        }
    }
}

impl Device for CarviiStateDevice {
    fn name(&self) -> &str {
        "CarviiStateDevice"
    }
    fn description(&self) -> &str {
        "CARVII State Device"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.position >= self.num_positions {
            self.position = self.num_positions.saturating_sub(1);
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
            "State" => Ok(PropertyValue::Integer(self.position as i64)),
            "Label" => Ok(PropertyValue::String(
                self.labels
                    .get(self.position as usize)
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
                self.set_position(pos as u64)
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

impl StateDevice for CarviiStateDevice {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if self.num_positions == 0 {
            return Err(MmError::LocallyDefined("No positions defined".to_string()));
        }
        let pos = pos.min(self.num_positions - 1);
        if pos == self.position {
            return Ok(());
        }
        let cmd = format!("{}{}", self.cmd_char, self.wire_position(pos));
        self.send_cmd(&cmd)?;
        self.position = pos;
        self.props
            .set("State", PropertyValue::Integer(self.position as i64))?;
        self.props.set(
            "Label",
            PropertyValue::String(
                self.labels
                    .get(self.position as usize)
                    .cloned()
                    .unwrap_or_default(),
            ),
        )?;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        Ok(self.position)
    }
    fn get_number_of_positions(&self) -> u64 {
        self.num_positions
    }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels
            .get(pos as usize)
            .cloned()
            .ok_or_else(|| MmError::LocallyDefined(format!("Position {} out of range", pos)))
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
            return Err(MmError::LocallyDefined(format!(
                "Position {} out of range",
                pos
            )));
        }
        self.labels[pos as usize] = label.to_string();
        if pos == self.position {
            self.props
                .set("Label", PropertyValue::String(label.to_string()))?;
        }
        Ok(())
    }

    fn set_gate_open(&mut self, _open: bool) -> MmResult<()> {
        Ok(())
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(true)
    }
}

fn uses_one_based_wire(cmd_char: char) -> bool {
    matches!(cmd_char, 'A' | 'B' | 'C')
}

fn default_position(cmd_char: char) -> u64 {
    match cmd_char {
        'D' | 'N' | 'M' => 1,
        _ => 0,
    }
}

fn default_labels(cmd_char: char, num_positions: u64) -> Vec<String> {
    match cmd_char {
        'A' => (0..num_positions)
            .map(|i| format!("ExFilter-{}", i))
            .collect(),
        'B' => (0..num_positions)
            .map(|i| format!("EmFilter-{}", i))
            .collect(),
        'C' => (0..num_positions)
            .map(|i| format!("Dichroic-{}", i))
            .collect(),
        'D' if num_positions == 2 => vec!["Out".to_string(), "In".to_string()],
        'N' if num_positions == 2 => vec!["Off (no spin)".to_string(), "On (spinning)".to_string()],
        'P' if num_positions == 2 => vec!["To camera".to_string(), "To eyepieces".to_string()],
        'M' if num_positions == 2 => vec!["Screen active".to_string(), "Screen locked".to_string()],
        _ => (0..num_positions)
            .map(|i| format!("Position-{}", i + 1))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::Transport;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingTransport {
        sent: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingTransport {
        fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
            let sent = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    sent: Arc::clone(&sent),
                },
                sent,
            )
        }
    }

    impl Transport for RecordingTransport {
        fn send(&mut self, cmd: &str) -> MmResult<()> {
            self.sent.lock().unwrap().push(cmd.to_string());
            Ok(())
        }

        fn receive_line(&mut self) -> MmResult<String> {
            Err(MmError::SerialTimeout)
        }

        fn purge(&mut self) -> MmResult<()> {
            Ok(())
        }
    }

    #[test]
    fn initialize_ex_filter() {
        let mut d = CarviiStateDevice::new('A', 8);
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 0);
        assert_eq!(d.get_number_of_positions(), 8);
    }

    #[test]
    fn set_position() {
        let (t, sent) = RecordingTransport::new();
        let mut d = CarviiStateDevice::new('A', 8).with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position(2).unwrap();
        assert_eq!(d.get_position().unwrap(), 2);
        assert_eq!(*sent.lock().unwrap(), vec!["A3\r".to_string()]);
    }

    #[test]
    fn disk_slider() {
        let (t, sent) = RecordingTransport::new();
        let mut d = CarviiStateDevice::new('D', 2).with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 1);
        d.set_position(0).unwrap();
        assert_eq!(d.get_position().unwrap(), 0);
        assert_eq!(*sent.lock().unwrap(), vec!["D0\r".to_string()]);
    }

    #[test]
    fn duplicate_position_is_noop() {
        let (t, sent) = RecordingTransport::new();
        let mut d = CarviiStateDevice::new('P', 2).with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position(0).unwrap();
        assert_eq!(d.get_position().unwrap(), 0);
        assert!(sent.lock().unwrap().is_empty());
    }

    #[test]
    fn high_position_clamps_to_last_position() {
        let (t, sent) = RecordingTransport::new();
        let mut d = CarviiStateDevice::new('C', 5).with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position(99).unwrap();
        assert_eq!(d.get_position().unwrap(), 4);
        assert_eq!(*sent.lock().unwrap(), vec!["C5\r".to_string()]);
    }

    #[test]
    fn upstream_labels() {
        let d = CarviiStateDevice::new('M', 2);
        assert!(d.has_property("State"));
        assert!(d.has_property("Label"));
        assert_eq!(d.get_property("State").unwrap(), PropertyValue::Integer(1));
        assert_eq!(
            d.get_property("Label").unwrap(),
            PropertyValue::String("Screen locked".into())
        );
        assert_eq!(d.get_position_label(0).unwrap(), "Screen active");
        assert_eq!(d.get_position_label(1).unwrap(), "Screen locked");
        let ex = CarviiStateDevice::new('A', 8);
        assert_eq!(ex.get_position_label(7).unwrap(), "ExFilter-7");
    }

    #[test]
    fn label_roundtrip() {
        let (t, sent) = RecordingTransport::new();
        let mut d = CarviiStateDevice::new('A', 8).with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position_label(1, "FITC").unwrap();
        assert_eq!(d.get_position_label(1).unwrap(), "FITC");
        d.set_position_by_label("FITC").unwrap();
        assert_eq!(d.get_position().unwrap(), 1);
        assert_eq!(*sent.lock().unwrap(), vec!["A2\r".to_string()]);
    }

    #[test]
    fn no_transport_error() {
        let mut d = CarviiStateDevice::new('A', 8);
        d.initialize().unwrap();
        assert!(d.set_position(1).is_err());
    }
}
