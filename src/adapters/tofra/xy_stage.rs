/// TOFRA XYStage with two IMS MDrive integrated controllers.
///
/// Protocol: same as ZStage but two separate controller addresses (X and Y).
///   Default controllers: X="3", Y="4"
///
/// Init per axis: `/<ctrl>j<SD>h<HC>m<RC>V<slvel>v<invel>L<accel>n2f<LP>R\r`
///
/// Step size: LeadUm / (StepDivide × MotorSteps)
/// Defaults: LeadUm=1000 µm, StepDivide=256, MotorSteps=200 → 0.01953125 µm/step
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

const DEFAULT_LEAD_UM: f64 = 1000.0;
const DEFAULT_STEP_DIVIDE: f64 = 256.0;
const DEFAULT_MOTOR_STEPS: f64 = 200.0;
const DEFAULT_HC: i64 = 5;
const DEFAULT_RC: i64 = 50;
const DEFAULT_SLEW_VEL_UM: f64 = 1000.0;
const DEFAULT_INIT_VEL_UM: f64 = 100.0;
const DEFAULT_ACCEL_UM: f64 = 10.0;
const DEFAULT_LIMIT_POL: i64 = 0;

pub struct TofraXYStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    ctrl_x: String,
    ctrl_y: String,
    step_divide_x: i64,
    step_divide_y: i64,
    slew_velocity_x: f64,
    slew_velocity_y: f64,
    init_velocity_x: f64,
    init_velocity_y: f64,
    acceleration_x: f64,
    acceleration_y: f64,
    hold_current_x: i64,
    hold_current_y: i64,
    run_current_x: i64,
    run_current_y: i64,
    motor_steps_x: i64,
    motor_steps_y: i64,
    lead_um_x: i64,
    lead_um_y: i64,
    limit_polarity_x: i64,
    limit_polarity_y: i64,
    step_size_um_x: f64,
    step_size_um_y: f64,
    x_um: f64,
    y_um: f64,
    speed_x: f64,
    speed_y: f64,
    out1_x: i64,
    out1_y: i64,
    out2_x: i64,
    out2_y: i64,
    execute_x: String,
    execute_y: String,
    port: String,
}

