/// TOFRA Z-Drive with IMS MDrive integrated controller.
///
/// Protocol (TX `\r`, RX `\r`):
///   Init:    `/<ctrl>j256h<HC>m<RC>V<slvel>v<invel>L<accel>n<n>R\r`
///   Query:   `/<ctrl>?0\r`         → `/0<status><steps>`
///   Abs:     `/<ctrl>A<steps>R\r`  → `/0<status>`
///   Rel +:   `/<ctrl>P<steps>R\r`  → `/0<status>`
///   Rel -:   `/<ctrl>D<steps>R\r`  → `/0<status>`
///   Stop:    `/<ctrl>T\r`          → `/0<status>`
///   Origin:  `/<ctrl>z0R\r`        → `/0<status>`
///   Home:    `/<ctrl>Z1000000000R\r` → `/0<status>`
///
/// Response format: find `/0` at index `ind`, status at `ind+2`, data from `ind+3`.
/// Status `@` = busy.
///
/// Step size: FullTurnUm / (256 × MotorSteps)
/// Defaults: FullTurnUm=100 µm, MotorSteps=400 → 0.0009765625 µm/step
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::RefCell;

const DEFAULT_FULL_TURN_UM: f64 = 100.0;
const DEFAULT_MOTOR_STEPS: f64 = 400.0;
const DEFAULT_HC: i64 = 5;
const DEFAULT_RC: i64 = 25;
const DEFAULT_SLEW_VEL_UM: f64 = 40.0;
const DEFAULT_INIT_VEL_UM: f64 = 4.0;
const DEFAULT_ACCEL_UM: f64 = 1.0;
const DEFAULT_WITH_LIMITS: i64 = 0;

pub struct TofraZStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    ctrl: String,
    slew_velocity_um: f64,
    init_velocity_um: f64,
    acceleration_um: f64,
    hold_current: i64,
    run_current: i64,
    motor_steps: i64,
    full_turn_um: i64,
    with_limits: i64,
    speed_um_s: f64,
    out1: i64,
    out2: i64,
    execute: String,
    step_size_um: f64,
    position_um: f64,
    port: String,
}

