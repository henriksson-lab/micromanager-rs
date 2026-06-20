/// Squid+ filter wheel (StateDevice).
///
/// The filter wheel is driven by a stepper motor on the W axis of the Squid+
/// microcontroller.  It has 8 positions (0–7 in MicroManager convention).
///
/// Movement is relative: the controller converts a position delta into
/// millimetres, then into microsteps, and sends a MOVE_W binary packet.
///
/// Default motor parameters (from squid-control config):
///   screw_pitch   = 1.0 mm/rev
///   microstepping = 64
///   fullsteps/rev = 200
///   → 12 800 microsteps per mm
///   → 1 600 microsteps per position step (1.0 / 8 mm = 0.125 mm)
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

use super::protocol;

const NUM_POSITIONS: u64 = 8;

/// Screw pitch in mm (distance per full motor revolution).
const SCREW_PITCH_MM: f64 = 1.0;

/// Microsteps per full step.
const MICROSTEPPING: f64 = 64.0;

/// Full steps per motor revolution.
const FULLSTEPS_PER_REV: f64 = 200.0;

/// Post-home offset in mm (small adjustment from home switch position).
const HOME_OFFSET_MM: f64 = 0.008;

/// Microsteps per mm = microstepping * fullsteps_per_rev / screw_pitch.
const USTEPS_PER_MM: f64 = MICROSTEPPING * FULLSTEPS_PER_REV / SCREW_PITCH_MM;

/// Distance in mm between adjacent positions.
const STEP_MM: f64 = SCREW_PITCH_MM / NUM_POSITIONS as f64;

pub struct SquidPlusFilterWheel {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position: u64,
    labels: Vec<String>,
    cmd_id: u8,
}

impl SquidPlusFilterWheel {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();

        let labels: Vec<String> = (0..NUM_POSITIONS)
            .map(|i| format!("Filter-{}", i))
            .collect();

        Self {
            props,
            transport: None,
            initialized: false,
            position: 0,
            labels,
            cmd_id: 0,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn next_cmd_id(&mut self) -> u8 {
        self.cmd_id = self.cmd_id.wrapping_add(1);
        self.cmd_id
    }

    fn send_and_wait(&mut self, pkt: &[u8]) -> MmResult<()> {
        let t = self.transport.as_mut().ok_or(MmError::NotConnected)?;
        t.send_bytes(pkt)?;
        let resp = t.receive_bytes(protocol::MSG_LENGTH)?;
        match protocol::parse_response(&resp) {
            Some((_id, status)) if status == protocol::STATUS_COMPLETED => Ok(()),
            Some((_id, status)) if status == protocol::STATUS_IN_PROGRESS => {
                // Poll until completed
                loop {
                    let resp = t.receive_bytes(protocol::MSG_LENGTH)?;
                    match protocol::parse_response(&resp) {
                        Some((_id, s)) if s == protocol::STATUS_COMPLETED => return Ok(()),
                        Some(_) => continue,
                        None => return Err(MmError::SerialInvalidResponse),
                    }
                }
            }
            Some(_) => Err(MmError::SerialCommandFailed),
            None => Err(MmError::SerialInvalidResponse),
        }
    }

    /// Convert a position delta to signed microsteps.
    fn delta_to_usteps(delta_positions: i64) -> i32 {
        let delta_mm = delta_positions as f64 * STEP_MM;
        (delta_mm * USTEPS_PER_MM).round() as i32
    }

    /// Move the wheel by `delta_positions` steps (positive = forward).
    fn move_by(&mut self, delta_positions: i64) -> MmResult<()> {
        let usteps = Self::delta_to_usteps(delta_positions);
        let id = self.next_cmd_id();
        let pkt = protocol::build_move_w(id, usteps);
        self.send_and_wait(&pkt)
    }
}

impl Default for SquidPlusFilterWheel {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for SquidPlusFilterWheel {
    fn name(&self) -> &str {
        "SquidPlusFilterWheel"
    }
    fn description(&self) -> &str {
        "Squid+ filter wheel (8-position, stepper on W axis)"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Home the filter wheel
        let id = self.next_cmd_id();
        let pkt = protocol::build_home_w(id);
        self.send_and_wait(&pkt)?;

        // Apply post-home offset
        let offset_usteps = (HOME_OFFSET_MM * USTEPS_PER_MM).round() as i32;
        let id = self.next_cmd_id();
        let pkt = protocol::build_move_w(id, offset_usteps);
        self.send_and_wait(&pkt)?;

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
                self.labels[self.position as usize].clone(),
            )),
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

impl StateDevice for SquidPlusFilterWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= NUM_POSITIONS {
            return Err(MmError::UnknownPosition);
        }
        if pos == self.position {
            return Ok(());
        }
        let delta = pos as i64 - self.position as i64;
        self.move_by(delta)?;
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

