/// Sutter Lambda filter wheel — binary serial protocol.
///
/// Binary protocol:
///   Wheel A position: send 1 byte = `(speed << 4) | position`
///                     response = [echo_byte, 0x0D]
///   Wheel B position: send 1 byte = `0x80 | (speed << 4) | position`
///                     response = [echo_byte, 0x0D]
///   Wheel C position: send 2 bytes = `[0xFC, (speed << 4) | position]`
///                     response = [0xFC, echo_byte, 0x0D]
///
/// Speed 0–7 (encoded in bits 4–6), position 0–9 (encoded in bits 0–3).
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

/// Which wheel on the Lambda controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelId {
    A,
    B,
    C,
}

pub struct LambdaWheel {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    wheel: WheelId,
    position: u8,
    speed: u8,
    num_positions: u8,
    labels: Vec<String>,
    gate_open: bool,
}

impl LambdaWheel {
    pub fn new(wheel: WheelId) -> Self {
        let num_positions: u8 = 10;
        let labels: Vec<String> = (0..num_positions)
            .map(|i| format!("Filter-{}", i))
            .collect();
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("Label", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Speed", PropertyValue::Integer(3), false)
            .unwrap();
        props.set_property_limits("Speed", 0.0, 7.0).unwrap();
        props
            .define_property(
                "Closed_Position",
                PropertyValue::String(String::new()),
                false,
            )
            .unwrap();
        let wheel_name = match wheel {
            WheelId::A => "A",
            WheelId::B => "B",
            WheelId::C => "C",
        };
        props
            .define_property("Wheel", PropertyValue::String(wheel_name.into()), true)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            wheel,
            position: 0,
            speed: 3,
            num_positions,
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

    /// Send the wheel-move command and wait for echo + CR.
    fn send_move(&mut self, pos: u8) -> MmResult<()> {
        let speed = self.speed;
        let wheel = self.wheel;
        let payload = (speed << 4) | pos;
        self.call_transport(|t| {
            match wheel {
                WheelId::A => {
                    let cmd = payload;
                    t.send_bytes(&[cmd])?;
                    let resp = t.receive_bytes(2)?;
                    if resp.len() != 2 || resp[1] != 0x0D || (resp[0] != cmd && resp[0] != pos) {
                        return Err(MmError::SerialInvalidResponse);
                    }
                }
                WheelId::B => {
                    let cmd = 0x80 | payload;
                    t.send_bytes(&[cmd])?;
                    let resp = t.receive_bytes(2)?;
                    if resp.len() != 2 || resp[1] != 0x0D || (resp[0] != cmd && resp[0] != pos) {
                        return Err(MmError::SerialInvalidResponse);
                    }
                }
                WheelId::C => {
                    t.send_bytes(&[0xFC, payload])?;
                    let resp = t.receive_bytes(3)?;
                    if resp.len() != 3
                        || resp[2] != 0x0D
                        || resp[0] != 0xFC
                        || (resp[1] != payload && resp[1] != pos)
                    {
                        return Err(MmError::SerialInvalidResponse);
                    }
                }
            }
            Ok(())
        })
    }

    fn closed_position(&self) -> u8 {
        self.props
            .get("Closed_Position")
            .ok()
            .and_then(|v| v.as_str().parse::<u8>().ok())
            .filter(|&pos| pos < self.num_positions)
            .unwrap_or(0)
    }

    fn set_physical_position(&mut self, pos: u8) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized {
            self.send_move(pos)?;
        }
        Ok(())
    }
}

impl Device for LambdaWheel {
    fn name(&self) -> &str {
        match self.wheel {
            WheelId::A => "Wheel-A",
            WheelId::B => "Wheel-B",
            WheelId::C => "Wheel-C",
        }
    }
    fn description(&self) -> &str {
        "Sutter Lambda filter wheel adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.position = 0;
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
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u8;
                self.set_position(pos as u64)
            }
            "Label" => {
                let label = val.as_str().to_string();
                self.set_position_by_label(&label)
            }
            "Speed" => {
                let s = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u8;
                if s > 7 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.speed = s;
                self.props.set(name, PropertyValue::Integer(s as i64))
            }
            "Closed_Position" => {
                let closed = val.as_str();
                if !closed.is_empty() {
                    let pos = closed
                        .parse::<u8>()
                        .map_err(|_| MmError::InvalidPropertyValue)?;
                    if pos >= self.num_positions {
                        return Err(MmError::UnknownPosition);
                    }
                }
                self.props.set(name, val)
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

impl StateDevice for LambdaWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions as u64 {
            return Err(MmError::UnknownPosition);
        }
        let physical_pos = if self.gate_open {
            pos as u8
        } else {
            self.closed_position()
        };
        if self.gate_open || self.initialized {
            self.set_physical_position(physical_pos)?;
        }
        self.position = pos as u8;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        Ok(self.position as u64)
    }

    fn get_number_of_positions(&self) -> u64 {
        self.num_positions as u64
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
        if pos >= self.num_positions as u64 {
            return Err(MmError::UnknownPosition);
        }
        self.labels[pos as usize] = label.to_string();
        Ok(())
    }

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        if self.gate_open == open {
            return Ok(());
        }
        let physical_pos = if open {
            self.position
        } else {
            self.closed_position()
        };
        self.set_physical_position(physical_pos)?;
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