impl TofraZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String(String::new()))
            .unwrap();
        props
            .define_pre_init_property("ControllerName", PropertyValue::String("2".into()))
            .unwrap();
        props
            .define_pre_init_property("SlewVelocity", PropertyValue::Float(DEFAULT_SLEW_VEL_UM))
            .unwrap();
        props
            .define_pre_init_property("InitVelocity", PropertyValue::Float(DEFAULT_INIT_VEL_UM))
            .unwrap();
        props
            .define_pre_init_property("Acceleration", PropertyValue::Float(DEFAULT_ACCEL_UM))
            .unwrap();
        props
            .define_pre_init_property("HoldCurrent", PropertyValue::Integer(DEFAULT_HC))
            .unwrap();
        props
            .define_pre_init_property("RunCurrent", PropertyValue::Integer(DEFAULT_RC))
            .unwrap();
        props
            .define_pre_init_property(
                "MotorSteps",
                PropertyValue::Integer(DEFAULT_MOTOR_STEPS as i64),
            )
            .unwrap();
        props
            .define_pre_init_property(
                "FullTurnUm",
                PropertyValue::Integer(DEFAULT_FULL_TURN_UM as i64),
            )
            .unwrap();
        props
            .define_pre_init_property("WithLimits", PropertyValue::Integer(DEFAULT_WITH_LIMITS))
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            ctrl: "2".into(),
            slew_velocity_um: DEFAULT_SLEW_VEL_UM,
            init_velocity_um: DEFAULT_INIT_VEL_UM,
            acceleration_um: DEFAULT_ACCEL_UM,
            hold_current: DEFAULT_HC,
            run_current: DEFAULT_RC,
            motor_steps: DEFAULT_MOTOR_STEPS as i64,
            full_turn_um: DEFAULT_FULL_TURN_UM as i64,
            with_limits: DEFAULT_WITH_LIMITS,
            speed_um_s: 0.0,
            out1: 0,
            out2: 0,
            execute: String::new(),
            step_size_um: DEFAULT_FULL_TURN_UM / (256.0 * DEFAULT_MOTOR_STEPS),
            position_um: 0.0,
            port: String::new(),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = RefCell::new(Some(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.borrow_mut().as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let full = format!("/{}{}\r", self.ctrl, command);
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }

    /// Parse response: find `/0`, status at ind+2, data from ind+3.
    fn parse_pos(resp: &str) -> MmResult<i64> {
        let ind = resp
            .find("/0")
            .ok_or_else(|| MmError::LocallyDefined(format!("bad response: {}", resp)))?;
        let data = resp.get(ind + 3..).unwrap_or("").trim();
        data.parse::<i64>()
            .map_err(|_| MmError::LocallyDefined(format!("bad data: {}", resp)))
    }

    fn parse_status(resp: &str) -> MmResult<char> {
        let ind = resp
            .find("/0")
            .ok_or_else(|| MmError::LocallyDefined(format!("bad response: {}", resp)))?;
        resp[ind + 2..]
            .chars()
            .next()
            .ok_or_else(|| MmError::LocallyDefined(format!("bad response: {}", resp)))
    }

    fn check_response(resp: &str) -> MmResult<()> {
        if resp.find("/0").is_some() {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("bad response: {}", resp)))
        }
    }

    fn um_to_cpp_steps(value_um: f64, step_size_um: f64) -> i64 {
        (value_um / step_size_um + 0.5).trunc() as i64
    }

    fn clear_port(&self) -> MmResult<()> {
        self.call_transport(|t| t.purge())
    }

    fn define_runtime_properties(&mut self) -> MmResult<()> {
        for (name, value) in [
            ("Position", PropertyValue::String(String::new())),
            ("Out1", PropertyValue::Integer(0)),
            ("Out2", PropertyValue::Integer(0)),
            ("Execute", PropertyValue::String(String::new())),
            ("Speed", PropertyValue::Float(0.0)),
        ] {
            if !self.props.has_property(name) {
                self.props.define_property(name, value, false)?;
            }
        }
        Ok(())
    }

    fn send_raw_controller_command(&self, command: &str) -> MmResult<()> {
        let resp = self.cmd(command)?;
        Self::check_response(&resp)
    }

    fn move_continuous(&mut self, speed: f64) -> MmResult<()> {
        self.speed_um_s = speed;
        if speed == 0.0 {
            return self.stop();
        }
        let steps = Self::um_to_cpp_steps(speed, self.step_size_um);
        let command = if steps > 0 {
            format!("V{}P0R", steps)
        } else {
            format!("V{}D0R", -steps)
        };
        self.send_raw_controller_command(&command)
    }

    fn set_outputs(&self) -> MmResult<()> {
        self.send_raw_controller_command(&format!("J{}R", self.out1 + 2 * self.out2))
    }

    fn set_origin(&mut self) -> MmResult<()> {
        self.clear_port()?;
        let resp = self.cmd("z0R")?;
        Self::check_response(&resp)?;
        self.position_um = 0.0;
        Ok(())
    }
}

