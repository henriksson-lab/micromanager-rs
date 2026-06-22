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
const DEFAULT_MAX_VELOCITY_MM_S: f64 = 25.0;
const DEFAULT_ACCELERATION_MM_S2: f64 = 500.0;

pub struct SquidPlusXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
    cmd_id: u8,
    max_velocity_mm_s: f64,
    acceleration_mm_s2: f64,
    full_steps_per_rev_x: f64,
    full_steps_per_rev_y: f64,
    screw_pitch_x_mm: f64,
    screw_pitch_y_mm: f64,
    microstepping_x: f64,
    microstepping_y: f64,
    direction_x: f64,
    direction_y: f64,
}

impl SquidPlusXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        define_stage_config_props(&mut props);
        props
            .define_property(
                "Acceleration",
                PropertyValue::Float(DEFAULT_ACCELERATION_MM_S2),
                false,
            )
            .unwrap();
        props
            .define_property(
                "MaxVelocity",
                PropertyValue::Float(DEFAULT_MAX_VELOCITY_MM_S),
                false,
            )
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            x_um: 0.0,
            y_um: 0.0,
            cmd_id: 0,
            max_velocity_mm_s: DEFAULT_MAX_VELOCITY_MM_S,
            acceleration_mm_s2: DEFAULT_ACCELERATION_MM_S2,
            full_steps_per_rev_x: FULLSTEPS_PER_REV_XY,
            full_steps_per_rev_y: FULLSTEPS_PER_REV_XY,
            screw_pitch_x_mm: SCREW_PITCH_XY_MM,
            screw_pitch_y_mm: SCREW_PITCH_XY_MM,
            microstepping_x: MICROSTEPPING_XY,
            microstepping_y: MICROSTEPPING_XY,
            direction_x: DIRECTION_X,
            direction_y: DIRECTION_Y,
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

    fn send_axis_velocity_values(
        &mut self,
        axis: u8,
        max_velocity_mm_s: f64,
        acceleration_mm_s2: f64,
    ) -> MmResult<()> {
        let id = self.next_cmd_id();
        let pkt = protocol::build_set_max_velocity_acceleration(
            id,
            axis,
            max_velocity_mm_s,
            acceleration_mm_s2,
        );
        self.send_and_wait(&pkt)
    }

    fn configure_velocity_values(
        &mut self,
        max_velocity_mm_s: f64,
        acceleration_mm_s2: f64,
    ) -> MmResult<()> {
        self.send_axis_velocity_values(protocol::AXIS_X, max_velocity_mm_s, acceleration_mm_s2)?;
        self.send_axis_velocity_values(protocol::AXIS_Y, max_velocity_mm_s, acceleration_mm_s2)
    }

    fn configure_velocity(&mut self) -> MmResult<()> {
        self.configure_velocity_values(self.max_velocity_mm_s, self.acceleration_mm_s2)
    }

    fn um_to_usteps_x(um: f64) -> i32 {
        (um / 1000.0 * USTEPS_PER_MM_XY / DIRECTION_X).round_ties_even() as i32
    }

    fn um_to_usteps_y(um: f64) -> i32 {
        (um / 1000.0 * USTEPS_PER_MM_XY / DIRECTION_Y).round_ties_even() as i32
    }

    fn read_configuration(&mut self) -> MmResult<()> {
        self.full_steps_per_rev_x = self
            .props
            .get("FullStepsPerRevX")?
            .as_f64()
            .unwrap_or(FULLSTEPS_PER_REV_XY);
        self.full_steps_per_rev_y = self
            .props
            .get("FullStepsPerRevY")?
            .as_f64()
            .unwrap_or(FULLSTEPS_PER_REV_XY);
        self.screw_pitch_x_mm = self
            .props
            .get("ScrewPitchXmm")?
            .as_f64()
            .unwrap_or(SCREW_PITCH_XY_MM);
        self.screw_pitch_y_mm = self
            .props
            .get("ScrewPitchYmm")?
            .as_f64()
            .unwrap_or(SCREW_PITCH_XY_MM);
        self.microstepping_x = self
            .props
            .get("MicroSteppingDefaultX")?
            .as_i64()
            .unwrap_or(MICROSTEPPING_XY as i64) as f64;
        self.microstepping_y = self
            .props
            .get("MicroSteppingDefaultY")?
            .as_i64()
            .unwrap_or(MICROSTEPPING_XY as i64) as f64;
        self.direction_x = if self.props.get("DirectionX")?.as_str() == "Positive" {
            1.0
        } else {
            -1.0
        };
        self.direction_y = if self.props.get("DirectionY")?.as_str() == "Positive" {
            1.0
        } else {
            -1.0
        };
        Ok(())
    }

    fn usteps_per_mm_x(&self) -> f64 {
        self.microstepping_x * self.full_steps_per_rev_x / self.screw_pitch_x_mm
    }

    fn usteps_per_mm_y(&self) -> f64 {
        self.microstepping_y * self.full_steps_per_rev_y / self.screw_pitch_y_mm
    }

    fn pos_to_usteps_x(&self, um: f64) -> i32 {
        (um / 1000.0 * self.usteps_per_mm_x() / self.direction_x).round_ties_even() as i32
    }

    fn pos_to_usteps_y(&self, um: f64) -> i32 {
        (um / 1000.0 * self.usteps_per_mm_y() / self.direction_y).round_ties_even() as i32
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
        self.read_configuration()?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        self.max_velocity_mm_s = self
            .props
            .get("MaxVelocity")?
            .as_f64()
            .unwrap_or(DEFAULT_MAX_VELOCITY_MM_S);
        self.acceleration_mm_s2 = self
            .props
            .get("Acceleration")?
            .as_f64()
            .unwrap_or(DEFAULT_ACCELERATION_MM_S2);
        self.configure_velocity()?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "MaxVelocity" => Ok(PropertyValue::Float(self.max_velocity_mm_s)),
            "Acceleration" => Ok(PropertyValue::Float(self.acceleration_mm_s2)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "MaxVelocity" => {
                let value = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(1.0..=655.35).contains(&value) {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized {
                    self.configure_velocity_values(value, self.acceleration_mm_s2)?;
                }
                self.max_velocity_mm_s = value;
                self.props.set(name, PropertyValue::Float(value))?;
                Ok(())
            }
            "Acceleration" => {
                let value = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(1.0..=6553.5).contains(&value) {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized {
                    self.configure_velocity_values(self.max_velocity_mm_s, value)?;
                }
                self.acceleration_mm_s2 = value;
                self.props.set(name, PropertyValue::Float(value))?;
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
        DeviceType::XYStage
    }

    fn busy(&self) -> bool {
        false
    }
}

fn define_stage_config_props(props: &mut PropertyMap) {
    props
        .define_property(
            "FullStepsPerRevX",
            PropertyValue::Float(FULLSTEPS_PER_REV_XY),
            false,
        )
        .unwrap();
    props
        .define_property(
            "FullStepsPerRevY",
            PropertyValue::Float(FULLSTEPS_PER_REV_XY),
            false,
        )
        .unwrap();
    props
        .define_property(
            "ScrewPitchXmm",
            PropertyValue::Float(SCREW_PITCH_XY_MM),
            false,
        )
        .unwrap();
    props
        .define_property(
            "ScrewPitchYmm",
            PropertyValue::Float(SCREW_PITCH_XY_MM),
            false,
        )
        .unwrap();
    props
        .define_property(
            "MicroSteppingDefaultX",
            PropertyValue::Integer(MICROSTEPPING_XY as i64),
            false,
        )
        .unwrap();
    props
        .define_property(
            "MicroSteppingDefaultY",
            PropertyValue::Integer(MICROSTEPPING_XY as i64),
            false,
        )
        .unwrap();
    props
        .define_property(
            "DirectionX",
            PropertyValue::String("Negative".into()),
            false,
        )
        .unwrap();
    props
        .set_allowed_values("DirectionX", &["Positive", "Negative"])
        .unwrap();
    props
        .define_property(
            "DirectionY",
            PropertyValue::String("Negative".into()),
            false,
        )
        .unwrap();
    props
        .set_allowed_values("DirectionY", &["Positive", "Negative"])
        .unwrap();
}

impl XYStage for SquidPlusXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let ux = self.pos_to_usteps_x(x);
        let uy = self.pos_to_usteps_y(y);
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
        let ux = self.pos_to_usteps_x(dx);
        let uy = self.pos_to_usteps_y(dy);
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
        (
            self.direction_x * 1000.0 / self.usteps_per_mm_x(),
            self.direction_y * 1000.0 / self.usteps_per_mm_y(),
        )
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
            .expect_binary(&ok_response(1))
            .expect_binary(&ok_response(2))
    }

    #[test]
    fn initialize() {
        let mut dev = SquidPlusXYStage::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        assert!(dev.initialized);
        assert_eq!(dev.get_xy_position_um().unwrap(), (0.0, 0.0));
        assert_eq!(
            dev.get_property("MaxVelocity").unwrap(),
            PropertyValue::Float(25.0)
        );
        assert_eq!(
            dev.get_property("Acceleration").unwrap(),
            PropertyValue::Float(500.0)
        );
    }

    #[test]
    fn set_xy_position() {
        let t = make_init_transport()
            .expect_binary(&ok_response(3)) // move X
            .expect_binary(&ok_response(4)); // move Y
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
            .expect_binary(&ok_response(3))
            .expect_binary(&ok_response(4));
        let mut dev = SquidPlusXYStage::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_relative_xy_position_um(50.0, -30.0).unwrap();
        let (x, y) = dev.get_xy_position_um().unwrap();
        assert!((x - 50.0).abs() < 0.01);
        assert!((y + 30.0).abs() < 0.01);
    }

    #[test]
    fn home() {
        let t = make_init_transport().expect_binary(&ok_response(3)); // home XY
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
        assert!((sx + 0.0496).abs() < 0.001);
        assert_eq!(sx, sy);
    }

    #[test]
    fn ustep_conversion() {
        // 1 mm = 1000 um; upstream Cephla defaults DirectionX/Y to Negative.
        assert_eq!(SquidPlusXYStage::um_to_usteps_x(1000.0), -20157);
        assert_eq!(SquidPlusXYStage::um_to_usteps_y(1000.0), -20157);
        assert_eq!(SquidPlusXYStage::um_to_usteps_x(0.1), -2);
        assert_eq!(SquidPlusXYStage::um_to_usteps_y(-0.1), 2);
    }

    #[test]
    fn no_transport_error() {
        assert!(SquidPlusXYStage::new().initialize().is_err());
    }

    #[test]
    fn velocity_property_sends_both_axes_after_init() {
        let t = make_init_transport()
            .expect_binary(&ok_response(3))
            .expect_binary(&ok_response(4));
        let mut dev = SquidPlusXYStage::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("MaxVelocity", PropertyValue::Float(10.0))
            .unwrap();
        assert_eq!(
            dev.get_property("MaxVelocity").unwrap(),
            PropertyValue::Float(10.0)
        );
    }

    #[test]
    fn velocity_property_cache_updates_only_after_both_axis_acks() {
        let mut failed = ok_response(4);
        failed[1] = protocol::STATUS_CHECKSUM_ERROR;
        failed[protocol::MSG_LENGTH - 1] = protocol::crc8(&failed[..protocol::MSG_LENGTH - 1]);

        let t = make_init_transport()
            .expect_binary(&ok_response(3))
            .expect_binary(&failed);
        let mut dev = SquidPlusXYStage::new().with_transport(Box::new(t));
        dev.initialize().unwrap();

        assert_eq!(
            dev.set_property("MaxVelocity", PropertyValue::Float(10.0)),
            Err(MmError::SerialCommandFailed)
        );
        assert_eq!(
            dev.get_property("MaxVelocity").unwrap(),
            PropertyValue::Float(DEFAULT_MAX_VELOCITY_MM_S)
        );
    }

    #[test]
    fn exposes_upstream_stage_configuration_properties() {
        let dev = SquidPlusXYStage::new();
        assert!(dev.has_property("FullStepsPerRevX"));
        assert!(dev.has_property("ScrewPitchYmm"));
        assert!(dev.has_property("MicroSteppingDefaultX"));
        assert!(dev.has_property("DirectionY"));
        assert_eq!(
            dev.get_property("DirectionX").unwrap(),
            PropertyValue::String("Negative".into())
        );
        assert_eq!(
            dev.get_property("DirectionY").unwrap(),
            PropertyValue::String("Negative".into())
        );
        assert!(dev.has_property("Acceleration"));
        assert!(dev.has_property("MaxVelocity"));
    }
}
