/// ASI FW1000 Filter Wheel.
///
/// Protocol (TX `\r`, RX echo of command + data):
///   Responses echo the command; data follows after the echo.
///   `VN \r`         → "VN <version>"  firmware version
///   `VB 6\r`        → echo            set verbose=6 (disables prompts)
///   `FW <n>\r`      → echo            select wheel 0 or 1
///   `NF\r`          → "NF <N>"        number of filter positions
///   `MP\r`          → "MP <pos>"      current position (0-based)
///   `MP <pos>\r`    → echo            set position (0-based)
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
#[cfg(not(test))]
use std::sync::atomic::{AtomicI8, Ordering};

#[cfg(not(test))]
static ACTIVE_WHEEL: AtomicI8 = AtomicI8::new(-1);

#[cfg(test)]
thread_local! {
    static ACTIVE_WHEEL: Cell<i8> = const { Cell::new(-1) };
}

#[cfg(not(test))]
fn active_wheel_load() -> i8 {
    ACTIVE_WHEEL.load(Ordering::SeqCst)
}

#[cfg(test)]
fn active_wheel_load() -> i8 {
    ACTIVE_WHEEL.with(Cell::get)
}

#[cfg(not(test))]
fn active_wheel_store(wheel: i8) {
    ACTIVE_WHEEL.store(wheel, Ordering::SeqCst);
}

#[cfg(test)]
fn active_wheel_store(wheel: i8) {
    ACTIVE_WHEEL.with(|active| active.set(wheel));
}

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
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
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
        let labels: Vec<String> = (0..num).map(|i| format!("State-{}", i)).collect();
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

    fn select_wheel_if_needed(&self) -> MmResult<()> {
        if active_wheel_load() == self.wheel as i8 {
            return Ok(());
        }
        self.select_wheel()
    }

    fn select_wheel(&self) -> MmResult<()> {
        let resp = self.cmd(&format!("FW {}", self.wheel))?;
        if Self::parse_last_word(&resp).parse::<u8>().ok() != Some(self.wheel) {
            return Err(MmError::SerialInvalidResponse);
        }
        active_wheel_store(self.wheel as i8);
        Ok(())
    }

    /// Parse the data field from an echo response "CMD <data>".
    fn parse_last_word(resp: &str) -> &str {
        resp.split_whitespace().last().unwrap_or("")
    }

    fn clamp_position_i64(&self, pos: i64) -> u64 {
        if pos < 0 {
            0
        } else if pos as u64 >= self.num_positions {
            self.num_positions.saturating_sub(1)
        } else {
            pos as u64
        }
    }

    fn clamp_position_u64(&self, pos: u64) -> u64 {
        if pos >= self.num_positions {
            self.num_positions.saturating_sub(1)
        } else {
            pos
        }
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
        active_wheel_store(-1);
        // Match upstream FilterWheelSA initialization: disable prompt characters first.
        let vb_resp = self.cmd("VB 6")?;
        if !vb_resp.starts_with("VB 6") {
            return Err(MmError::SerialInvalidResponse);
        }
        self.select_wheel()?;
        // Query number of positions
        let nf_resp = self.cmd("NF")?;
        let mut n: u64 = Self::parse_last_word(&nf_resp).parse().unwrap_or(6);
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
        // Query current position after the forced initialization-time wheel select.
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
                self.set_position(self.clamp_position_i64(pos))
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
                    self.select_wheel_if_needed()?;
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
                self.select_wheel_if_needed()?;
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
        let _ = self.select_wheel_if_needed();
        self.cmd("?")
            .map(|resp| resp.trim() == "3")
            .unwrap_or(false)
    }
}

