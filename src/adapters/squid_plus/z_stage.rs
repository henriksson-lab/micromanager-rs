/// Squid+ Z focus stage.
///
/// Driven by a stepper motor on the Z axis of the Squid+ microcontroller.
///
/// Motor parameters (from squid-control config):
///   screw_pitch   = 0.3 mm/rev
///   microstepping = 256
///   fullsteps/rev = 200
///   → 170 666.667 microsteps per mm
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};

use super::{common, protocol};

const SCREW_PITCH_Z_MM: f64 = 0.3;
const MICROSTEPPING_Z: f64 = 256.0;
const FULLSTEPS_PER_REV_Z: f64 = 200.0;
const USTEPS_PER_MM_Z: f64 = MICROSTEPPING_Z * FULLSTEPS_PER_REV_Z / SCREW_PITCH_Z_MM;
const DIRECTION_Z: f64 = -1.0;
const DEFAULT_MAX_VELOCITY_MM_S: f64 = 5.0;
const DEFAULT_ACCELERATION_MM_S2: f64 = 100.0;

pub struct SquidPlusZStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    z_pos_um: f64,
    cmd_id: u8,
    max_velocity_mm_s: f64,
    acceleration_mm_s2: f64,
    full_steps_per_rev_z: f64,
    screw_pitch_z_mm: f64,
    microstepping_z: f64,
    direction_z: f64,
}

impl SquidPlusZStage {
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
            z_pos_um: 0.0,
            cmd_id: 0,
            max_velocity_mm_s: DEFAULT_MAX_VELOCITY_MM_S,
            acceleration_mm_s2: DEFAULT_ACCELERATION_MM_S2,
            full_steps_per_rev_z: FULLSTEPS_PER_REV_Z,
            screw_pitch_z_mm: SCREW_PITCH_Z_MM,
            microstepping_z: MICROSTEPPING_Z,
            direction_z: DIRECTION_Z,
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

    fn configure_velocity(&mut self) -> MmResult<()> {
        let id = self.next_cmd_id();
        let pkt = protocol::build_set_max_velocity_acceleration(
            id,
            protocol::AXIS_Z,
            self.max_velocity_mm_s,
            self.acceleration_mm_s2,
        );
        self.send_and_wait(&pkt)
    }

    fn um_to_usteps(um: f64) -> i32 {
        (um / 1000.0 * USTEPS_PER_MM_Z / DIRECTION_Z) as i32
    }

    fn read_configuration(&mut self) -> MmResult<()> {
        self.full_steps_per_rev_z = self
            .props
            .get("FullStepsPerRevZ")?
            .as_f64()
            .unwrap_or(FULLSTEPS_PER_REV_Z);
        self.screw_pitch_z_mm = self
            .props
            .get("ScrewPitchZmm")?
            .as_f64()
            .unwrap_or(SCREW_PITCH_Z_MM);
        self.microstepping_z = self
            .props
            .get("MicroSteppingDefaultZ")?
            .as_i64()
            .unwrap_or(MICROSTEPPING_Z as i64) as f64;
        self.direction_z = if self.props.get("DirectionZ")?.as_str() == "Positive" {
            1.0
        } else {
            -1.0
        };
        Ok(())
    }

    fn usteps_per_mm(&self) -> f64 {
        self.microstepping_z * self.full_steps_per_rev_z / self.screw_pitch_z_mm
    }

    fn pos_to_usteps(&self, um: f64) -> i32 {
        (um / 1000.0 * self.usteps_per_mm() / self.direction_z) as i32
    }
}

impl Default for SquidPlusZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for SquidPlusZStage {
    fn name(&self) -> &str {
        "SquidPlusZStage"
    }
    fn description(&self) -> &str {
        "Squid+ Z focus stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.read_configuration()?;
        self.z_pos_um = 0.0;
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
                self.max_velocity_mm_s = value;
                self.props.set(name, PropertyValue::Float(value))?;
                if self.initialized {
                    self.configure_velocity()?;
                }
                Ok(())
            }
            "Acceleration" => {
                let value = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(1.0..=6553.5).contains(&value) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.acceleration_mm_s2 = value;
                self.props.set(name, PropertyValue::Float(value))?;
                if self.initialized {
                    self.configure_velocity()?;
                }
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
        DeviceType::Stage
    }

    fn busy(&self) -> bool {
        false
    }
}