    /// Build a valid 24-byte response with STATUS_COMPLETED for the given cmd_id.
    fn ok_response(cmd_id: u8) -> Vec<u8> {
        let mut buf = vec![0u8; protocol::MSG_LENGTH];
        buf[0] = cmd_id;
        buf[1] = protocol::STATUS_COMPLETED;
        buf[protocol::MSG_LENGTH - 1] = protocol::crc8(&buf[..protocol::MSG_LENGTH - 1]);
        buf
    }

    /// Mock transport that answers the init sequence (home + offset move).
    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect_binary(&ok_response(1)) // home response
            .expect_binary(&ok_response(2)) // offset move response
    }

    #[test]
    fn initialize() {
        let mut dev = SquidPlusFilterWheel::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        assert!(dev.initialized);
        assert_eq!(dev.get_position().unwrap(), 0);

        // Transport is inside the struct; init success verifies the scripted responses matched.
    }

    #[test]
    fn set_position_forward() {
        // Move from 0 → 3: delta = 3, usteps = 3 * 1600 = 4800
        let t = make_init_transport().expect_binary(&ok_response(3));
        let mut dev = SquidPlusFilterWheel::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_position(3).unwrap();
        assert_eq!(dev.get_position().unwrap(), 3);
    }

    #[test]
    fn set_position_backward() {
        // Move 0→5, then 5→2: delta = -3, usteps = -4800
        let t = make_init_transport()
            .expect_binary(&ok_response(3)) // 0→5
            .expect_binary(&ok_response(4)); // 5→2
        let mut dev = SquidPlusFilterWheel::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_position(5).unwrap();
        dev.set_position(2).unwrap();
        assert_eq!(dev.get_position().unwrap(), 2);
    }

    #[test]
    fn set_position_same_is_noop() {
        let mut dev = SquidPlusFilterWheel::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        // No extra binary response needed — same position is a no-op
        dev.set_position(0).unwrap();
    }

    #[test]
    fn out_of_range() {
        let mut dev = SquidPlusFilterWheel::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        assert!(dev.set_position(8).is_err());
        assert!(dev.set_position(100).is_err());
    }

    #[test]
    fn labels() {
        let mut dev = SquidPlusFilterWheel::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();

        assert_eq!(dev.get_position_label(0).unwrap(), "Filter-0");
        assert_eq!(dev.get_position_label(7).unwrap(), "Filter-7");
        assert!(dev.get_position_label(8).is_err());

        dev.set_position_label(0, "DAPI").unwrap();
        assert_eq!(dev.get_position_label(0).unwrap(), "DAPI");

        // Navigate by label
        let t = make_init_transport().expect_binary(&ok_response(3));
        let mut dev2 = SquidPlusFilterWheel::new().with_transport(Box::new(t));
        dev2.initialize().unwrap();
        dev2.set_position_label(3, "GFP").unwrap();
        dev2.set_position_by_label("GFP").unwrap();
        assert_eq!(dev2.get_position().unwrap(), 3);
    }

    #[test]
    fn ustep_calculation() {
        // 1 position step = 0.125 mm × 12800 usteps/mm = 1600 usteps
        assert_eq!(SquidPlusFilterWheel::delta_to_usteps(1), 1600);
        assert_eq!(SquidPlusFilterWheel::delta_to_usteps(-1), -1600);
        assert_eq!(SquidPlusFilterWheel::delta_to_usteps(4), 6400);
    }

    #[test]
    fn no_transport_error() {
        assert!(SquidPlusFilterWheel::new().initialize().is_err());
    }
}