impl StateDevice for AsiFw1000FilterWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        let pos = self.clamp_position_u64(pos);
        self.select_wheel_if_needed()?;
        let resp = self.cmd(&format!("MP {}", pos))?;
        if Self::parse_last_word(&resp).parse::<u64>().ok() != Some(pos) {
            return Err(MmError::LocallyDefined("Error setting filter wheel".into()));
        }
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        self.select_wheel_if_needed()?;
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

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect("VB 6\r", "VB 6")
            .expect("FW 0\r", "FW 0") // select wheel 0
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
        assert_eq!(w.get_position_label(0).unwrap(), "State-0");
        assert_eq!(w.get_position_label(5).unwrap(), "State-5");
    }

    #[test]
    fn set_position() {
        let t = make_init_transport()
            .expect("MP 3\r", "MP 3")
            .expect("MP\r", "MP 3");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position(3).unwrap();
        assert_eq!(w.get_position().unwrap(), 3);
    }

    #[test]
    fn label_roundtrip() {
        let t = make_init_transport()
            .expect("MP 2\r", "MP 2")
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
            .expect("FW 0\r", "FW 0")
            .expect("NF\r", "NF 7")
            .expect("MP\r", "MP 0")
            .expect("MP 5\r", "MP 5")
            .expect("MP\r", "MP 5");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert_eq!(w.get_number_of_positions(), 6);
        w.set_position(6).unwrap();
        assert_eq!(w.get_position().unwrap(), 5);
    }

    #[test]
    fn set_position_rejects_non_mp_echo() {
        let t = make_init_transport().expect("MP 3\r", "ERR");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert!(w.set_position(3).is_err());
    }

    #[test]
    fn set_position_rejects_mismatched_mp_echo() {
        let t = make_init_transport().expect("MP 3\r", "MP 2");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert!(w.set_position(3).is_err());
    }

    #[test]
    fn initialize_rejects_mismatched_wheel_echo() {
        let t = MockTransport::new()
            .expect("VB 6\r", "VB 6")
            .expect("FW 0\r", "FW 1");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        assert!(w.initialize().is_err());
    }

    #[test]
    fn speed_setting_rejects_mismatched_echo_without_cache_update() {
        let t = make_init_transport().expect("SV 4\r", "SV 3");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert!(w
            .set_property("SpeedSetting", PropertyValue::Integer(4))
            .is_err());
        assert_eq!(
            w.get_property("SpeedSetting").unwrap(),
            PropertyValue::Integer(0)
        );
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

    #[test]
    fn reselects_wheel_before_later_commands_when_global_active_changed() {
        active_wheel_store(1);
        let t = MockTransport::new()
            .expect("VB 6\r", "VB 6")
            .expect("FW 0\r", "FW 0")
            .expect("NF\r", "NF 6")
            .expect("MP\r", "MP 0")
            .expect("FW 0\r", "FW 0")
            .expect("MP 3\r", "MP 3");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        active_wheel_store(1);
        w.set_position(3).unwrap();
    }

    #[test]
    fn reselects_wheel_before_live_position_query() {
        let t = make_init_transport()
            .expect("FW 0\r", "FW 0")
            .expect("MP\r", "MP 4");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        active_wheel_store(1);
        assert_eq!(w.get_position().unwrap(), 4);
    }

    #[test]
    fn port_is_preinit_only() {
        let t = make_init_transport();
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        w.initialize().unwrap();
        assert!(w
            .set_property("Port", PropertyValue::String("COM2".into()))
            .is_err());
        assert_eq!(
            w.get_property("Port").unwrap(),
            PropertyValue::String("COM1".into())
        );
    }

    #[test]
    fn closed_position_uses_numeric_state_value() {
        let t = make_init_transport().expect("MP 4\r", "MP 4");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert!(w
            .set_property(
                "Closed_Position",
                PropertyValue::String("Position-5".into())
            )
            .is_err());
        w.set_property("Closed_Position", PropertyValue::String("4".into()))
            .unwrap();
        w.set_gate_open(false).unwrap();
    }

    #[test]
    fn state_property_clamps_negative_and_high_values() {
        let t = make_init_transport()
            .expect("MP 0\r", "MP 0")
            .expect("MP\r", "MP 0")
            .expect("MP 5\r", "MP 5")
            .expect("MP\r", "MP 5");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_property("State", PropertyValue::Integer(-2)).unwrap();
        assert_eq!(w.get_position().unwrap(), 0);
        w.set_property("State", PropertyValue::Integer(99)).unwrap();
        assert_eq!(w.get_position().unwrap(), 5);
    }
}
