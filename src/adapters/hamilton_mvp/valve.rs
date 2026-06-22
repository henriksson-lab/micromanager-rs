/// Hamilton MVP (Modular Valve Positioner).
///
/// Protocol (TX `\r`, RX echo + 0x06 ACK):
///   Address char (default 'a') prepended to every command.
///   Device echoes the command back, then sends 0x06 (ACK) on success,
///   0x15 (NAK) on failure. Data queries append data after the ACK.
///
///   `<a>LXR\r`         → echo + ACK        initialize/reset
///   `<a>LQT\r`         → echo + ACK + '2'–'7'  valve type digit
///   `<a>LQP\r`         → echo + ACK + N    current position (1-based ASCII digit)
///   `<a>LP0<N>R\r`     → echo + ACK        set position (0=CW, N=1-based)
///   `<a>F\r`           → echo + ACK + 'Y'/'N'  movement finished?
///
/// Valve type → number of positions:
///   '2'=8, '3'=6, '4'=3, '5'=2, '6'=2, '7'=4
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};

const ACK: char = '\x06';
const ROTATION_CLOCKWISE: &str = "Clockwise";
const ROTATION_COUNTERCLOCKWISE: &str = "Counterclockwise";
const ROTATION_LEAST_ANGLE: &str = "Least rotation angle";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RotationDirection {
    Clockwise,
    Counterclockwise,
    LeastAngle,
}

impl RotationDirection {
    fn as_str(self) -> &'static str {
        match self {
            Self::Clockwise => ROTATION_CLOCKWISE,
            Self::Counterclockwise => ROTATION_COUNTERCLOCKWISE,
            Self::LeastAngle => ROTATION_LEAST_ANGLE,
        }
    }

    fn from_property(value: &PropertyValue) -> MmResult<Self> {
        match value {
            PropertyValue::String(s) if s == ROTATION_CLOCKWISE => Ok(Self::Clockwise),
            PropertyValue::String(s) if s == ROTATION_COUNTERCLOCKWISE => {
                Ok(Self::Counterclockwise)
            }
            PropertyValue::String(s) if s == ROTATION_LEAST_ANGLE => Ok(Self::LeastAngle),
            _ => Err(MmError::InvalidPropertyValue),
        }
    }
}

pub struct HamiltonMvpValve {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    address: char,
    device_name: String,
    num_positions: u64,
    position: Cell<u64>,
    labels: Vec<String>,
    valve_type: u64,
    rotation_direction: RotationDirection,
}

impl HamiltonMvpValve {
    pub fn new(address: char) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Address", PropertyValue::String(address.to_string()), false)
            .unwrap();
        props
            .define_property("ValveType", PropertyValue::String(String::new()), true)
            .unwrap();
        let num = 6u64;
        let labels: Vec<String> = (1..=num).map(|i| format!("Position-{}", i)).collect();
        Self {
            props,
            transport: None,
            initialized: false,
            address,
            device_name: format!("HamiltonMVP-{}", address),
            num_positions: num,
            position: Cell::new(0),
            labels,
            valve_type: 0,
            rotation_direction: RotationDirection::LeastAngle,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(RefCell::new(t));
        self
    }

