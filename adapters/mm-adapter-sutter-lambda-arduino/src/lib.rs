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

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const NUM_POSITIONS: u64 = 10;

pub struct LambdaArduinoWheel {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position: u64,
    speed: u8,
    labels: Vec<String>,
    gate_open: bool,
}

impl LambdaArduinoWheel {
    pub fn new() -> Self {
        let labels = (0..NUM_POSITIONS).map(|i| format!("Position-{}", i)).collect();
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("State", PropertyValue::Integer(0), false).unwrap();
        props.define_property("Speed", PropertyValue::Integer(3), false).unwrap();
        props.set_property_limits("Speed", 0.0, 7.0).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            position: 0,
            speed: 3,
            labels,
            gate_open: true,
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

    fn send_recv(&mut self, cmd: &str) -> MmResult<String> {
        self.call_transport(|t| Ok(t.send_recv(cmd)?.trim().to_string()))
    }

    fn go_online(&mut self, online: bool) -> MmResult<()> {
        let cmd = if online { "O\r" } else { "L\r" };
        let resp = self.send_recv(cmd)?;
        match resp.as_str() {
            "K" => Ok(()),
            "E" => Err(MmError::SerialInvalidResponse),
            _   => Err(MmError::SerialInvalidResponse),
        }
    }

    fn get_wheel_position(&mut self) -> MmResult<u64> {
        let resp = self.send_recv("W\r")?;
        if resp.len() != 1 { return Err(MmError::SerialInvalidResponse); }
        let ch = resp.chars().next().unwrap();
        if !ch.is_ascii_digit() { return Err(MmError::SerialInvalidResponse); }
        Ok((ch as u64) - ('0' as u64))
    }

    fn set_wheel_position(&mut self, pos: u64) -> MmResult<()> {
        let cmd = format!("M{}\r", pos);
        let resp = self.send_recv(&cmd)?;
        match resp.as_str() {
            "K" => Ok(()),
            "E" => Err(MmError::SerialInvalidResponse),
            _   => Err(MmError::SerialInvalidResponse),
        }
    }

    fn get_wheel_speed(&mut self) -> MmResult<u8> {
        let resp = self.send_recv("F\r")?;
        if resp.len() != 1 { return Err(MmError::SerialInvalidResponse); }
        let ch = resp.chars().next().unwrap();
        if !('0'..='7').contains(&ch) { return Err(MmError::SerialInvalidResponse); }
        Ok((ch as u8) - b'0')
    }

    fn set_wheel_speed(&mut self, speed: u8) -> MmResult<()> {
        let cmd = format!("S{}\r", speed);
        let resp = self.send_recv(&cmd)?;
        match resp.as_str() {
            "K" => Ok(()),
            _   => Err(MmError::SerialInvalidResponse),
        }
    }
}

impl Default for LambdaArduinoWheel {
    fn default() -> Self { Self::new() }
}

impl Device for LambdaArduinoWheel {
    fn name(&self) -> &str { "LambdaArduinoWheel" }
    fn description(&self) -> &str { "Sutter Lambda Parallel Arduino wheel" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.go_online(true)?;
        let pos = self.get_wheel_position()?;
        self.position = pos;
        let spd = self.get_wheel_speed()?;
        self.speed = spd;
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
            "State" => Ok(PropertyValue::Integer(self.position as i64)),
            "Speed" => Ok(PropertyValue::Integer(self.speed as i64)),
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
                if s > 7 { return Err(MmError::InvalidPropertyValue); }
                self.set_wheel_speed(s)?;
                self.speed = s;
                Ok(())
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::State }
    fn busy(&self) -> bool { false }
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

    fn get_position(&self) -> MmResult<u64> { Ok(self.position) }
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

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> { Ok(self.gate_open) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_initialized_wheel() -> LambdaArduinoWheel {
        let t = MockTransport::new()
            .expect("O\r", "K")   // go online
            .expect("W\r", "0")   // get position → 0
            .expect("F\r", "3");  // get speed → 3
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
            .expect("W\r", "0")
            .expect("F\r", "3")
            .expect("M5\r", "K");
        let mut w = LambdaArduinoWheel::new().with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position(5).unwrap();
        assert_eq!(w.get_position().unwrap(), 5);
    }

    #[test]
    fn set_speed() {
        let t = MockTransport::new()
            .expect("O\r", "K")
            .expect("W\r", "0")
            .expect("F\r", "3")
            .expect("S7\r", "K");
        let mut w = LambdaArduinoWheel::new().with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_wheel_speed(7).unwrap();
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
}
