/// Squid+ XY translation stage.
///
/// Driven by stepper motors on the X and Y axes of the Squid+ microcontroller.
///
/// Motor parameters (from squid-control config):
///   screw_pitch   = 2.54 mm/rev
///   microstepping = 256
///   fullsteps/rev = 200
///   → 20 157.48 microsteps per mm
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

use super::{common, protocol};

const SCREW_PITCH_XY_MM: f64 = 2.54;
const MICROSTEPPING_XY: f64 = 256.0;
const FULLSTEPS_PER_REV_XY: f64 = 200.0;
const USTEPS_PER_MM_XY: f64 = MICROSTEPPING_XY * FULLSTEPS_PER_REV_XY / SCREW_PITCH_XY_MM;
const DIRECTION_X: f64 = -1.0;
const DIRECTION_Y: f64 = -1.0;

pub struct SquidPlusXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
    cmd_id: u8,
}

impl SquidPlusXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            x_um: 0.0,
            y_um: 0.0,
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
        common::send_and_wait(t.as_mut(), pkt)
    }

    fn um_to_usteps_x(um: f64) -> i32 {
        (um / 1000.0 * USTEPS_PER_MM_XY / DIRECTION_X) as i32
    }

    fn um_to_usteps_y(um: f64) -> i32 {
        (um / 1000.0 * USTEPS_PER_MM_XY / DIRECTION_Y) as i32
    }
}

impl Default for SquidPlusXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for SquidPlusXYStage {
    fn name(&self) -> &str {
        "SquidPlusXYStage"
    }
    fn description(&self) -> &str {
        "Squid+ XY translation stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.x_um = 0.0;
        self.y_um = 0.0;
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
        self.props.set(name, val)
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
        DeviceType::XYStage
    }

    fn busy(&self) -> bool {
        false
    }
}

impl XYStage for SquidPlusXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let ux = Self::um_to_usteps_x(x);
        let uy = Self::um_to_usteps_y(y);
        let id = self.next_cmd_id();
        let pkt = protocol::build_move(id, protocol::CMD_MOVETO_X, ux);
        self.send_and_wait(&pkt)?;
        let id = self.next_cmd_id();
        let pkt = protocol::build_move(id, protocol::CMD_MOVETO_Y, uy);
        self.send_and_wait(&pkt)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        Ok((self.x_um, self.y_um))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let ux = Self::um_to_usteps_x(dx);
        let uy = Self::um_to_usteps_y(dy);
        let id = self.next_cmd_id();
        let pkt = protocol::build_move(id, protocol::CMD_MOVE_X, ux);
        self.send_and_wait(&pkt)?;
        let id = self.next_cmd_id();
        let pkt = protocol::build_move(id, protocol::CMD_MOVE_Y, uy);
        self.send_and_wait(&pkt)?;
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        let id = self.next_cmd_id();
        let pkt = protocol::build_home_xy(id, protocol::HOME_POSITIVE, protocol::HOME_POSITIVE);
        self.send_and_wait(&pkt)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Err(MmError::UnsupportedCommand)
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        let step = 1000.0 / USTEPS_PER_MM_XY;
        (DIRECTION_X * step, DIRECTION_Y * step)
    }

    fn set_origin(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn ok_response(cmd_id: u8) -> Vec<u8> {
        let mut buf = vec![0u8; protocol::MSG_LENGTH];
        buf[0] = cmd_id;
        buf[1] = protocol::STATUS_COMPLETED;
        buf[protocol::MSG_LENGTH - 1] = protocol::crc8(&buf[..protocol::MSG_LENGTH - 1]);
        buf
    }

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
    }

    #[test]
    fn initialize() {
        let mut dev = SquidPlusXYStage::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        assert!(dev.initialized);
        assert_eq!(dev.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn set_xy_position() {
        let t = make_init_transport()
            .expect_binary(&ok_response(1)) // move X
            .expect_binary(&ok_response(2)); // move Y
        let mut dev = SquidPlusXYStage::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_xy_position_um(100.0, 200.0).unwrap();
        let (x, y) = dev.get_xy_position_um().unwrap();
        assert!((x - 100.0).abs() < 0.01);
        assert!((y - 200.0).abs() < 0.01);
    }

    #[test]
    fn relative_move() {
        let t = make_init_transport()
            .expect_binary(&ok_response(1))
            .expect_binary(&ok_response(2));
        let mut dev = SquidPlusXYStage::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_relative_xy_position_um(50.0, -30.0).unwrap();
        let (x, y) = dev.get_xy_position_um().unwrap();
        assert!((x - 50.0).abs() < 0.01);
        assert!((y + 30.0).abs() < 0.01);
    }

    #[test]
    fn home() {
        let t = make_init_transport().expect_binary(&ok_response(1)); // home XY
        let mut dev = SquidPlusXYStage::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.x_um = 500.0;
        dev.y_um = 300.0;
        dev.home().unwrap();
        assert_eq!(dev.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn set_origin() {
        let t = make_init_transport();
        let mut dev = SquidPlusXYStage::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.x_um = 100.0;
        assert_eq!(dev.set_origin().unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn step_size() {
        let dev = SquidPlusXYStage::new();
        let (sx, sy) = dev.get_step_size_um();
        // C++ default DirectionX/Y is Negative, making the exposed step size signed.
        assert!((sx + 0.0496).abs() < 0.001);
        assert_eq!(sx, sy);
    }

    #[test]
    fn ustep_conversion() {
        // 1 mm = 1000 µm → 20157 usteps
        let usteps = SquidPlusXYStage::um_to_usteps_x(1000.0);
        assert_eq!(usteps, -20157);
    }

    #[test]
    fn no_transport_error() {
        assert!(SquidPlusXYStage::new().initialize().is_err());
    }
}
