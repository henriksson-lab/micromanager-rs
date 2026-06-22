/// ASI FW-1000 filter wheel controller.
///
/// Protocol (ASCII, terminated with space or `\r`):
///   `VN\r`        → version string (≥3 chars)
///   `NF\r`        → number of filter positions (6 or 8)
///   `FW\r`        → current wheel number (0 or 1)
///   `FW <n>\r`    → select wheel n
///   `MP\r`        → current filter position (0-indexed digit)
///   `MP <n>\r`    → move to filter position n (0-indexed)
///   `?\r`         → busy status ('3' = busy, other = idle)
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

pub struct AsiFW1000 {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    position: u64,
    num_positions: u64,
    labels: Vec<String>,
    gate_open: bool,
    speed_setting: i64,
    manual_serial_answer: String,
    closed_position: String,
}

impl AsiFW1000 {
    pub fn new() -> Self {
        let num_positions: u64 = 6;
        let labels: Vec<String> = (0..num_positions).map(|i| format!("State-{}", i)).collect();
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("Label", PropertyValue::String("State-0".into()), false)
            .unwrap();
        props
            .define_property(
                "Closed_Position",
                PropertyValue::String(String::new()),
                false,
            )
            .unwrap();
        props
            .define_property("SpeedSetting", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_property_limits("SpeedSetting", 0.0, 9.0).unwrap();
        props
            .define_property("SerialCommand", PropertyValue::String(String::new()), false)
            .unwrap();
        props
            .define_property("SerialResponse", PropertyValue::String(String::new()), true)
            .unwrap();

        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            position: 0,
            num_positions,
            labels,
            gate_open: true,
            speed_setting: 0,
            manual_serial_answer: String::new(),
            closed_position: String::new(),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = RefCell::new(Some(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.borrow_mut().as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let cmd = format!("{}\r", command);
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }

    fn parse_last_word(resp: &str) -> &str {
        resp.split_whitespace().last().unwrap_or("")
    }
}

impl Default for AsiFW1000 {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for AsiFW1000 {
    fn name(&self) -> &str {
        "ASI-FW1000"
    }
    fn description(&self) -> &str {
        "ASI FW-1000 filter wheel"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }

        let ver = self.cmd("VN")?;
        if ver.len() < 3 {
            return Err(MmError::LocallyDefined("No version response".into()));
        }

        let nf = self.cmd("NF")?;
        let mut n: u64 = Self::parse_last_word(&nf).parse().unwrap_or(6);
        if n != 6 && n != 8 {
            n = 6;
        }
        self.num_positions = n;
        self.labels = (0..n).map(|i| format!("State-{}", i)).collect();
        let allowed: Vec<String> = (0..n)
            .map(|i| i.to_string())
            .chain(std::iter::once(String::new()))
            .collect();
        let allowed_refs: Vec<&str> = allowed.iter().map(String::as_str).collect();
        self.props
            .set_allowed_values("Closed_Position", &allowed_refs)?;

        let pos_str = self.cmd("MP")?;
        self.position = Self::parse_last_word(&pos_str).parse().unwrap_or(0);

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
            "SpeedSetting" => Ok(PropertyValue::Integer(self.speed_setting)),
            "SerialResponse" => Ok(PropertyValue::String(self.manual_serial_answer.clone())),
            "Closed_Position" => Ok(PropertyValue::String(self.closed_position.clone())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u64;
                self.set_position(pos)
            }
            "Label" => {
                let label = val.as_str().to_string();
                self.set_position_by_label(&label)
            }
            "Closed_Position" => {
                let label = val.as_str().to_string();
                self.props.set(name, PropertyValue::String(label.clone()))?;
                self.closed_position = label;
                Ok(())
            }
            "SpeedSetting" => {
                let speed = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    let resp = self.cmd(&format!("SV {}", speed))?;
                    if Self::parse_last_word(&resp).parse::<i64>().ok() != Some(speed) {
                        return Err(MmError::SerialInvalidResponse);
                    }
                }
                self.speed_setting = speed;
                self.props.set(name, PropertyValue::Integer(speed))?;
                Ok(())
            }
            "SerialCommand" => {
                let command = val.as_str().to_string();
                self.manual_serial_answer = self.cmd(&command)?;
                self.props.set(name, PropertyValue::String(command))?;
                if let Some(e) = self.props.entry_mut("SerialResponse") {
                    e.value = PropertyValue::String(self.manual_serial_answer.clone());
                }
                Ok(())
            }
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
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
        self.cmd("?")
            .map(|resp| resp.trim() == "3")
            .unwrap_or(false)
    }
}

impl StateDevice for AsiFW1000 {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized {
            let resp = self.cmd(&format!("MP {}", pos))?;
            let pos_read: u64 = Self::parse_last_word(&resp)
                .parse()
                .map_err(|_| MmError::LocallyDefined("Error setting filter wheel".into()))?;
            if pos_read != pos {
                return Err(MmError::LocallyDefined("Error setting filter wheel".into()));
            }
        }
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        let pos_str = self.cmd("MP")?;
        Self::parse_last_word(&pos_str)
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)
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
        if !open && !self.closed_position.is_empty() {
            let pos = self
                .closed_position
                .parse::<u64>()
                .map_err(|_| MmError::InvalidPropertyValue)?;
            self.set_position(pos)?;
        }
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

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("VN\r", "FW-1000 v2.1")
            .expect("NF\r", "NF 8")
            .expect("MP\r", "MP 0")
    }

