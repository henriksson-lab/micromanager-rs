/// ASI FW1000 Filter Wheel.
///
/// Protocol (TX `\r`, RX echo of command + data):
///   Responses echo the command; data follows after the echo.
///   `VN \r`         → "VN <version>"  firmware version
///   `VB 6\r`        → echo            set verbose=6 (disables prompts)
///   `FW<n>\r`       → echo            select wheel 0 or 1
///   `NF\r`          → "NF <N>"        number of filter positions
///   `MP\r`          → "MP <pos>"      current position (0-based)
///   `MP <pos>\r`    → echo            set position (0-based)
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

pub struct AsiFw1000FilterWheel {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    wheel: u8,
    num_positions: u64,
    position: u64,
    labels: Vec<String>,
    gate_open: bool,
    speed_setting: i64,
    manual_serial_answer: String,
    closed_position: String,
}

impl AsiFw1000FilterWheel {
    pub fn new(wheel: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property(
                "ASIFilterWheelNumber",
                PropertyValue::Integer(wheel as i64),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("ASIFilterWheelNumber", &["0", "1"])
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
        let num = 6u64;
        let labels: Vec<String> = (0..num).map(|i| format!("Position-{}", i + 1)).collect();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            wheel,
            num_positions: num,
            position: 0,
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
        let full = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    /// Parse the data field from an echo response "CMD <data>".
    fn parse_last_word(resp: &str) -> &str {
        resp.split_whitespace().last().unwrap_or("")
    }
}

impl Default for AsiFw1000FilterWheel {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Device for AsiFw1000FilterWheel {
    fn name(&self) -> &str {
        "ASIFilterWheel"
    }
    fn description(&self) -> &str {
        "ASIFW1000 FilterWheel"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        // Match upstream FilterWheelSA initialization: disable prompt characters first.
        let vb_resp = self.cmd("VB 6")?;
        if !vb_resp.starts_with("VB 6") {
            return Err(MmError::SerialInvalidResponse);
        }
        // Select wheel
        self.cmd(&format!("FW{}", self.wheel))?;
        // Query number of positions
        let nf_resp = self.cmd("NF")?;
        let mut n: u64 = Self::parse_last_word(&nf_resp).parse().unwrap_or(6);
        if n != 6 && n != 8 {
            n = 6;
        }
        self.num_positions = n;
        self.labels = (0..n).map(|i| format!("Position-{}", i + 1)).collect();
        let allowed: Vec<String> = self
            .labels
            .iter()
            .cloned()
            .chain(std::iter::once(String::new()))
            .collect();
        let allowed_refs: Vec<&str> = allowed.iter().map(String::as_str).collect();
        self.props
            .set_allowed_values("Closed_Position", &allowed_refs)?;
        // Query current position
        let mp_resp = self.cmd("MP")?;
        self.position = Self::parse_last_word(&mp_resp).parse().unwrap_or(0);
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "ASIFilterWheelNumber" => Ok(PropertyValue::Integer(self.wheel as i64)),
            "State" => Ok(PropertyValue::Integer(self.get_position()? as i64)),
            "Label" => Ok(PropertyValue::String(
                self.labels
                    .get(self.get_position()? as usize)
                    .cloned()
                    .unwrap_or_default(),
            )),
            "Closed_Position" => Ok(PropertyValue::String(self.closed_position.clone())),
            "SpeedSetting" => Ok(PropertyValue::Integer(self.speed_setting)),
            "SerialResponse" => Ok(PropertyValue::String(self.manual_serial_answer.clone())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "ASIFilterWheelNumber" => {
                if self.initialized {
                    return Err(MmError::CanNotSetProperty);
                }
                let wheel = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if wheel != 0 && wheel != 1 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.wheel = wheel as u8;
                self.props.set(name, PropertyValue::Integer(wheel))
            }
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if pos < 0 {
                    return Err(MmError::UnknownPosition);
                }
                self.set_position(pos as u64)
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
                self.props.set(name, PropertyValue::Integer(speed))?;
                if self.initialized {
                    let resp = self.cmd(&format!("SV {}", speed))?;
                    if !resp.starts_with("SV") {
                        return Err(MmError::SerialInvalidResponse);
                    }
                }
                self.speed_setting = speed;
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

impl StateDevice for AsiFw1000FilterWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        let resp = self.cmd(&format!("MP{}", pos))?;
        if !resp.starts_with("MP") {
            return Err(MmError::LocallyDefined("Error setting filter wheel".into()));
        }
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        let mp_resp = self.cmd("MP")?;
        Self::parse_last_word(&mp_resp)
            .parse::<u64>()
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
            let label = self.closed_position.clone();
            self.set_position_by_label(&label)?;
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

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect("VB 6\r", "VB 6")
            .expect("FW0\r", "FW0") // select wheel 0
            .expect("NF\r", "NF 6") // 6 positions
            .expect("MP\r", "MP 0") // current position 0
    }

    #[test]
    fn initialize() {
        let t = make_init_transport().expect("MP\r", "MP 0");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert_eq!(w.get_number_of_positions(), 6);
        assert_eq!(w.get_position().unwrap(), 0);
        assert_eq!(w.get_position_label(0).unwrap(), "Position-1");
        assert_eq!(w.get_position_label(5).unwrap(), "Position-6");
    }

    #[test]
    fn set_position() {
        let t = make_init_transport()
            .expect("MP3\r", "MP3")
            .expect("MP\r", "MP 3");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position(3).unwrap();
        assert_eq!(w.get_position().unwrap(), 3);
    }

    #[test]
    fn label_roundtrip() {
        let t = make_init_transport()
            .expect("MP2\r", "MP2")
            .expect("MP\r", "MP 2");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position_label(2, "DAPI").unwrap();
        assert_eq!(w.get_position_label(2).unwrap(), "DAPI");
        w.set_position_by_label("DAPI").unwrap();
        assert_eq!(w.get_position().unwrap(), 2);
    }

    #[test]
    fn invalid_position_count_falls_back_to_six() {
        let t = MockTransport::new()
            .expect("VB 6\r", "VB 6")
            .expect("FW0\r", "FW0")
            .expect("NF\r", "NF 7")
            .expect("MP\r", "MP 0");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert_eq!(w.get_number_of_positions(), 6);
        assert!(w.set_position(6).is_err());
    }

    #[test]
    fn set_position_rejects_non_mp_echo() {
        let t = make_init_transport().expect("MP3\r", "ERR");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert!(w.set_position(3).is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(AsiFw1000FilterWheel::new(0).initialize().is_err());
    }

    #[test]
    fn state_property_queries_live_position() {
        let t = make_init_transport().expect("MP\r", "MP 4");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert_eq!(w.get_property("State").unwrap(), PropertyValue::Integer(4));
    }
}