impl TofraXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String(String::new()))
            .unwrap();
        for (name, value) in [
            ("ControllerNameX", PropertyValue::String("3".into())),
            ("ControllerNameY", PropertyValue::String("4".into())),
            (
                "StepDivideX",
                PropertyValue::Integer(DEFAULT_STEP_DIVIDE as i64),
            ),
            (
                "StepDivideY",
                PropertyValue::Integer(DEFAULT_STEP_DIVIDE as i64),
            ),
            ("SlewVelocityX", PropertyValue::Float(DEFAULT_SLEW_VEL_UM)),
            ("SlewVelocityY", PropertyValue::Float(DEFAULT_SLEW_VEL_UM)),
            ("InitVelocityX", PropertyValue::Float(DEFAULT_INIT_VEL_UM)),
            ("InitVelocityY", PropertyValue::Float(DEFAULT_INIT_VEL_UM)),
            ("AccelerationX", PropertyValue::Float(DEFAULT_ACCEL_UM)),
            ("AccelerationY", PropertyValue::Float(DEFAULT_ACCEL_UM)),
            ("HoldCurrentX", PropertyValue::Integer(DEFAULT_HC)),
            ("HoldCurrentY", PropertyValue::Integer(DEFAULT_HC)),
            ("RunCurrentX", PropertyValue::Integer(DEFAULT_RC)),
            ("RunCurrentY", PropertyValue::Integer(DEFAULT_RC)),
            (
                "MotorStepsX",
                PropertyValue::Integer(DEFAULT_MOTOR_STEPS as i64),
            ),
            (
                "MotorStepsY",
                PropertyValue::Integer(DEFAULT_MOTOR_STEPS as i64),
            ),
            ("LeadUmX", PropertyValue::Integer(DEFAULT_LEAD_UM as i64)),
            ("LeadUmY", PropertyValue::Integer(DEFAULT_LEAD_UM as i64)),
            ("LimitPolarityX", PropertyValue::Integer(DEFAULT_LIMIT_POL)),
            ("LimitPolarityY", PropertyValue::Integer(DEFAULT_LIMIT_POL)),
        ] {
            props.define_pre_init_property(name, value).unwrap();
        }
        let step_size = DEFAULT_LEAD_UM / (DEFAULT_STEP_DIVIDE * DEFAULT_MOTOR_STEPS);
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            ctrl_x: "3".into(),
            ctrl_y: "4".into(),
            step_divide_x: DEFAULT_STEP_DIVIDE as i64,
            step_divide_y: DEFAULT_STEP_DIVIDE as i64,
            slew_velocity_x: DEFAULT_SLEW_VEL_UM,
            slew_velocity_y: DEFAULT_SLEW_VEL_UM,
            init_velocity_x: DEFAULT_INIT_VEL_UM,
            init_velocity_y: DEFAULT_INIT_VEL_UM,
            acceleration_x: DEFAULT_ACCEL_UM,
            acceleration_y: DEFAULT_ACCEL_UM,
            hold_current_x: DEFAULT_HC,
            hold_current_y: DEFAULT_HC,
            run_current_x: DEFAULT_RC,
            run_current_y: DEFAULT_RC,
            motor_steps_x: DEFAULT_MOTOR_STEPS as i64,
            motor_steps_y: DEFAULT_MOTOR_STEPS as i64,
            lead_um_x: DEFAULT_LEAD_UM as i64,
            lead_um_y: DEFAULT_LEAD_UM as i64,
            limit_polarity_x: DEFAULT_LIMIT_POL,
            limit_polarity_y: DEFAULT_LIMIT_POL,
            step_size_um_x: step_size,
            step_size_um_y: step_size,
            x_um: 0.0,
            y_um: 0.0,
            speed_x: 0.0,
            speed_y: 0.0,
            out1_x: 0,
            out1_y: 0,
            out2_x: 0,
            out2_y: 0,
            execute_x: String::new(),
            execute_y: String::new(),
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

    fn cmd(&self, ctrl: &str, command: &str) -> MmResult<String> {
        let full = format!("/{}{}\r", ctrl, command);
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }

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

    fn axis_init_cmd(
        step_divide: i64,
        hold_current: i64,
        run_current: i64,
        slew_velocity: f64,
        init_velocity: f64,
        acceleration: f64,
        limit_polarity: i64,
        step_size_um: f64,
    ) -> String {
        let slvel = Self::um_to_cpp_steps(slew_velocity, step_size_um);
        let invel = Self::um_to_cpp_steps(init_velocity, step_size_um);
        let accel = Self::um_to_cpp_steps(acceleration, step_size_um);
        format!(
            "j{}h{}m{}V{}v{}L{}n2f{}R",
            step_divide, hold_current, run_current, slvel, invel, accel, limit_polarity
        )
    }

    fn define_runtime_properties(&mut self) -> MmResult<()> {
        for (name, value) in [
            ("PositionX", PropertyValue::String(String::new())),
            ("PositionY", PropertyValue::String(String::new())),
            ("ExecuteX", PropertyValue::String(String::new())),
            ("ExecuteY", PropertyValue::String(String::new())),
            ("SpeedX", PropertyValue::Float(0.0)),
            ("SpeedY", PropertyValue::Float(0.0)),
            ("Out1X", PropertyValue::Integer(0)),
            ("Out1Y", PropertyValue::Integer(0)),
            ("Out2X", PropertyValue::Integer(0)),
            ("Out2Y", PropertyValue::Integer(0)),
        ] {
            if !self.props.has_property(name) {
                self.props.define_property(name, value, false)?;
            }
        }
        Ok(())
    }

    fn send_raw_controller_command(&self, ctrl: &str, command: &str) -> MmResult<()> {
        let resp = self.cmd(ctrl, command)?;
        Self::check_response(&resp)
    }

    fn move_axis_continuous(&self, ctrl: &str, speed: f64, step_size_um: f64) -> MmResult<()> {
        if speed == 0.0 {
            return self.send_raw_controller_command(ctrl, "T");
        }
        let steps = Self::um_to_cpp_steps(speed, step_size_um);
        let command = if steps > 0 {
            format!("V{}P0R", steps)
        } else {
            format!("V{}D0R", -steps)
        };
        self.send_raw_controller_command(ctrl, &command)
    }

    fn set_outputs(ctrl: &str, out1: i64, out2: i64, this: &Self) -> MmResult<()> {
        this.send_raw_controller_command(ctrl, &format!("J{}R", out1 + 2 * out2))
    }
}