    #[test]
    fn initialize() {
        let t = make_transport().expect("MP\r", "MP 0");
        let mut fw = AsiFW1000::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        assert_eq!(fw.get_position().unwrap(), 0);
        assert_eq!(fw.get_number_of_positions(), 8);
    }

    #[test]
    fn set_position() {
        let t = make_transport()
            .expect("MP 5\r", "MP 5")
            .expect("MP\r", "MP 5");
        let mut fw = AsiFW1000::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_position(5).unwrap();
        assert_eq!(fw.get_position().unwrap(), 5);
    }

    #[test]
    fn out_of_range_rejected() {
        let mut fw = AsiFW1000::new().with_transport(Box::new(make_transport()));
        fw.initialize().unwrap();
        assert!(fw.set_position(8).is_err());
    }

    #[test]
    fn label_navigation() {
        let t = make_transport()
            .expect("MP 3\r", "MP 3")
            .expect("MP\r", "MP 3");
        let mut fw = AsiFW1000::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_position_label(3, "DAPI").unwrap();
        fw.set_position_by_label("DAPI").unwrap();
        assert_eq!(fw.get_position().unwrap(), 3);
    }

    #[test]
    fn invalid_position_count_falls_back_to_six() {
        let t = MockTransport::new()
            .expect("VN\r", "FW-1000 v2.1")
            .expect("NF\r", "NF 7")
            .expect("MP\r", "MP 0");
        let mut fw = AsiFW1000::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        assert_eq!(fw.get_number_of_positions(), 6);
        assert!(fw.set_position(6).is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(AsiFW1000::new().initialize().is_err());
    }

    #[test]
    fn port_is_pre_init_and_locked_after_initialize() {
        let mut fw = AsiFW1000::new().with_transport(Box::new(make_transport()));
        assert!(fw.props.entry("Port").unwrap().pre_init);
        fw.initialize().unwrap();
        assert_eq!(
            fw.set_property("Port", PropertyValue::String("COM2".into()))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn speed_setting_rejects_mismatched_echo_without_cache_update() {
        let t = make_transport().expect("SV 4\r", "SV 3");
        let mut fw = AsiFW1000::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        assert!(fw
            .set_property("SpeedSetting", PropertyValue::Integer(4))
            .is_err());
        assert_eq!(
            fw.get_property("SpeedSetting").unwrap(),
            PropertyValue::Integer(0)
        );
    }

    #[test]
    fn closed_position_uses_numeric_state_value() {
        let t = make_transport().expect("MP 4\r", "MP 4");
        let mut fw = AsiFW1000::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        assert!(fw
            .set_property("Closed_Position", PropertyValue::String("State-4".into()))
            .is_err());
        fw.set_property("Closed_Position", PropertyValue::String("4".into()))
            .unwrap();
        fw.set_gate_open(false).unwrap();
    }
}