impl Default for TofraZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for TofraZStage {
    fn name(&self) -> &str {
        "TOFRA Z-Drive"
    }
    fn description(&self) -> &str {
        "TOFRA Z-Drive with Integrated Controller"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        self.define_runtime_properties()?;
        let ss = self.full_turn_um as f64 / (256.0 * self.motor_steps as f64);
        self.step_size_um = ss;
        let slvel = Self::um_to_cpp_steps(self.slew_velocity_um, ss);
        let invel = Self::um_to_cpp_steps(self.init_velocity_um, ss);
        let accel = Self::um_to_cpp_steps(self.acceleration_um, ss);
        let init_cmd = format!(
            "j256h{}m{}V{}v{}L{}n{}R",
            self.hold_current,
            self.run_current,
            slvel,
            invel,
            accel,
            self.with_limits * 2
        );
        self.clear_port()?;
        let resp = self.cmd(&init_cmd)?;
        Self::check_response(&resp)?;
        self.clear_port()?;
        let pos_resp = self.cmd("?0")?;
        let steps = Self::parse_pos(&pos_resp)?;
        self.position_um = steps as f64 * self.step_size_um;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Port" => Ok(PropertyValue::String(self.port.clone())),
            "ControllerName" => Ok(PropertyValue::String(self.ctrl.clone())),
            "SlewVelocity" => Ok(PropertyValue::Float(self.slew_velocity_um)),
            "InitVelocity" => Ok(PropertyValue::Float(self.init_velocity_um)),
            "Acceleration" => Ok(PropertyValue::Float(self.acceleration_um)),
            "HoldCurrent" => Ok(PropertyValue::Integer(self.hold_current)),
            "RunCurrent" => Ok(PropertyValue::Integer(self.run_current)),
            "MotorSteps" => Ok(PropertyValue::Integer(self.motor_steps)),
            "FullTurnUm" => Ok(PropertyValue::Integer(self.full_turn_um)),
            "WithLimits" => Ok(PropertyValue::Integer(self.with_limits)),
            "Position" => Ok(PropertyValue::String(format!(
                "{}",
                self.get_position_um()?
            ))),
            "Speed" => Ok(PropertyValue::Float(self.speed_um_s)),
            "Out1" => Ok(PropertyValue::Integer(self.out1)),
            "Out2" => Ok(PropertyValue::Integer(self.out2)),
            "Execute" => Ok(PropertyValue::String(self.execute.clone())),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => {
                if let Some(e) = self.props.entry_mut("Port") {
                    e.value = PropertyValue::String(self.port.clone());
                }
                Ok(())
            }
            "Port" => {
                self.port = val.as_str().to_string();
                self.props
                    .set(name, PropertyValue::String(self.port.clone()))
            }
            "ControllerName" if !self.initialized => {
                self.ctrl = val.as_str().to_string();
                self.props
                    .set(name, PropertyValue::String(self.ctrl.clone()))
            }
            "SlewVelocity" if !self.initialized => {
                self.slew_velocity_um = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.slew_velocity_um))
            }
            "InitVelocity" if !self.initialized => {
                self.init_velocity_um = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.init_velocity_um))
            }
            "Acceleration" if !self.initialized => {
                self.acceleration_um = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.acceleration_um))
            }
            "HoldCurrent" if !self.initialized => {
                self.hold_current = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.hold_current))
            }
            "RunCurrent" if !self.initialized => {
                self.run_current = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.run_current))
            }
            "MotorSteps" if !self.initialized => {
                self.motor_steps = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.motor_steps))
            }
            "FullTurnUm" if !self.initialized => {
                self.full_turn_um = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.full_turn_um))
            }
            "WithLimits" if !self.initialized => {
                self.with_limits = if val.as_i64().ok_or(MmError::InvalidPropertyValue)? == 0 {
                    0
                } else {
                    1
                };
                self.props
                    .set(name, PropertyValue::Integer(self.with_limits))
            }
            "Position" => {
                let text = val.as_str().to_string();
                self.props.set(name, PropertyValue::String(text.clone()))?;
                match text.as_str() {
                    "ORIGIN" => self.set_origin(),
                    "HOME" => self.home(),
                    _ => match text.parse::<f64>() {
                        Ok(pos) => self.set_position_um(pos),
                        Err(_) => Ok(()),
                    },
                }
            }
            "Speed" => {
                let speed = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(speed))?;
                self.move_continuous(speed)
            }
            "Out1" => {
                self.out1 = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(self.out1))?;
                self.set_outputs()
            }
            "Out2" => {
                self.out2 = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(self.out2))?;
                self.set_outputs()
            }
            "Execute" => {
                self.execute = val.as_str().to_string();
                self.props
                    .set(name, PropertyValue::String(self.execute.clone()))?;
                let command = self.execute.clone();
                self.send_raw_controller_command(&command)
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
        self.cmd("Q")
            .and_then(|resp| Self::parse_status(&resp))
            .map(|status| status == '@')
            .unwrap_or(false)
    }
}