impl Default for TofraXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for TofraXYStage {
    fn name(&self) -> &str {
        "TOFRA XYStage"
    }
    fn description(&self) -> &str {
        "TOFRA XYStage with Integrated Controller"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        self.define_runtime_properties()?;
        self.step_size_um_x =
            self.lead_um_x as f64 / (self.step_divide_x as f64 * self.motor_steps_x as f64);
        self.step_size_um_y =
            self.lead_um_y as f64 / (self.step_divide_y as f64 * self.motor_steps_y as f64);
        let init_x = Self::axis_init_cmd(
            self.step_divide_x,
            self.hold_current_x,
            self.run_current_x,
            self.slew_velocity_x,
            self.init_velocity_x,
            self.acceleration_x,
            self.limit_polarity_x,
            self.step_size_um_x,
        );
        let init_y = Self::axis_init_cmd(
            self.step_divide_y,
            self.hold_current_y,
            self.run_current_y,
            self.slew_velocity_y,
            self.init_velocity_y,
            self.acceleration_y,
            self.limit_polarity_y,
            self.step_size_um_y,
        );
        let cx = self.ctrl_x.clone();
        let cy = self.ctrl_y.clone();
        self.clear_port()?;
        let rx = self.cmd(&cx, &init_x)?;
        Self::check_response(&rx)?;
        self.clear_port()?;
        let ry = self.cmd(&cy, &init_y)?;
        Self::check_response(&ry)?;
        self.clear_port()?;
        let px_resp = self.cmd(&cx, "?0")?;
        let x_steps = Self::parse_pos(&px_resp)?;
        self.clear_port()?;
        let py_resp = self.cmd(&cy, "?0")?;
        let y_steps = Self::parse_pos(&py_resp)?;
        self.x_um = x_steps as f64 * self.step_size_um_x;
        self.y_um = y_steps as f64 * self.step_size_um_y;
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
            "ControllerNameX" => Ok(PropertyValue::String(self.ctrl_x.clone())),
            "ControllerNameY" => Ok(PropertyValue::String(self.ctrl_y.clone())),
            "PositionX" => Ok(PropertyValue::String(format!(
                "{}",
                self.get_xy_position_um()?.0
            ))),
            "PositionY" => Ok(PropertyValue::String(format!(
                "{}",
                self.get_xy_position_um()?.1
            ))),
            "SpeedX" => Ok(PropertyValue::Float(self.speed_x)),
            "SpeedY" => Ok(PropertyValue::Float(self.speed_y)),
            "Out1X" => Ok(PropertyValue::Integer(self.out1_x)),
            "Out1Y" => Ok(PropertyValue::Integer(self.out1_y)),
            "Out2X" => Ok(PropertyValue::Integer(self.out2_x)),
            "Out2Y" => Ok(PropertyValue::Integer(self.out2_y)),
            "ExecuteX" => Ok(PropertyValue::String(self.execute_x.clone())),
            "ExecuteY" => Ok(PropertyValue::String(self.execute_y.clone())),
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
            "ControllerNameX" if !self.initialized => {
                self.ctrl_x = val.as_str().to_string();
                self.props
                    .set(name, PropertyValue::String(self.ctrl_x.clone()))
            }
            "ControllerNameY" if !self.initialized => {
                self.ctrl_y = val.as_str().to_string();
                self.props
                    .set(name, PropertyValue::String(self.ctrl_y.clone()))
            }
            "StepDivideX" if !self.initialized => {
                self.step_divide_x = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.step_divide_x))
            }
            "StepDivideY" if !self.initialized => {
                self.step_divide_y = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.step_divide_y))
            }
            "SlewVelocityX" if !self.initialized => {
                self.slew_velocity_x = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.slew_velocity_x))
            }
            "SlewVelocityY" if !self.initialized => {
                self.slew_velocity_y = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.slew_velocity_y))
            }
            "InitVelocityX" if !self.initialized => {
                self.init_velocity_x = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.init_velocity_x))
            }
            "InitVelocityY" if !self.initialized => {
                self.init_velocity_y = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.init_velocity_y))
            }
            "AccelerationX" if !self.initialized => {
                self.acceleration_x = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.acceleration_x))
            }
            "AccelerationY" if !self.initialized => {
                self.acceleration_y = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.acceleration_y))
            }
            "HoldCurrentX" if !self.initialized => {
                self.hold_current_x = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.hold_current_x))
            }
            "HoldCurrentY" if !self.initialized => {
                self.hold_current_y = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.hold_current_y))
            }
            "RunCurrentX" if !self.initialized => {
                self.run_current_x = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.run_current_x))
            }
            "RunCurrentY" if !self.initialized => {
                self.run_current_y = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.run_current_y))
            }
            "MotorStepsX" if !self.initialized => {
                self.motor_steps_x = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.motor_steps_x))
            }
            "MotorStepsY" if !self.initialized => {
                self.motor_steps_y = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.motor_steps_y))
            }
            "LeadUmX" if !self.initialized => {
                self.lead_um_x = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(self.lead_um_x))
            }
            "LeadUmY" if !self.initialized => {
                self.lead_um_y = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(self.lead_um_y))
            }
            "LimitPolarityX" if !self.initialized => {
                self.limit_polarity_x = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.limit_polarity_x))
            }
            "LimitPolarityY" if !self.initialized => {
                self.limit_polarity_y = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.limit_polarity_y))
            }
            "PositionX" => {
                let text = val.as_str().to_string();
                self.props.set(name, PropertyValue::String(text.clone()))?;
                match text.as_str() {
                    "ORIGIN" => {
                        let ctrl = self.ctrl_x.clone();
                        self.send_raw_controller_command(&ctrl, "z0R")
                    }
                    "HOME" => {
                        let ctrl = self.ctrl_x.clone();
                        self.send_raw_controller_command(&ctrl, "Z1000000000R")
                    }
                    _ => match text.parse::<f64>() {
                        Ok(x) => self.set_xy_position_um(x, self.y_um),
                        Err(_) => Ok(()),
                    },
                }
            }
            "PositionY" => {
                let text = val.as_str().to_string();
                self.props.set(name, PropertyValue::String(text.clone()))?;
                match text.as_str() {
                    "ORIGIN" => {
                        let ctrl = self.ctrl_y.clone();
                        self.send_raw_controller_command(&ctrl, "z0R")
                    }
                    "HOME" => {
                        let ctrl = self.ctrl_y.clone();
                        self.send_raw_controller_command(&ctrl, "Z1000000000R")
                    }
                    _ => match text.parse::<f64>() {
                        Ok(y) => self.set_xy_position_um(self.x_um, y),
                        Err(_) => Ok(()),
                    },
                }
            }
            "SpeedX" => {
                self.speed_x = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.speed_x))?;
                self.move_axis_continuous(&self.ctrl_x, self.speed_x, self.step_size_um_x)
            }
            "SpeedY" => {
                self.speed_y = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.speed_y))?;
                self.move_axis_continuous(&self.ctrl_y, self.speed_y, self.step_size_um_y)
            }
            "Out1X" => {
                self.out1_x = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(self.out1_x))?;
                Self::set_outputs(&self.ctrl_x, self.out1_x, self.out2_x, self)
            }
            "Out2X" => {
                self.out2_x = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(self.out2_x))?;
                Self::set_outputs(&self.ctrl_x, self.out1_x, self.out2_x, self)
            }
            "Out1Y" => {
                self.out1_y = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(self.out1_y))?;
                Self::set_outputs(&self.ctrl_y, self.out1_y, self.out2_y, self)
            }
            "Out2Y" => {
                self.out2_y = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(self.out2_y))?;
                Self::set_outputs(&self.ctrl_y, self.out1_y, self.out2_y, self)
            }
            "ExecuteX" => {
                self.execute_x = val.as_str().to_string();
                self.props
                    .set(name, PropertyValue::String(self.execute_x.clone()))?;
                self.send_raw_controller_command(&self.ctrl_x, &self.execute_x)
            }
            "ExecuteY" => {
                self.execute_y = val.as_str().to_string();
                self.props
                    .set(name, PropertyValue::String(self.execute_y.clone()))?;
                self.send_raw_controller_command(&self.ctrl_y, &self.execute_y)
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
        let x_busy = self
            .cmd(&self.ctrl_x, "Q")
            .and_then(|resp| Self::parse_status(&resp))
            .map(|status| status == '@')
            .unwrap_or(false);
        let y_busy = self
            .cmd(&self.ctrl_y, "Q")
            .and_then(|resp| Self::parse_status(&resp))
            .map(|status| status == '@')
            .unwrap_or(false);
        x_busy || y_busy
    }
}