fn define_stage_config_props(props: &mut PropertyMap) {
    props
        .define_property(
            "FullStepsPerRevZ",
            PropertyValue::Float(FULLSTEPS_PER_REV_Z),
            false,
        )
        .unwrap();
    props
        .define_property(
            "ScrewPitchZmm",
            PropertyValue::Float(SCREW_PITCH_Z_MM),
            false,
        )
        .unwrap();
    props
        .define_property(
            "MicroSteppingDefaultZ",
            PropertyValue::Integer(MICROSTEPPING_Z as i64),
            false,
        )
        .unwrap();
    props
        .define_property(
            "DirectionZ",
            PropertyValue::String("Negative".into()),
            false,
        )
        .unwrap();
    props
        .set_allowed_values("DirectionZ", &["Positive", "Negative"])
        .unwrap();
}

impl Stage for SquidPlusZStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let usteps = self.pos_to_usteps(pos);
        let id = self.next_cmd_id();
        let pkt = protocol::build_move(id, protocol::CMD_MOVETO_Z, usteps);
        self.send_and_wait(&pkt)?;
        self.z_pos_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        Ok(self.z_pos_um)
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let usteps = self.pos_to_usteps(d);
        let id = self.next_cmd_id();
        let pkt = protocol::build_move(id, protocol::CMD_MOVE_Z, usteps);
        self.send_and_wait(&pkt)?;
        self.z_pos_um += d;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        let id = self.next_cmd_id();
        let pkt = protocol::build_home(id, protocol::AXIS_Z, protocol::HOME_POSITIVE);
        self.send_and_wait(&pkt)?;
        self.z_pos_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Err(MmError::UnsupportedCommand)
    }

    fn get_focus_direction(&self) -> FocusDirection {
        FocusDirection::TowardSample
    }

    fn is_continuous_focus_drive(&self) -> bool {
        false
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
        MockTransport::new().expect_binary(&ok_response(1))
    }

    #[test]
    fn initialize() {
        let mut dev = SquidPlusZStage::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        assert!(dev.initialized);
        assert_eq!(dev.get_position_um().unwrap(), 0.0);
    }

    #[test]
    fn set_position_um() {
        let t = make_init_transport().expect_binary(&ok_response(2));
        let mut dev = SquidPlusZStage::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_position_um(10.0).unwrap();
        assert!((dev.get_position_um().unwrap() - 10.0).abs() < 0.01);
    }

    #[test]
    fn relative_move() {
        let t = make_init_transport()
            .expect_binary(&ok_response(2))
            .expect_binary(&ok_response(3));
        let mut dev = SquidPlusZStage::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_relative_position_um(5.0).unwrap();
        dev.set_relative_position_um(-2.0).unwrap();
        assert!((dev.get_position_um().unwrap() - 3.0).abs() < 0.01);
    }

    #[test]
    fn home() {
        let t = make_init_transport().expect_binary(&ok_response(2));
        let mut dev = SquidPlusZStage::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.z_pos_um = 100.0;
        dev.home().unwrap();
        assert_eq!(dev.get_position_um().unwrap(), 0.0);
    }

    #[test]
    fn ustep_conversion() {
        // 1 mm = 1000 µm, default C++ direction is negative and casts toward zero.
        let usteps = SquidPlusZStage::um_to_usteps(1000.0);
        assert_eq!(usteps, -170666);
    }

    #[test]
    fn no_transport_error() {
        assert!(SquidPlusZStage::new().initialize().is_err());
    }

    #[test]
    fn acceleration_property_sends_axis_after_init() {
        let t = make_init_transport().expect_binary(&ok_response(2));
        let mut dev = SquidPlusZStage::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Acceleration", PropertyValue::Float(50.0))
            .unwrap();
        assert_eq!(
            dev.get_property("Acceleration").unwrap(),
            PropertyValue::Float(50.0)
        );
    }

    #[test]
    fn exposes_upstream_stage_configuration_properties() {
        let dev = SquidPlusZStage::new();
        assert!(dev.has_property("FullStepsPerRevZ"));
        assert!(dev.has_property("ScrewPitchZmm"));
        assert!(dev.has_property("MicroSteppingDefaultZ"));
        assert!(dev.has_property("DirectionZ"));
        assert!(dev.has_property("Acceleration"));
        assert!(dev.has_property("MaxVelocity"));
    }
}