impl Stage for TofraZStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let steps = Self::um_to_cpp_steps(pos, self.step_size_um);
        self.clear_port()?;
        let resp = self.cmd(&format!("A{}R", steps))?;
        Self::check_response(&resp)?;
        self.position_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        self.clear_port()?;
        let resp = self.cmd("?0")?;
        Ok(Self::parse_pos(&resp)? as f64 * self.step_size_um)
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        if d == 0.0 {
            return Ok(());
        }
        let steps = Self::um_to_cpp_steps(d, self.step_size_um);
        if steps == 0 {
            return Ok(());
        }
        self.clear_port()?;
        let resp = if steps > 0 {
            self.cmd(&format!("P{}R", steps))?
        } else {
            self.cmd(&format!("D{}R", -steps))?
        };
        Self::check_response(&resp)?;
        self.position_um += d;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        self.clear_port()?;
        let resp = if self.with_limits == 0 {
            self.cmd("z0R")?
        } else {
            self.cmd("Z1000000000R")?
        };
        Self::check_response(&resp)?;
        self.position_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        self.clear_port()?;
        let resp = self.cmd("T")?;
        Self::check_response(&resp)?;
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((0.0, 10000.0))
    }
    fn get_focus_direction(&self) -> FocusDirection {
        FocusDirection::Unknown
    }
    fn is_continuous_focus_drive(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn init_cmd() -> String {
        // step_size = 100/(256*400) = 0.0009765625
        // slvel = round(40/0.0009765625) = 40960
        // invel = round(4/0.0009765625) = 4096
        // accel = round(1/0.0009765625) = 1024
        format!("/2j256h{}m{}V40960v4096L1024n0R\r", DEFAULT_HC, DEFAULT_RC)
    }

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect(&init_cmd(), "/00")
            .expect("/2?0\r", "/000")
    }

    #[test]
    fn initialize() {
        let t = make_init_transport().expect("/2?0\r", "/000");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap()).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        // 1.0 µm / 0.0009765625 µm/step = 1024 steps
        let t = make_init_transport()
            .expect("/2A1024R\r", "/00")
            .expect("/2?0\r", "/001024");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(1.0).unwrap();
        assert!((s.get_position_um().unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn move_relative_pos() {
        // 0.5 µm / 0.0009765625 = 512 steps
        let t = make_init_transport()
            .expect("/2P512R\r", "/00")
            .expect("/2?0\r", "/00512");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(0.5).unwrap();
        assert!((s.get_position_um().unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn move_relative_neg() {
        // Upstream casts (d / step + 0.5) to long, so negative values truncate toward zero.
        let t = make_init_transport()
            .expect("/2D511R\r", "/00")
            .expect("/2?0\r", "/00-512");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(-0.5).unwrap();
        assert!((s.get_position_um().unwrap() + 0.5).abs() < 1e-9);
    }

    #[test]
    fn move_absolute_neg_uses_cpp_cast() {
        let t = make_init_transport()
            .expect("/2A-511R\r", "/00")
            .expect("/2?0\r", "/00-512");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(-0.5).unwrap();
        assert!((s.get_position_um().unwrap() + 0.5).abs() < 1e-9);
    }

    #[test]
    fn home() {
        let t = make_init_transport()
            .expect("/2z0R\r", "/00")
            .expect("/2?0\r", "/000");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert!((s.get_position_um().unwrap()).abs() < 1e-9);
    }

    #[test]
    fn position_origin_with_limits_still_uses_origin_command() {
        let t = MockTransport::new()
            .expect("/2j256h5m25V40960v4096L1024n2R\r", "/00")
            .expect("/2?0\r", "/000")
            .expect("/2z0R\r", "/00");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.set_property("WithLimits", PropertyValue::Integer(1))
            .unwrap();
        s.initialize().unwrap();
        s.set_property("Position", PropertyValue::String("ORIGIN".into()))
            .unwrap();
    }

    #[test]
    fn position_home_with_limits_uses_hardware_home_command() {
        let t = MockTransport::new()
            .expect("/2j256h5m25V40960v4096L1024n2R\r", "/00")
            .expect("/2?0\r", "/000")
            .expect("/2Z1000000000R\r", "/00");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.set_property("WithLimits", PropertyValue::Integer(1))
            .unwrap();
        s.initialize().unwrap();
        s.set_property("Position", PropertyValue::String("HOME".into()))
            .unwrap();
    }

    #[test]
    fn stop() {
        let t = make_init_transport().expect("/2T\r", "/00");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.stop().unwrap();
    }

    #[test]
    fn no_transport_error() {
        assert!(TofraZStage::new().initialize().is_err());
    }

    #[test]
    fn busy_polls_controller_status() {
        let t = make_init_transport().expect("/2Q\r", "/0@");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
    }

    #[test]
    fn config_port_revert_and_runtime_actions() {
        let t = MockTransport::new()
            .expect("/5j256h5m25V20480v4096L1024n2R\r", "/00")
            .expect("/5?0\r", "/000")
            .expect("/5V512P0R\r", "/00")
            .expect("/5J1R\r", "/00")
            .expect("/5J3R\r", "/00")
            .expect("/5X1R\r", "/00");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        s.set_property("ControllerName", PropertyValue::String("5".into()))
            .unwrap();
        s.set_property("SlewVelocity", PropertyValue::Float(20.0))
            .unwrap();
        s.set_property("WithLimits", PropertyValue::Integer(1))
            .unwrap();
        s.initialize().unwrap();
        s.set_property("Port", PropertyValue::String("COM2".into()))
            .unwrap();
        assert_eq!(
            s.get_property("Port").unwrap(),
            PropertyValue::String("COM1".into())
        );
        s.set_property("Speed", PropertyValue::Float(0.5)).unwrap();
        s.set_property("Out1", PropertyValue::Integer(1)).unwrap();
        s.set_property("Out2", PropertyValue::Integer(1)).unwrap();
        s.set_property("Execute", PropertyValue::String("X1R".into()))
            .unwrap();
    }
}