    fn make_wheel_a() -> LambdaWheel {
        let t = MockTransport::new();
        LambdaWheel::new(WheelId::A).with_transport(Box::new(t))
    }

    #[test]
    fn initialize_does_not_move_wheel() {
        let mut wheel = make_wheel_a();
        wheel.initialize().unwrap();
        assert_eq!(wheel.get_position().unwrap(), 0);
    }

    #[test]
    fn set_position_wheel_a() {
        // Set to position 3: cmd = (3<<4)|3 = 0x33
        let t = MockTransport::new().expect_binary(&[0x33, 0x0D]); // move to 3
        let mut wheel = LambdaWheel::new(WheelId::A).with_transport(Box::new(t));
        wheel.initialize().unwrap();
        wheel.set_position(3).unwrap();
        assert_eq!(wheel.get_position().unwrap(), 3);
    }

    #[test]
    fn set_position_wheel_b() {
        // Wheel B: cmd = 0x80 | (3<<4) | 5 = 0x80 | 0x30 | 0x05 = 0xB5
        let t = MockTransport::new().expect_binary(&[0xB5, 0x0D]); // move B to 5
        let mut wheel = LambdaWheel::new(WheelId::B).with_transport(Box::new(t));
        wheel.initialize().unwrap();
        wheel.set_position(5).unwrap();
        assert_eq!(wheel.get_position().unwrap(), 5);
    }

    #[test]
    fn set_position_wheel_c() {
        // Wheel C: send [0xFC, (3<<4)|2=0x32], recv [0xFC, 0x32, 0x0D]
        let t = MockTransport::new().expect_binary(&[0xFC, 0x32, 0x0D]); // move to 2
        let mut wheel = LambdaWheel::new(WheelId::C).with_transport(Box::new(t));
        wheel.initialize().unwrap();
        wheel.set_position(2).unwrap();
        assert_eq!(wheel.get_position().unwrap(), 2);
    }

    #[test]
    fn wheel_c_accepts_stripped_position_echo() {
        let t = MockTransport::new().expect_binary(&[0xFC, 0x02, 0x0D]);
        let mut wheel = LambdaWheel::new(WheelId::C).with_transport(Box::new(t));
        wheel.initialize().unwrap();
        wheel.set_position(2).unwrap();
        assert_eq!(wheel.get_position().unwrap(), 2);
    }

    #[test]
    fn out_of_range_rejected() {
        let mut wheel = make_wheel_a();
        wheel.initialize().unwrap();
        assert!(wheel.set_position(10).is_err());
    }

    #[test]
    fn label_navigation() {
        let t = MockTransport::new().expect_binary(&[0x34, 0x0D]); // move to 4
        let mut wheel = LambdaWheel::new(WheelId::A).with_transport(Box::new(t));
        wheel.initialize().unwrap();
        wheel.set_position_label(4, "DAPI").unwrap();
        wheel.set_position_by_label("DAPI").unwrap();
        assert_eq!(wheel.get_position().unwrap(), 4);
    }

    #[test]
    fn rejects_wrong_echo_without_mutating_position() {
        let t = MockTransport::new().expect_binary(&[0x35, 0x0D]);
        let mut wheel = LambdaWheel::new(WheelId::A).with_transport(Box::new(t));
        wheel.initialize().unwrap();
        assert_eq!(wheel.set_position(4), Err(MmError::SerialInvalidResponse));
        assert_eq!(wheel.get_position().unwrap(), 0);
    }

    #[test]
    fn initialized_port_change_is_rejected_and_preserved() {
        let t = MockTransport::new();
        let mut wheel = LambdaWheel::new(WheelId::A).with_transport(Box::new(t));
        wheel
            .set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        wheel.initialize().unwrap();
        assert_eq!(
            wheel.set_property("Port", PropertyValue::String("COM2".into())),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            wheel.get_property("Port").unwrap(),
            PropertyValue::String("COM1".into())
        );
    }

    #[test]
    fn closed_gate_moves_to_closed_position_but_keeps_requested_state() {
        let t = MockTransport::new()
            .expect_binary(&[0x32, 0x0D])
            .expect_binary(&[0x32, 0x0D]);
        let mut wheel = LambdaWheel::new(WheelId::A).with_transport(Box::new(t));
        wheel
            .set_property("Closed_Position", PropertyValue::String("2".into()))
            .unwrap();
        wheel.initialize().unwrap();

        wheel.set_gate_open(false).unwrap();
        wheel.set_position(5).unwrap();

        assert_eq!(wheel.get_position().unwrap(), 5);
        assert!(!wheel.get_gate_open().unwrap());
    }

    #[test]
    fn reopening_gate_moves_to_cached_state() {
        let t = MockTransport::new()
            .expect_binary(&[0x32, 0x0D])
            .expect_binary(&[0x35, 0x0D]);
        let mut wheel = LambdaWheel::new(WheelId::A).with_transport(Box::new(t));
        wheel
            .set_property("Closed_Position", PropertyValue::String("2".into()))
            .unwrap();
        wheel.initialize().unwrap();

        wheel.set_gate_open(false).unwrap();
        wheel.position = 5;
        wheel.set_gate_open(true).unwrap();

        assert!(wheel.get_gate_open().unwrap());
    }
}
