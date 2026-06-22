/// Sutter Lambda 2 filter wheel — binary serial protocol.
///
/// Wheel A: send `(speed << 4) | position`, recv `[echo, 0x0D]`
/// Wheel B: send `0x80 | (speed << 4) | position`, recv `[echo, 0x0D]`
/// Wheel C: send `[0xFC, (speed << 4) | position]`, recv `[0xFC, echo, 0x0D]`
///
/// Speed 0–7, position 0–9.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelId {
    A,
    B,
    C,
}

pub struct Lambda2Wheel {
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

impl Lambda2Wheel {
    pub fn new(wheel: WheelId) -> Self {
        let num_positions: u8 = 10;
        let labels: Vec<String> = (0..num_positions)
            .map(|i| format!("Filter-{}", i))
            .collect();
        let mut props = PropertyMap::new();
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
            .define_property("State Change Speed", PropertyValue::Integer(3), false)
            .unwrap();
        props
            .set_property_limits("State Change Speed", 0.0, 7.0)
            .unwrap();
        let wname = match wheel {
            WheelId::A => "A",
            WheelId::B => "B",
            WheelId::C => "C",
        };
        props
            .define_property("Wheel", PropertyValue::String(wname.into()), true)
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
}

impl Device for Lambda2Wheel {
    fn name(&self) -> &str {
        match self.wheel {
            WheelId::A => "Wheel-A",
            WheelId::B => "Wheel-B",
            WheelId::C => "Wheel-C",
        }
    }
    fn description(&self) -> &str {
        "Sutter Lambda Filter Wheel"
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
            "Speed" | "State Change Speed" => Ok(PropertyValue::Integer(self.speed as i64)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u8;
                self.set_position(pos as u64)
            }
            "Label" => {
                let label = val.as_str().to_string();
                self.set_position_by_label(&label)
            }
            "Speed" | "State Change Speed" => {
                let s = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u8;
                if s > 7 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.speed = s;
                self.props.set("Speed", PropertyValue::Integer(s as i64))?;
                self.props
                    .set("State Change Speed", PropertyValue::Integer(s as i64))
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

impl StateDevice for Lambda2Wheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions as u64 {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized {
            self.send_move(pos as u8)?;
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
    fn wheel_a_initialize_does_not_move_and_move() {
        let t = MockTransport::new().expect_binary(&[0x35, 0x0D]); // move to pos 5 (0x30|5=0x35)
        let mut w = Lambda2Wheel::new(WheelId::A).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position(5).unwrap();
        assert_eq!(w.get_position().unwrap(), 5);
    }

    #[test]
    fn wheel_b_initialize_and_move() {
        // Wheel B cmd = 0x80 | (3<<4) | pos
        let t = MockTransport::new().expect_binary(&[0xB3, 0x0D]); // move to 3: 0x80|0x30|3=0xB3
        let mut w = Lambda2Wheel::new(WheelId::B).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position(3).unwrap();
        assert_eq!(w.get_position().unwrap(), 3);
    }

    #[test]
    fn wheel_c_initialize_and_move() {
        // Wheel C: send [0xFC, payload], recv [0xFC, payload, 0x0D]
        let t = MockTransport::new().expect_binary(&[0xFC, 0x32, 0x0D]); // move to 2: (3<<4)|2=0x32
        let mut w = Lambda2Wheel::new(WheelId::C).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position(2).unwrap();
        assert_eq!(w.get_position().unwrap(), 2);
    }

    #[test]
    fn wheel_c_accepts_stripped_position_echo() {
        let t = MockTransport::new().expect_binary(&[0xFC, 0x02, 0x0D]);
        let mut w = Lambda2Wheel::new(WheelId::C).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position(2).unwrap();
        assert_eq!(w.get_position().unwrap(), 2);
    }

    #[test]
    fn out_of_range_rejected() {
        let t = MockTransport::new();
        let mut w = Lambda2Wheel::new(WheelId::A).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert!(w.set_position(10).is_err());
    }

    #[test]
    fn label_navigation() {
        let t = MockTransport::new().expect_binary(&[0x34, 0x0D]);
        let mut w = Lambda2Wheel::new(WheelId::A).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position_label(4, "FITC").unwrap();
        w.set_position_by_label("FITC").unwrap();
        assert_eq!(w.get_position().unwrap(), 4);
    }

    #[test]
    fn rejects_wrong_echo_without_mutating_position() {
        let t = MockTransport::new().expect_binary(&[0x35, 0x0D]);
        let mut w = Lambda2Wheel::new(WheelId::A).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert_eq!(w.set_position(4), Err(MmError::SerialInvalidResponse));
        assert_eq!(w.get_position().unwrap(), 0);
    }

    #[test]
    fn upstream_state_change_speed_name_controls_move_byte() {
        let t = MockTransport::new().expect_binary(&[0x75, 0x0D]);
        let mut w = Lambda2Wheel::new(WheelId::A).with_transport(Box::new(t));
        w.set_property("State Change Speed", PropertyValue::Integer(7))
            .unwrap();
        assert_eq!(w.get_property("Speed").unwrap(), PropertyValue::Integer(7));
        w.initialize().unwrap();
        w.set_position(5).unwrap();
    }

    #[test]
    fn invalid_state_change_speed_preserves_cached_speed() {
        let mut w = Lambda2Wheel::new(WheelId::A);
        assert_eq!(
            w.set_property("State Change Speed", PropertyValue::Integer(8)),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            w.get_property("State Change Speed").unwrap(),
            PropertyValue::Integer(3)
        );
    }
}