    fn cmd_ack(&self, command: &str) -> MmResult<String> {
        let full = format!("{}{}\r", self.address, command);
        let echo = format!("{}{}", self.address, command);
        let Some(transport) = self.transport.as_ref() else {
            return Err(MmError::NotConnected);
        };
        let mut transport = transport.borrow_mut();
        transport.send(&full)?;
        let echoed = transport.receive_line()?;
        if echoed != echo {
            return Err(MmError::SerialInvalidResponse);
        }
        let ack_line = transport.receive_line()?;
        let mut chars = ack_line.chars();
        match chars.next() {
            Some(ACK) => Ok(chars.collect::<String>().trim().to_string()),
            Some('\x15') => Err(MmError::SerialInvalidResponse),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn parse_decimal(response: &str, max_digits: usize) -> MmResult<u64> {
        let trimmed = response.trim();
        if trimmed.is_empty()
            || trimmed.len() > max_digits
            || !trimmed.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(MmError::SerialInvalidResponse);
        }
        trimmed
            .parse::<u64>()
            .map_err(|_| MmError::SerialInvalidResponse)
    }

    fn valve_type_to_positions(c: u64) -> MmResult<u64> {
        match c {
            2 => Ok(8),
            3 => Ok(6),
            4 => Ok(3),
            5 | 6 => Ok(2),
            7 => Ok(4),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn valve_type_name(c: u64) -> MmResult<&'static str> {
        match c {
            2 => Ok("8 ports"),
            3 => Ok("6 ports"),
            4 => Ok("3 ports"),
            5 => Ok("2 ports 180 degrees apart"),
            6 => Ok("2 ports 90 degrees apart"),
            7 => Ok("4 ports"),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn valve_speed_hz(c: u64) -> MmResult<i64> {
        match c {
            0..=9 => Ok((30 + c * 10) as i64),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn parse_instrument_error(response: &str) -> MmResult<bool> {
        let bytes = response.as_bytes();
        if bytes.len() != 4 || bytes[0] != 0x50 || bytes[2] != 0x50 || bytes[3] != 0x50 {
            return Err(MmError::SerialInvalidResponse);
        }
        let b1 = bytes[1];
        if b1 & (1 << 3) != 0 || b1 & (1 << 4) != 0 || b1 & (1 << 5) != 0 || b1 & (1 << 6) == 0 {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(b1 & 1 != 0)
    }

    fn equal_angle(num_positions: u64, ccw: bool, start_pos: u64, dest_pos: u64) -> i64 {
        let increment = 360 / num_positions as i64;
        let start_angle = increment * start_pos as i64;
        let dest_angle = increment * dest_pos as i64;
        let mut delta_angle = dest_angle - start_angle;
        if ccw {
            delta_angle = -delta_angle;
        }
        if delta_angle >= 0 {
            delta_angle
        } else {
            360 + delta_angle
        }
    }

    fn valve_rotation_angle(&self, ccw: bool, start_pos: u64, dest_pos: u64) -> i64 {
        match self.valve_type {
            2 => Self::equal_angle(8, ccw, start_pos, dest_pos),
            3 => Self::equal_angle(6, ccw, start_pos, dest_pos),
            4 => {
                let angle = [[0, 270, 180], [90, 0, 270], [180, 90, 0]];
                if ccw {
                    angle[start_pos as usize][dest_pos as usize]
                } else {
                    angle[dest_pos as usize][start_pos as usize]
                }
            }
            5 => Self::equal_angle(2, ccw, start_pos, dest_pos),
            6 => {
                let angle = [[0, 270], [90, 0]];
                if ccw {
                    angle[start_pos as usize][dest_pos as usize]
                } else {
                    angle[dest_pos as usize][start_pos as usize]
                }
            }
            7 => Self::equal_angle(4, ccw, start_pos, dest_pos),
            _ => 0,
        }
    }

    fn should_rotate_ccw(&self, cur_pos: u64, new_pos: u64) -> bool {
        match self.rotation_direction {
            RotationDirection::Clockwise => false,
            RotationDirection::Counterclockwise => true,
            RotationDirection::LeastAngle => {
                let cw_angle = self.valve_rotation_angle(false, cur_pos, new_pos);
                let ccw_angle = self.valve_rotation_angle(true, cur_pos, new_pos);
                ccw_angle < cw_angle
            }
        }
    }

    fn query_position(&self) -> MmResult<u64> {
        let pos_data = self.cmd_ack("LQP")?;
        let pos1 = Self::parse_decimal(&pos_data, 2)?;
        if pos1 == 0 || pos1 > self.num_positions {
            return Err(MmError::SerialInvalidResponse);
        }
        let pos = pos1 - 1;
        self.position.set(pos);
        Ok(pos)
    }

    fn poll_busy(&self) -> MmResult<bool> {
        let response = self.cmd_ack("F")?;
        match response.as_str() {
            "Y" => Ok(false),
            "N" | "*" => Ok(true),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }
}

impl Default for HamiltonMvpValve {
    fn default() -> Self {
        Self::new('a')
    }
}

impl Device for HamiltonMvpValve {
    fn name(&self) -> &str {
        &self.device_name
    }
    fn description(&self) -> &str {
        "Hamilton Modular Valve Positioner"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let firmware = self.cmd_ack("U")?;
        if firmware.is_empty() {
            return Err(MmError::SerialInvalidResponse);
        }
        if !self.props.has_property("FirmwareVersion") {
            self.props
                .define_property("FirmwareVersion", PropertyValue::String(firmware), true)?;
        } else if let Some(entry) = self.props.entry_mut("FirmwareVersion") {
            entry.value = PropertyValue::String(firmware);
        }
        let valve_not_initialized = Self::parse_instrument_error(&self.cmd_ack("E2")?)?;
        if valve_not_initialized {
            self.cmd_ack("LXR")?;
            for _ in 0..75 {
                if !self.poll_busy()? {
                    break;
                }
            }
        }
        // Query valve type to determine number of positions
        let type_data = self.cmd_ack("LQT")?;
        let type_num = Self::parse_decimal(&type_data, 1)?;
        self.num_positions = Self::valve_type_to_positions(type_num)?;
        self.valve_type = type_num;
        let valve_type_name = Self::valve_type_name(type_num)?;
        self.props
            .entry_mut("ValveType")
            .map(|e| e.value = PropertyValue::String(valve_type_name.into()));
        self.labels = (1..=self.num_positions)
            .map(|i| format!("Position-{}", i - 1))
            .collect();
        // Query current position
        let pos_data = self.cmd_ack("LQP")?;
        let pos1 = Self::parse_decimal(&pos_data, 2)?;
        if pos1 == 0 || pos1 > self.num_positions {
            return Err(MmError::SerialInvalidResponse);
        }
        self.position.set(pos1 - 1); // 1-based -> 0-based
        let speed_data = self.cmd_ack("LQF")?;
        let speed_num = Self::parse_decimal(&speed_data, 1)?;
        let speed_hz = Self::valve_speed_hz(speed_num)?;
        if !self.props.has_property("ValveSpeedHz") {
            self.props
                .define_property("ValveSpeedHz", PropertyValue::Integer(speed_hz), true)?;
        } else if let Some(entry) = self.props.entry_mut("ValveSpeedHz") {
            entry.value = PropertyValue::Integer(speed_hz);
        }
        if !self.props.has_property("RotationDirection") {
            self.props.define_property(
                "RotationDirection",
                PropertyValue::String(self.rotation_direction.as_str().into()),
                false,
            )?;
            self.props.set_allowed_values(
                "RotationDirection",
                &[
                    ROTATION_CLOCKWISE,
                    ROTATION_COUNTERCLOCKWISE,
                    ROTATION_LEAST_ANGLE,
                ],
            )?;
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "RotationDirection" {
            let rotation_direction = RotationDirection::from_property(&val)?;
            self.props.set(name, val)?;
            self.rotation_direction = rotation_direction;
            Ok(())
        } else {
            self.props.set(name, val)
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
        self.poll_busy().unwrap_or(false)
    }
}

impl StateDevice for HamiltonMvpValve {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!(
                "Position {} out of range",
                pos
            )));
        }
        let cur_pos = if self.initialized {
            self.query_position()?
        } else {
            self.position.get()
        };
        // Device uses 1-based positions; 0=CW rotation, 1=CCW rotation.
        let cmd = format!(
            "LP{}{}R",
            if self.should_rotate_ccw(cur_pos, pos) {
                1
            } else {
                0
            },
            pos + 1
        );
        self.cmd_ack(&cmd)?;
        self.position.set(pos);
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        if self.initialized {
            self.query_position()
        } else {
            Ok(self.position.get())
        }
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
        Ok(())
    }

    fn set_gate_open(&mut self, _open: bool) -> MmResult<()> {
        Ok(())
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        // U -> firmware; E2 -> initialized status; LQT -> ACK + '3' (6-pos valve); LQP -> ACK + '1';
        // LQF -> ACK + '2' (50 Hz).
        MockTransport::new()
            .expect("aU\r", "aU")
            .expect("aU\r", "\x061.2.3")
            .expect("aE2\r", "aE2")
            .expect("aE2\r", "\x06P@PP")
            .expect("aLQT\r", "aLQT")
            .expect("aLQT\r", "\x063") // type '3' = 6 positions
            .expect("aLQP\r", "aLQP")
            .expect("aLQP\r", "\x061") // position 1
            .expect("aLQF\r", "aLQF")
            .expect("aLQF\r", "\x062")
    }

    #[test]
    fn initialize() {
        let t = make_init_transport()
            .expect("aLQP\r", "aLQP")
            .expect("aLQP\r", "\x061");
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        v.initialize().unwrap();
        assert_eq!(v.get_number_of_positions(), 6);
        assert_eq!(v.get_position().unwrap(), 0); // pos 1 → 0-indexed 0
        assert_eq!(
            v.get_property("ValveType").unwrap(),
            PropertyValue::String("6 ports".into())
        );
        assert_eq!(
            v.get_property("ValveSpeedHz").unwrap(),
            PropertyValue::Integer(50)
        );
        assert_eq!(
            v.get_property("FirmwareVersion").unwrap(),
            PropertyValue::String("1.2.3".into())
        );
        assert_eq!(
            v.get_property("RotationDirection").unwrap(),
            PropertyValue::String("Least rotation angle".into())
        );
        assert_eq!(v.get_position_label(0).unwrap(), "Position-0");
    }

    #[test]
    fn set_position() {
        let t = make_init_transport()
            .expect("aLQP\r", "aLQP")
            .expect("aLQP\r", "\x061")
            .expect("aLP02R\r", "aLP02R")
            .expect("aLP02R\r", "\x06")
            .expect("aLQP\r", "aLQP")
            .expect("aLQP\r", "\x062"); // set position 2 (1-based)
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        v.initialize().unwrap();
        v.set_position(1).unwrap(); // 0-based → device sends 2
        assert_eq!(v.get_position().unwrap(), 1);
    }

    #[test]
    fn nak_fails() {
        let t = make_init_transport()
            .expect("aLQP\r", "aLQP")
            .expect("aLQP\r", "\x061")
            .expect("aLP02R\r", "aLP02R")
            .expect("aLP02R\r", "\x15"); // NAK
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        v.initialize().unwrap();
        assert!(v.set_position(1).is_err());
    }

    #[test]
    fn label_roundtrip() {
        let t = make_init_transport()
            .expect("aLQP\r", "aLQP")
            .expect("aLQP\r", "\x061")
            .expect("aLP02R\r", "aLP02R")
            .expect("aLP02R\r", "\x06")
            .expect("aLQP\r", "aLQP")
            .expect("aLQP\r", "\x062");
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        v.initialize().unwrap();
        v.set_position_label(1, "Buffer").unwrap();
        assert_eq!(v.get_position_label(1).unwrap(), "Buffer");
        v.set_position_by_label("Buffer").unwrap();
        assert_eq!(v.get_position().unwrap(), 1);
    }

    #[test]
    fn valve_type_8_port() {
        let t = MockTransport::new()
            .expect("aU\r", "aU")
            .expect("aU\r", "\x061.2.3")
            .expect("aE2\r", "aE2")
            .expect("aE2\r", "\x06P@PP")
            .expect("aLQT\r", "aLQT")
            .expect("aLQT\r", "\x062") // type '2' = 8 positions
            .expect("aLQP\r", "aLQP")
            .expect("aLQP\r", "\x061")
            .expect("aLQF\r", "aLQF")
            .expect("aLQF\r", "\x062");
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        v.initialize().unwrap();
        assert_eq!(v.get_number_of_positions(), 8);
    }

    #[test]
    fn mismatched_echo_fails() {
        let t = MockTransport::new()
            .expect("aU\r", "bU")
            .expect("aU\r", "\x061.2.3");
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        assert_eq!(v.initialize(), Err(MmError::SerialInvalidResponse));
    }

    #[test]
    fn invalid_valve_type_fails_initialize() {
        let t = MockTransport::new()
            .expect("aU\r", "aU")
            .expect("aU\r", "\x061.2.3")
            .expect("aE2\r", "aE2")
            .expect("aE2\r", "\x06P@PP")
            .expect("aLQT\r", "aLQT")
            .expect("aLQT\r", "\x069");
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        assert_eq!(v.initialize(), Err(MmError::SerialInvalidResponse));
    }

    #[test]
    fn invalid_position_response_fails_initialize() {
        let t = MockTransport::new()
            .expect("aU\r", "aU")
            .expect("aU\r", "\x061.2.3")
            .expect("aE2\r", "aE2")
            .expect("aE2\r", "\x06P@PP")
            .expect("aLQT\r", "aLQT")
            .expect("aLQT\r", "\x063")
            .expect("aLQP\r", "aLQP")
            .expect("aLQP\r", "\x060");
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        assert_eq!(v.initialize(), Err(MmError::SerialInvalidResponse));
    }

    #[test]
    fn initialize_runs_lxr_only_when_e2_reports_uninitialized() {
        let t = MockTransport::new()
            .expect("aU\r", "aU")
            .expect("aU\r", "\x061.2.3")
            .expect("aE2\r", "aE2")
            .expect("aE2\r", "\x06PAPP")
            .expect("aLXR\r", "aLXR")
            .expect("aLXR\r", "\x06")
            .expect("aF\r", "aF")
            .expect("aF\r", "\x06Y")
            .expect("aLQT\r", "aLQT")
            .expect("aLQT\r", "\x063")
            .expect("aLQP\r", "aLQP")
            .expect("aLQP\r", "\x061")
            .expect("aLQF\r", "aLQF")
            .expect("aLQF\r", "\x062");
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        v.initialize().unwrap();
    }

    #[test]
    fn get_position_performs_live_lqp_read() {
        let t = make_init_transport()
            .expect("aLQP\r", "aLQP")
            .expect("aLQP\r", "\x063");
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        v.initialize().unwrap();
        assert_eq!(v.get_position().unwrap(), 2);
    }

    #[test]
    fn busy_polls_movement_finished_request() {
        let t = make_init_transport()
            .expect("aF\r", "aF")
            .expect("aF\r", "\x06*")
            .expect("aF\r", "aF")
            .expect("aF\r", "\x06Y");
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        v.initialize().unwrap();
        assert!(v.busy());
        assert!(!v.busy());
    }

    #[test]
    fn counterclockwise_rotation_direction_uses_lp1() {
        let t = make_init_transport()
            .expect("aLQP\r", "aLQP")
            .expect("aLQP\r", "\x061")
            .expect("aLP12R\r", "aLP12R")
            .expect("aLP12R\r", "\x06");
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        v.initialize().unwrap();
        v.set_property(
            "RotationDirection",
            PropertyValue::String("Counterclockwise".into()),
        )
        .unwrap();
        v.set_position(1).unwrap();
    }

    #[test]
    fn least_angle_can_choose_counterclockwise() {
        let t = make_init_transport()
            .expect("aLQP\r", "aLQP")
            .expect("aLQP\r", "\x061")
            .expect("aLP16R\r", "aLP16R")
            .expect("aLP16R\r", "\x06");
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        v.initialize().unwrap();
        v.set_position(5).unwrap();
    }

    #[test]
    fn no_transport_error() {
        assert!(HamiltonMvpValve::new('a').initialize().is_err());
    }
}
