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
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, StateDevice};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

/// Which wheel on the Lambda controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelId { A, B, C }

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
        let labels: Vec<String> = (0..num_positions).map(|i| format!("Position-{}", i)).collect();
        let mut props = PropertyMap::new();
        props.define_property("State", PropertyValue::Integer(0), false).unwrap();
        props.define_property("Label", PropertyValue::String("Position-0".into()), false).unwrap();
        props.define_property("Speed", PropertyValue::Integer(3), false).unwrap();
        props.set_property_limits("Speed", 0.0, 7.0).unwrap();
        let wheel_name = match wheel { WheelId::A => "A", WheelId::B => "B", WheelId::C => "C" };
        props.define_property("Wheel", PropertyValue::String(wheel_name.into()), true).unwrap();

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
        self.call_transport(|t| {
            match wheel {
                WheelId::A => {
                    let cmd = (speed << 4) | pos;
                    t.send_bytes(&[cmd])?;
                    let resp = t.receive_bytes(2)?;
                    if resp.last() != Some(&0x0D) {
                        return Err(MmError::SerialInvalidResponse);
                    }
                }
                WheelId::B => {
                    let cmd = 0x80 | (speed << 4) | pos;
                    t.send_bytes(&[cmd])?;
                    let resp = t.receive_bytes(2)?;
                    if resp.last() != Some(&0x0D) {
                        return Err(MmError::SerialInvalidResponse);
                    }
                }
                WheelId::C => {
                    let payload = (speed << 4) | pos;
                    t.send_bytes(&[0xFC, payload])?;
                    let resp = t.receive_bytes(3)?;
                    if resp.last() != Some(&0x0D) {
                        return Err(MmError::SerialInvalidResponse);
                    }
                }
            }
            Ok(())
        })
    }
}

impl Device for LambdaWheel {
    fn name(&self) -> &str { "LambdaWheel" }
    fn description(&self) -> &str { "Sutter Lambda filter wheel" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Move to position 0 to confirm communication
        self.send_move(0)?;
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
                self.labels.get(self.position as usize).cloned().unwrap_or_default()
            )),
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
            "Speed" => {
                let s = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u8;
                if s > 7 { return Err(MmError::InvalidPropertyValue); }
                self.speed = s;
                self.props.set(name, PropertyValue::Integer(s as i64))
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

impl StateDevice for LambdaWheel {
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

    fn get_position(&self) -> MmResult<u64> { Ok(self.position as u64) }

    fn get_number_of_positions(&self) -> u64 { self.num_positions as u64 }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels.get(pos as usize).cloned().ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self.labels.iter().position(|l| l == label)
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

    fn get_gate_open(&self) -> MmResult<bool> { Ok(self.gate_open) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn make_wheel_a() -> LambdaWheel {
        // Init: move to pos 0 (speed 3 → cmd = 0x30 | 0 = 0x30), response = [0x30, 0x0D]
        let t = MockTransport::new()
            .expect_binary(&[0x30, 0x0D]);
        LambdaWheel::new(WheelId::A).with_transport(Box::new(t))
    }

    #[test]
    fn initialize_moves_to_zero() {
        let mut wheel = make_wheel_a();
        wheel.initialize().unwrap();
        assert_eq!(wheel.get_position().unwrap(), 0);
    }

    #[test]
    fn set_position_wheel_a() {
        // After init (1 response), set to position 3: cmd = (3<<4)|3 = 0x33
        let t = MockTransport::new()
            .expect_binary(&[0x30, 0x0D])   // init move to 0
            .expect_binary(&[0x33, 0x0D]);  // move to 3
        let mut wheel = LambdaWheel::new(WheelId::A).with_transport(Box::new(t));
        wheel.initialize().unwrap();
        wheel.set_position(3).unwrap();
        assert_eq!(wheel.get_position().unwrap(), 3);
    }

    #[test]
    fn set_position_wheel_b() {
        // Wheel B: cmd = 0x80 | (3<<4) | 5 = 0x80 | 0x30 | 0x05 = 0xB5
        let t = MockTransport::new()
            .expect_binary(&[0xB0, 0x0D])   // init move B to 0
            .expect_binary(&[0xB5, 0x0D]);  // move B to 5
        let mut wheel = LambdaWheel::new(WheelId::B).with_transport(Box::new(t));
        wheel.initialize().unwrap();
        wheel.set_position(5).unwrap();
        assert_eq!(wheel.get_position().unwrap(), 5);
    }

    #[test]
    fn set_position_wheel_c() {
        // Wheel C: send [0xFC, (3<<4)|0=0x30], recv [0xFC, 0x30, 0x0D]
        let t = MockTransport::new()
            .expect_binary(&[0xFC, 0x30, 0x0D])  // init
            .expect_binary(&[0xFC, 0x32, 0x0D]);  // move to 2
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
        let t = MockTransport::new()
            .expect_binary(&[0x30, 0x0D])   // init
            .expect_binary(&[0x34, 0x0D]);  // move to 4
        let mut wheel = LambdaWheel::new(WheelId::A).with_transport(Box::new(t));
        wheel.initialize().unwrap();
        wheel.set_position_label(4, "DAPI").unwrap();
        wheel.set_position_by_label("DAPI").unwrap();
        assert_eq!(wheel.get_position().unwrap(), 4);
    }
}
