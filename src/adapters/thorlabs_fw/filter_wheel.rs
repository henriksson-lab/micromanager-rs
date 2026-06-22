/// Thorlabs motorized filter wheel.
///
/// Protocol (ASCII, `\r` terminated):
///   `sensors=0\r`  → disable sensor mode
///   `pos?\r`        → current position (1-indexed integer)
///   `pos=<N>\r`     → move to position N (1-indexed, 1–6)
///
/// Positions are 1-indexed in commands but 0-indexed in the StateDevice API.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::time::{Duration, Instant};

const NUM_POSITIONS: u64 = 6;

pub struct ThorlabsFilterWheel {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position: u64,
    labels: Vec<String>,
    gate_open: bool,
    delay_ms: f64,
    changed_time: Option<Instant>,
}

impl ThorlabsFilterWheel {
    pub fn new() -> Self {
        let labels: Vec<String> = (1..=NUM_POSITIONS)
            .map(|i| format!("Filter-{}", i))
            .collect();
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String(String::new()), false)
            .unwrap();
        props
            .define_property("Delay_ms", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("Delay_ms", 0.0, f64::MAX)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            position: 0,
            labels,
            gate_open: true,
            delay_ms: 0.0,
            changed_time: None,
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
        let cmd = command.to_string();
        self.call_transport(|t| t.send(&cmd))
    }

    fn mark_changed(&mut self) {
        self.changed_time = Some(Instant::now());
    }

    fn delay_duration(&self) -> Duration {
        Duration::from_secs_f64(self.delay_ms.max(0.0) / 1000.0)
    }
}

impl Default for ThorlabsFilterWheel {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ThorlabsFilterWheel {
    fn name(&self) -> &str {
        "Thorlabs Filter Wheel"
    }
    fn description(&self) -> &str {
        "Thorlabs filter wheel driver"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.initialized {
            return Ok(());
        }
        if !self.props.has_property("State") {
            self.props
                .define_property("State", PropertyValue::Integer(0), false)?;
        }
        if !self.props.has_property("Label") {
            self.props
                .define_property("Label", PropertyValue::String("Filter-1".into()), false)?;
        }
        self.mark_changed();
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" if self.props.has_property("State") => {
                Ok(PropertyValue::Integer(self.position as i64))
            }
            "Label" if self.props.has_property("Label") => Ok(PropertyValue::String(
                self.labels
                    .get(self.position as usize)
                    .cloned()
                    .unwrap_or_default(),
            )),
            "Delay_ms" => Ok(PropertyValue::Float(self.delay_ms)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => self.props.set(
                name,
                PropertyValue::String(self.props.get(name)?.as_str().to_string()),
            ),
            "State" if self.props.has_property("State") => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u64;
                self.set_position(pos)
            }
            "Label" if self.props.has_property("Label") => {
                let label = val.as_str().to_string();
                self.set_position_by_label(&label)
            }
            "Delay_ms" => {
                self.delay_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.delay_ms))
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
        self.changed_time
            .map(|changed| changed.elapsed() < self.delay_duration())
            .unwrap_or(false)
    }
}

impl StateDevice for ThorlabsFilterWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if self.initialized {
            self.mark_changed();
        }
        if pos >= NUM_POSITIONS {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized {
            self.send_cmd(&format!("pos={}", pos + 1))?; // 1-indexed in command
        }
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        Ok(self.position)
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
    use crate::transport::MockTransport;

    #[test]
    fn initialize_reads_position() {
        let mut fw = ThorlabsFilterWheel::new();
        assert!(!fw.has_property("State"));
        assert!(!fw.has_property("Label"));
        assert!(fw.has_property("Delay_ms"));
        assert!(!fw.has_property("Delay"));
        fw.initialize().unwrap();
        assert!(fw.has_property("State"));
        assert!(fw.has_property("Label"));
        assert_eq!(fw.get_position().unwrap(), 0);
    }

    #[test]
    fn set_position() {
        let t = MockTransport::new();
        let mut fw = ThorlabsFilterWheel::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_position(3).unwrap(); // pos 3 (0-indexed) → command pos=4
        assert_eq!(fw.get_position().unwrap(), 3);
    }

    #[test]
    fn move_command_does_not_wait_for_reply() {
        let t = MockTransport::new();
        let mut fw = ThorlabsFilterWheel::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_position(3).unwrap();
        assert_eq!(fw.get_position().unwrap(), 3);
    }

    #[test]
    fn out_of_range_rejected() {
        let mut fw = ThorlabsFilterWheel::new();
        fw.initialize().unwrap();
        assert!(fw.set_position(6).is_err());
    }

    #[test]
    fn label_navigation() {
        let t = MockTransport::new();
        let mut fw = ThorlabsFilterWheel::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_position_label(1, "FITC").unwrap();
        fw.set_position_by_label("FITC").unwrap();
        assert_eq!(fw.get_position().unwrap(), 1);
    }

    #[test]
    fn no_transport_error() {
        assert!(ThorlabsFilterWheel::new().initialize().is_ok());
    }

    #[test]
    fn delay_tracks_busy_after_successful_move() {
        let t = MockTransport::new();
        let mut fw = ThorlabsFilterWheel::new().with_transport(Box::new(t));
        fw.set_property("Delay_ms", PropertyValue::Float(50.0))
            .unwrap();
        fw.initialize().unwrap();

        assert!(fw.busy());
        std::thread::sleep(Duration::from_millis(60));
        assert!(!fw.busy());
        fw.set_position(1).unwrap();
        assert!(fw.busy());
        std::thread::sleep(Duration::from_millis(60));
        assert!(!fw.busy());
    }

    #[test]
    fn initialized_port_change_is_reverted_without_error() {
        let mut fw = ThorlabsFilterWheel::new();
        fw.set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        fw.initialize().unwrap();

        fw.set_property("Port", PropertyValue::String("COM2".into()))
            .unwrap();
        assert_eq!(fw.get_property("Port").unwrap().as_str(), "COM1");
    }

    #[test]
    fn out_of_range_move_still_starts_delay_when_initialized() {
        let mut fw = ThorlabsFilterWheel::new();
        fw.set_property("Delay_ms", PropertyValue::Float(50.0))
            .unwrap();
        fw.initialize().unwrap();

        assert_eq!(fw.set_position(6).unwrap_err(), MmError::UnknownPosition);
        assert!(fw.busy());
    }
}
