/// Sutter Lambda Parallel Arduino adapter.
///
/// ASCII serial protocol (terminated with `\r`):
///   Go online:          "O\r"  → "K"
///   Go offline:         "L\r"  → "K"
///   Get busy:           "B\r"  → "0" (idle) or "1" (busy)
///   Get position:       "W\r"  → single digit "0".."9"
///   Set position N:     "MN\r" → "K" or "E"
///   Get speed:          "F\r"  → single digit "0".."7"
///   Set speed N:        "SN\r" → "K" or "E"
///   Load sequence:      "Q<digits>\r" → "K" or "E"
///   Start sequencing:   "R\r"  → "K"
///   Stop sequencing:    "E\r"  → "K"
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

const NUM_POSITIONS: u64 = 10;

pub struct LambdaArduinoWheel {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    position: u64,
    speed: u8,
    use_sequencing: bool,
    labels: Vec<String>,
    gate_open: bool,
}

impl LambdaArduinoWheel {
    pub fn new() -> Self {
        let labels = (0..NUM_POSITIONS)
            .map(|i| format!("Position-{}", i))
            .collect();
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("Speed", PropertyValue::Integer(3), false)
            .unwrap();
        props.set_property_limits("Speed", 0.0, 7.0).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            position: 0,
            speed: 3,
            use_sequencing: false,
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

    fn send_recv(&self, cmd: &str) -> MmResult<String> {
        self.call_transport(|t| Ok(t.send_recv(cmd)?.trim().to_string()))
    }

    fn expect_ack(resp: &str) -> MmResult<()> {
        match resp {
            "K" => Ok(()),
            "E" => Err(MmError::LocallyDefined(
                "The device indicated an error".into(),
            )),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn go_online(&self, online: bool) -> MmResult<()> {
        let cmd = if online { "O\r" } else { "L\r" };
        let resp = self.send_recv(cmd)?;
        Self::expect_ack(&resp)
    }

    fn get_busy(&self) -> MmResult<bool> {
        let resp = self.send_recv("B\r")?;
        match resp.as_str() {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn wait_for_quiescent(&self) -> MmResult<()> {
        for _ in 0..100 {
            if !self.get_busy()? {
                return Ok(());
            }
        }
        Err(MmError::LocallyDefined("Device busy for too long".into()))
    }

    fn set_sequencing(&self, start: bool) -> MmResult<()> {
        let cmd = if start { "R\r" } else { "E\r" };
        let resp = self.send_recv(cmd)?;
        Self::expect_ack(&resp)
    }

    pub fn start_sequence(&self) -> MmResult<()> {
        self.set_sequencing(true)
    }

    pub fn stop_sequence(&self) -> MmResult<()> {
        self.set_sequencing(false)
    }

    pub fn load_position_sequence(&self, sequence: &[u64]) -> MmResult<()> {
        let mut cmd = String::from("Q");
        for &pos in sequence {
            if pos >= NUM_POSITIONS {
                return Err(MmError::UnknownPosition);
            }
            cmd.push(char::from(b'0' + pos as u8));
        }
        cmd.push('\r');
        let resp = self.send_recv(&cmd)?;
        Self::expect_ack(&resp)
    }

    fn get_wheel_position(&self) -> MmResult<u64> {
        let resp = self.send_recv("W\r")?;
        if resp.len() != 1 {
            return Err(MmError::SerialInvalidResponse);
        }
        let ch = resp.chars().next().unwrap();
        if !ch.is_ascii_digit() {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok((ch as u64) - ('0' as u64))
    }

    fn set_wheel_position(&self, pos: u64) -> MmResult<()> {
        let cmd = format!("M{}\r", pos);
        let resp = self.send_recv(&cmd)?;
        Self::expect_ack(&resp)
    }

    fn get_wheel_speed(&self) -> MmResult<u8> {
        let resp = self.send_recv("F\r")?;
        if resp.len() != 1 {
            return Err(MmError::SerialInvalidResponse);
        }
        let ch = resp.chars().next().unwrap();
        if !('0'..='7').contains(&ch) {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok((ch as u8) - b'0')
    }

    fn set_wheel_speed(&self, speed: u8) -> MmResult<()> {
        let cmd = format!("S{}\r", speed);
        let resp = self.send_recv(&cmd)?;
        Self::expect_ack(&resp)
    }
}

impl Default for LambdaArduinoWheel {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for LambdaArduinoWheel {
    fn name(&self) -> &str {
        "ArduinoWheelA"
    }
    fn description(&self) -> &str {
        "Sutter Lambda Parallel Arduino wheel"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.go_online(true)?;
        self.wait_for_quiescent()?;
        self.set_sequencing(false)?;
        let pos = self.get_wheel_position()?;
        self.position = pos;
        let spd = self.get_wheel_speed()?;
        self.speed = spd;
        if !self.props.has_property("UseSequencing") {
            self.props.define_property(
                "UseSequencing",
                PropertyValue::String(if self.use_sequencing {
                    "Yes".into()
                } else {
                    "No".into()
                }),
                false,
            )?;
            self.props
                .set_allowed_values("UseSequencing", &["No", "Yes"])?;
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.go_online(false);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" if self.initialized => {
                Ok(PropertyValue::Integer(self.get_wheel_position()? as i64))
            }
            "State" => Ok(PropertyValue::Integer(self.position as i64)),
            "Speed" if self.initialized => {
                Ok(PropertyValue::Integer(self.get_wheel_speed()? as i64))
            }
            "Speed" => Ok(PropertyValue::Integer(self.speed as i64)),
            "UseSequencing" if self.props.has_property("UseSequencing") => {
                Ok(PropertyValue::String(if self.use_sequencing {
                    "Yes".into()
                } else {
                    "No".into()
                }))
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u64;
                self.set_position(pos)
            }
            "Speed" => {
                let s = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u8;
                if s > 7 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.set_wheel_speed(s)?;
                self.speed = s;
                Ok(())
            }
            "UseSequencing" => {
                if !self.props.has_property(name) {
                    return self.props.set(name, val);
                }
                let v = match val {
                    PropertyValue::String(v) => v,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                match v.as_str() {
                    "Yes" => self.use_sequencing = true,
                    "No" => self.use_sequencing = false,
                    _ => return Err(MmError::InvalidPropertyValue),
                }
                self.props.set(name, PropertyValue::String(v))?;
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
        self.get_busy().unwrap_or(false)
    }
}

impl StateDevice for LambdaArduinoWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= NUM_POSITIONS {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized {
            self.set_wheel_position(pos)?;
        }
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        if self.initialized {
            return self.get_wheel_position();
        }
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

    fn make_initialized_wheel() -> LambdaArduinoWheel {
        let t = MockTransport::new()
            .expect("O\r", "K") // go online
            .expect("B\r", "0") // wait for quiescent
            .expect("E\r", "K") // disable sequencing
            .expect("W\r", "0") // get position → 0
            .expect("F\r", "3") // get speed → 3
            .expect("W\r", "0"); // live position read
        LambdaArduinoWheel::new().with_transport(Box::new(t))
    }

    #[test]
    fn initialize_queries_state() {
        let mut w = make_initialized_wheel();
        w.initialize().unwrap();
        assert_eq!(w.get_position().unwrap(), 0);
    }

    #[test]
    fn set_position() {
        let t = MockTransport::new()
            .expect("O\r", "K")
            .expect("B\r", "0")
            .expect("E\r", "K")
            .expect("W\r", "0")
            .expect("F\r", "3")
            .expect("M5\r", "K")
            .expect("W\r", "5");
        let mut w = LambdaArduinoWheel::new().with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position(5).unwrap();
        assert_eq!(w.get_position().unwrap(), 5);
    }

    #[test]
    fn set_speed() {
        let t = MockTransport::new()
            .expect("O\r", "K")
            .expect("B\r", "0")
            .expect("E\r", "K")
            .expect("W\r", "0")
            .expect("F\r", "3")
            .expect("S7\r", "K");
        let mut w = LambdaArduinoWheel::new().with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_wheel_speed(7).unwrap();
    }

    #[test]
    fn device_error_ack_is_distinct_from_invalid_response() {
        assert_eq!(
            LambdaArduinoWheel::expect_ack("E").unwrap_err(),
            MmError::LocallyDefined("The device indicated an error".into())
        );
        assert_eq!(
            LambdaArduinoWheel::expect_ack("?").unwrap_err(),
            MmError::SerialInvalidResponse
        );
    }

    #[test]
    fn busy_polls_live_status() {
        let t = MockTransport::new().expect("B\r", "1");
        let w = LambdaArduinoWheel::new().with_transport(Box::new(t));
        assert!(w.busy());
    }

    #[test]
    fn initialized_properties_read_live_state() {
        let t = MockTransport::new()
            .expect("O\r", "K")
            .expect("B\r", "0")
            .expect("E\r", "K")
            .expect("W\r", "2")
            .expect("F\r", "4")
            .expect("W\r", "6")
            .expect("F\r", "7");
        let mut w = LambdaArduinoWheel::new().with_transport(Box::new(t));
        w.initialize().unwrap();
        assert_eq!(w.get_property("State").unwrap(), PropertyValue::Integer(6));
        assert_eq!(w.get_property("Speed").unwrap(), PropertyValue::Integer(7));
    }

    #[test]
    fn out_of_range_rejected() {
        let mut w = make_initialized_wheel();
        w.initialize().unwrap();
        assert!(w.set_position(10).is_err());
    }

    #[test]
    fn no_transport_error() {
        let mut w = LambdaArduinoWheel::new();
        assert!(w.initialize().is_err());
    }

    #[test]
    fn initialize_creates_cached_use_sequencing_property() {
        let t = MockTransport::new()
            .expect("O\r", "K")
            .expect("B\r", "0")
            .expect("E\r", "K")
            .expect("W\r", "0")
            .expect("F\r", "3");
        let mut w = LambdaArduinoWheel::new().with_transport(Box::new(t));
        assert!(!w.has_property("UseSequencing"));
        assert!(w.get_property("UseSequencing").is_err());
        assert!(w
            .set_property("UseSequencing", PropertyValue::String("Yes".into()))
            .is_err());
        w.initialize().unwrap();
        assert_eq!(
            w.get_property("UseSequencing").unwrap(),
            PropertyValue::String("No".into())
        );
        w.set_property("UseSequencing", PropertyValue::String("Yes".into()))
            .unwrap();
        assert_eq!(
            w.get_property("UseSequencing").unwrap(),
            PropertyValue::String("Yes".into())
        );
        assert!(w
            .set_property("UseSequencing", PropertyValue::String("Maybe".into()))
            .is_err());
    }

    #[test]
    fn sequence_helpers_use_upstream_commands() {
        let t = MockTransport::new()
            .expect("Q013\r", "K")
            .expect("R\r", "K")
            .expect("E\r", "K");
        let w = LambdaArduinoWheel::new().with_transport(Box::new(t));
        w.load_position_sequence(&[0, 1, 3]).unwrap();
        w.start_sequence().unwrap();
        w.stop_sequence().unwrap();
        assert!(w.load_position_sequence(&[10]).is_err());
    }
}