impl XYStage for TofraXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let sx = Self::um_to_cpp_steps(x, self.step_size_um_x);
        let sy = Self::um_to_cpp_steps(y, self.step_size_um_y);
        let cx = self.ctrl_x.clone();
        let cy = self.ctrl_y.clone();
        self.clear_port()?;
        let rx = self.cmd(&cx, &format!("A{}R", sx))?;
        Self::check_response(&rx)?;
        self.clear_port()?;
        let ry = self.cmd(&cy, &format!("A{}R", sy))?;
        Self::check_response(&ry)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        self.clear_port()?;
        let px_resp = self.cmd(&self.ctrl_x, "?0")?;
        self.clear_port()?;
        let py_resp = self.cmd(&self.ctrl_y, "?0")?;
        Ok((
            Self::parse_pos(&px_resp)? as f64 * self.step_size_um_x,
            Self::parse_pos(&py_resp)? as f64 * self.step_size_um_y,
        ))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let cx = self.ctrl_x.clone();
        let cy = self.ctrl_y.clone();
        if dx != 0.0 {
            let steps = Self::um_to_cpp_steps(dx, self.step_size_um_x);
            if steps != 0 {
                let cmd = if steps > 0 {
                    format!("P{}R", steps)
                } else {
                    format!("D{}R", -steps)
                };
                self.clear_port()?;
                let r = self.cmd(&cx, &cmd)?;
                Self::check_response(&r)?;
            }
        }
        if dy != 0.0 {
            let steps = Self::um_to_cpp_steps(dy, self.step_size_um_y);
            if steps != 0 {
                let cmd = if steps > 0 {
                    format!("P{}R", steps)
                } else {
                    format!("D{}R", -steps)
                };
                self.clear_port()?;
                let r = self.cmd(&cy, &cmd)?;
                Self::check_response(&r)?;
            }
        }
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        let cx = self.ctrl_x.clone();
        let cy = self.ctrl_y.clone();
        self.clear_port()?;
        let rx = self.cmd(&cx, "Z1000000000R")?;
        Self::check_response(&rx)?;
        self.clear_port()?;
        let ry = self.cmd(&cy, "Z1000000000R")?;
        Self::check_response(&ry)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let cx = self.ctrl_x.clone();
        let cy = self.ctrl_y.clone();
        self.clear_port()?;
        let rx = self.cmd(&cx, "T")?;
        Self::check_response(&rx)?;
        self.clear_port()?;
        let ry = self.cmd(&cy, "T")?;
        Self::check_response(&ry)?;
        Ok(())
    }

    fn set_origin(&mut self) -> MmResult<()> {
        let cx = self.ctrl_x.clone();
        let cy = self.ctrl_y.clone();
        self.clear_port()?;
        let rx = self.cmd(&cx, "z0R")?;
        Self::check_response(&rx)?;
        self.clear_port()?;
        let ry = self.cmd(&cy, "z0R")?;
        Self::check_response(&ry)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        (self.step_size_um_x, self.step_size_um_y)
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn init_cmd_str(ctrl: &str) -> String {
        // step_size = 1000/(256*200) = 0.01953125
        // slvel = round(1000/0.01953125) = 51200
        // invel = round(100/0.01953125) = 5120
        // accel = round(10/0.01953125) = 512
        format!(
            "/{}j256h{}m{}V51200v5120L512n2f{}R\r",
            ctrl, DEFAULT_HC, DEFAULT_RC, DEFAULT_LIMIT_POL
        )
    }

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect(&init_cmd_str("3"), "/00")
            .expect(&init_cmd_str("4"), "/00")
            .expect("/3?0\r", "/000")
            .expect("/4?0\r", "/000")
    }

    #[test]
    fn initialize() {
        let t = make_init_transport()
            .expect("/3?0\r", "/000")
            .expect("/4?0\r", "/000");
        let mut s = TofraXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn move_absolute() {
        // 10 µm / 0.01953125 = 512 steps
        let t = make_init_transport()
            .expect("/3A512R\r", "/00")
            .expect("/4A512R\r", "/00")
            .expect("/3?0\r", "/00512")
            .expect("/4?0\r", "/00512");
        let mut s = TofraXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(10.0, 10.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 10.0).abs() < 1e-9);
        assert!((y - 10.0).abs() < 1e-9);
    }

    #[test]
    fn move_relative() {
        // Upstream casts (d / step + 0.5) to long, so negative values truncate toward zero.
        let t = make_init_transport()
            .expect("/3P256R\r", "/00")
            .expect("/4D255R\r", "/00")
            .expect("/3?0\r", "/00256")
            .expect("/4?0\r", "/00-256");
        let mut s = TofraXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(5.0, -5.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 5.0).abs() < 1e-9);
        assert!((y + 5.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute_negative_uses_cpp_cast() {
        let t = make_init_transport()
            .expect("/3A-255R\r", "/00")
            .expect("/4A-255R\r", "/00")
            .expect("/3?0\r", "/00-256")
            .expect("/4?0\r", "/00-256");
        let mut s = TofraXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(-5.0, -5.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (-5.0, -5.0));
    }

    #[test]
    fn limits_are_unsupported() {
        let s = TofraXYStage::new();
        assert_eq!(s.get_limits_um(), Err(MmError::UnsupportedCommand));
    }

    #[test]
    fn home() {
        let t = make_init_transport()
            .expect("/3Z1000000000R\r", "/00")
            .expect("/4Z1000000000R\r", "/00")
            .expect("/3?0\r", "/000")
            .expect("/4?0\r", "/000");
        let mut s = TofraXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn position_origin_and_home_use_distinct_upstream_commands() {
        let t = make_init_transport()
            .expect("/3z0R\r", "/00")
            .expect("/4Z1000000000R\r", "/00");
        let mut s = TofraXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("PositionX", PropertyValue::String("ORIGIN".into()))
            .unwrap();
        s.set_property("PositionY", PropertyValue::String("HOME".into()))
            .unwrap();
    }

    #[test]
    fn stop() {
        let t = make_init_transport()
            .expect("/3T\r", "/00")
            .expect("/4T\r", "/00");
        let mut s = TofraXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.stop().unwrap();
    }

    #[test]
    fn no_transport_error() {
        assert!(TofraXYStage::new().initialize().is_err());
    }

    #[test]
    fn busy_polls_both_controllers() {
        let t = make_init_transport()
            .expect("/3Q\r", "/00")
            .expect("/4Q\r", "/0@");
        let mut s = TofraXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
    }

    #[test]
    fn config_port_revert_and_runtime_actions() {
        let t = make_init_transport()
            .expect("/3V256P0R\r", "/00")
            .expect("/4V255D0R\r", "/00")
            .expect("/3J1R\r", "/00")
            .expect("/3J3R\r", "/00")
            .expect("/4X1R\r", "/00");
        let mut s = TofraXYStage::new().with_transport(Box::new(t));
        s.set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        s.initialize().unwrap();
        s.set_property("Port", PropertyValue::String("COM2".into()))
            .unwrap();
        assert_eq!(
            s.get_property("Port").unwrap(),
            PropertyValue::String("COM1".into())
        );
        s.set_property("SpeedX", PropertyValue::Float(5.0)).unwrap();
        s.set_property("SpeedY", PropertyValue::Float(-5.0))
            .unwrap();
        s.set_property("Out1X", PropertyValue::Integer(1)).unwrap();
        s.set_property("Out2X", PropertyValue::Integer(1)).unwrap();
        s.set_property("ExecuteY", PropertyValue::String("X1R".into()))
            .unwrap();
    }
}
