/// Zaber ASCII protocol XY stage (single dual-axis controller).
///
/// Protocol: same as Stage — `/<device> <axis> <command>\n`.
/// Default axis mapping: X=axis 2, Y=axis 1 (matches Zaber ASR two-axis stage).
///
/// Init per axis:
///   `/<d> <ax> get resolution\n` → `@.. .. IDLE -- <resolution>`
///   `/<d> <ax> get pos\n`         → `@.. .. IDLE -- <steps>`
/// Move:
///   `/<d> <ax> move abs <steps>\n` / `move rel <steps>\n`
/// Home:
///   `/<d> 0 home\n` (homes all axes)
/// Stop:
///   `/<d> 0 stop\n`
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

const DEFAULT_MOTOR_STEPS: f64 = 200.0;
const DEFAULT_LINEAR_MOTION_MM: f64 = 2.0;
const CONV_FACTOR: f64 = 1.6384;

const PROP_PORT: &str = "Port";
const PROP_ZABER_PORT: &str = "Zaber Serial Port";
const PROP_CONTROLLER: &str = "Controller Device Number";
const PROP_CONTROLLER_Y: &str = "Controller Device Number (Y Axis)";
const PROP_AXIS_X: &str = "Axis Number (X Axis)";
const PROP_AXIS_Y: &str = "Axis Number (Y Axis)";
const PROP_LOCKSTEP_X: &str = "Lockstep Group (X Axis)";
const PROP_LOCKSTEP_Y: &str = "Lockstep Group (Y Axis)";
const PROP_MOTOR_STEPS_X: &str = "Motor Steps Per Rev (X Axis)";
const PROP_MOTOR_STEPS_Y: &str = "Motor Steps Per Rev (Y Axis)";
const PROP_LINEAR_MOTION_X: &str = "Linear Motion Per Motor Rev (X Axis) [mm]";
const PROP_LINEAR_MOTION_Y: &str = "Linear Motion Per Motor Rev (Y Axis) [mm]";
const PROP_SPEED_X: &str = "Speed X [mm/s]";
const PROP_SPEED_Y: &str = "Speed Y [mm/s]";
const PROP_ACCEL_X: &str = "Acceleration X [m/s^2]";
const PROP_ACCEL_Y: &str = "Acceleration Y [m/s^2]";
const PROP_MIRROR_X: &str = "TransposeMirrorX";
const PROP_MIRROR_Y: &str = "TransposeMirrorY";

pub struct ZaberXYStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    device_addr_x: u32,
    device_addr_y: u32,
    device_addr_y_initialized: bool,
    axis_x: u32,
    axis_y: u32,
    lockstep_group_x: u32,
    lockstep_group_y: u32,
    motor_steps_x: f64,
    motor_steps_y: f64,
    linear_motion_x_mm: f64,
    linear_motion_y_mm: f64,
    step_size_x_um: f64,
    step_size_y_um: f64,
    x_um: f64,
    y_um: f64,
    range_measured: bool,
}

impl ZaberXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property(PROP_PORT, PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_pre_init_property(PROP_ZABER_PORT, PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_pre_init_property(PROP_CONTROLLER, PropertyValue::Integer(1))
            .unwrap();
        props
            .set_property_limits(PROP_CONTROLLER, 1.0, 99.0)
            .unwrap();
        props
            .define_pre_init_property(PROP_CONTROLLER_Y, PropertyValue::String(String::new()))
            .unwrap();
        props
            .define_pre_init_property(PROP_AXIS_X, PropertyValue::Integer(2))
            .unwrap();
        props.set_property_limits(PROP_AXIS_X, 1.0, 9.0).unwrap();
        props
            .define_pre_init_property(PROP_AXIS_Y, PropertyValue::Integer(1))
            .unwrap();
        props.set_property_limits(PROP_AXIS_Y, 1.0, 9.0).unwrap();
        props
            .define_pre_init_property(PROP_LOCKSTEP_X, PropertyValue::Integer(0))
            .unwrap();
        props
            .set_property_limits(PROP_LOCKSTEP_X, 0.0, 3.0)
            .unwrap();
        props
            .define_pre_init_property(PROP_LOCKSTEP_Y, PropertyValue::Integer(0))
            .unwrap();
        props
            .set_property_limits(PROP_LOCKSTEP_Y, 0.0, 3.0)
            .unwrap();
        props
            .define_pre_init_property(
                PROP_MOTOR_STEPS_X,
                PropertyValue::Integer(DEFAULT_MOTOR_STEPS as i64),
            )
            .unwrap();
        props
            .define_pre_init_property(
                PROP_MOTOR_STEPS_Y,
                PropertyValue::Integer(DEFAULT_MOTOR_STEPS as i64),
            )
            .unwrap();
        props
            .define_pre_init_property(
                PROP_LINEAR_MOTION_X,
                PropertyValue::Float(DEFAULT_LINEAR_MOTION_MM),
            )
            .unwrap();
        props
            .define_pre_init_property(
                PROP_LINEAR_MOTION_Y,
                PropertyValue::Float(DEFAULT_LINEAR_MOTION_MM),
            )
            .unwrap();
        props
            .define_property(PROP_SPEED_X, PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property(PROP_SPEED_Y, PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property(PROP_ACCEL_X, PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property(PROP_ACCEL_Y, PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property(PROP_MIRROR_X, PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property(PROP_MIRROR_Y, PropertyValue::Integer(0), false)
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            device_addr_x: 1,
            device_addr_y: 1,
            device_addr_y_initialized: false,
            axis_x: 2,
            axis_y: 1,
            lockstep_group_x: 0,
            lockstep_group_y: 0,
            motor_steps_x: DEFAULT_MOTOR_STEPS,
            motor_steps_y: DEFAULT_MOTOR_STEPS,
            linear_motion_x_mm: DEFAULT_LINEAR_MOTION_MM,
            linear_motion_y_mm: DEFAULT_LINEAR_MOTION_MM,
            step_size_x_um: 0.15625,
            step_size_y_um: 0.15625,
            x_um: 0.0,
            y_um: 0.0,
            range_measured: false,
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
        let mut transport = self.transport.borrow_mut();
        match transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd_addr_axis(&self, device_addr: u32, axis: u32, command: &str) -> MmResult<String> {
        let full = format!("/{} {} {}\n", device_addr, axis, command);
        self.call_transport(|t| {
            let resp = t.send_recv(&full)?.trim().to_string();
            Self::reject_error(&resp)?;
            Ok(resp)
        })
    }

    fn cmd_device(&self, device_addr: u32, command: &str) -> MmResult<String> {
        self.cmd_addr_axis(device_addr, 0, command)
    }

    fn is_single_controller(&self) -> bool {
        !self.device_addr_y_initialized || self.device_addr_x == self.device_addr_y
    }

    fn parse_data(resp: &str) -> Option<i64> {
        resp.split_whitespace().nth(4).and_then(|s| s.parse().ok())
    }

    fn parse_status(resp: &str) -> Option<&str> {
        resp.split_whitespace().nth(2)
    }

    fn reject_error(resp: &str) -> MmResult<()> {
        let mut fields = resp.split_whitespace();
        let _device = fields.next();
        let _axis = fields.next();
        let _status = fields.next();
        let flags = fields.next();
        let data = fields.next();
        if flags == Some("RJ") {
            return match data {
                Some("BADCOMMAND") => Err(MmError::UnsupportedCommand),
                Some(reason) => Err(MmError::LocallyDefined(format!(
                    "Zaber command rejected: {}",
                    reason
                ))),
                None => Err(MmError::LocallyDefined("Zaber command rejected".into())),
            };
        }
        Ok(())
    }

    fn get_setting(&self, device_addr: u32, axis: u32, setting: &str) -> MmResult<i64> {
        let resp = self.cmd_addr_axis(device_addr, axis, &format!("get {}", setting))?;
        Self::parse_data(&resp)
            .ok_or_else(|| MmError::LocallyDefined(format!("bad response: {}", resp)))
    }

    fn set_setting(&self, device_addr: u32, axis: u32, setting: &str, value: i64) -> MmResult<()> {
        self.cmd_addr_axis(device_addr, axis, &format!("set {} {}", setting, value))?;
        Ok(())
    }

    fn get_resolution(&self, device_addr: u32, axis: u32) -> MmResult<f64> {
        let resp = self.cmd_addr_axis(device_addr, axis, "get resolution")?;
        Self::parse_data(&resp)
            .map(|r| r as f64)
            .ok_or_else(|| MmError::LocallyDefined(format!("bad response: {}", resp)))
    }

    fn get_pos_steps(&self, device_addr: u32, axis: u32) -> MmResult<i64> {
        let resp = self.cmd_addr_axis(device_addr, axis, "get pos")?;
        Self::parse_data(&resp)
            .ok_or_else(|| MmError::LocallyDefined(format!("bad response: {}", resp)))
    }

    fn move_axis(
        &self,
        device_addr: u32,
        axis: u32,
        lockstep_group: u32,
        kind: &str,
        steps: i64,
    ) -> MmResult<()> {
        if lockstep_group > 0 {
            self.cmd_device(
                device_addr,
                &format!("lockstep {} move {} {}", lockstep_group, kind, steps),
            )?;
        } else {
            self.cmd_addr_axis(device_addr, axis, &format!("move {} {}", kind, steps))?;
        }
        Ok(())
    }

    fn speed_data_to_mm_s(step_size_um: f64, data: i64) -> f64 {
        (data as f64 / CONV_FACTOR) * step_size_um / 1000.0
    }

    fn speed_mm_s_to_data(step_size_um: f64, speed: f64) -> i64 {
        let mut data = (speed * CONV_FACTOR * 1000.0 / step_size_um).round() as i64;
        if data == 0 && speed != 0.0 {
            data = 1;
        }
        data
    }

    fn accel_data_to_m_s2(step_size_um: f64, data: i64) -> f64 {
        (data as f64 * 10.0 / CONV_FACTOR) * step_size_um / 1000.0
    }

    fn accel_m_s2_to_data(step_size_um: f64, accel: f64) -> i64 {
        let mut data = (accel * CONV_FACTOR * 100.0 / step_size_um).round() as i64;
        if data == 0 && accel != 0.0 {
            data = 1;
        }
        data
    }

    pub fn move_velocity_mm_s(&mut self, vx: f64, vy: f64) -> MmResult<()> {
        let sx = Self::speed_mm_s_to_data(self.step_size_x_um, vx);
        let sy = Self::speed_mm_s_to_data(self.step_size_y_um, vy);
        self.move_axis(
            self.device_addr_x,
            self.axis_x,
            self.lockstep_group_x,
            "vel",
            sx,
        )?;
        self.move_axis(
            self.device_addr_y,
            self.axis_y,
            self.lockstep_group_y,
            "vel",
            sy,
        )?;
        Ok(())
    }
}

impl Default for ZaberXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ZaberXYStage {
    fn name(&self) -> &str {
        "ZaberXYStage"
    }
    fn description(&self) -> &str {
        "Zaber XY stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        if self.is_single_controller() {
            let axis_count = self.get_setting(self.device_addr_x, 0, "system.axiscount")?;
            if axis_count < 2 {
                return Err(MmError::NotSupported);
            }
            self.device_addr_y = self.device_addr_x;
        }
        let res_x = self.get_resolution(self.device_addr_x, self.axis_x)?;
        self.step_size_x_um = self.linear_motion_x_mm / self.motor_steps_x / res_x * 1000.0;
        let res_y = self.get_resolution(self.device_addr_y, self.axis_y)?;
        self.step_size_y_um = self.linear_motion_y_mm / self.motor_steps_y / res_y * 1000.0;
        let ax = self.axis_x;
        let ay = self.axis_y;
        let x_steps = self.get_pos_steps(self.device_addr_x, ax)?;
        let y_steps = self.get_pos_steps(self.device_addr_y, ay)?;
        self.x_um = x_steps as f64 * self.step_size_x_um;
        self.y_um = y_steps as f64 * self.step_size_y_um;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        self.range_measured = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if self.initialized {
            match name {
                PROP_SPEED_X => {
                    let data = self.get_setting(self.device_addr_x, self.axis_x, "maxspeed")?;
                    return Ok(PropertyValue::Float(Self::speed_data_to_mm_s(
                        self.step_size_x_um,
                        data,
                    )));
                }
                PROP_SPEED_Y => {
                    let data = self.get_setting(self.device_addr_y, self.axis_y, "maxspeed")?;
                    return Ok(PropertyValue::Float(Self::speed_data_to_mm_s(
                        self.step_size_y_um,
                        data,
                    )));
                }
                PROP_ACCEL_X => {
                    let data = self.get_setting(self.device_addr_x, self.axis_x, "accel")?;
                    return Ok(PropertyValue::Float(Self::accel_data_to_m_s2(
                        self.step_size_x_um,
                        data,
                    )));
                }
                PROP_ACCEL_Y => {
                    let data = self.get_setting(self.device_addr_y, self.axis_y, "accel")?;
                    return Ok(PropertyValue::Float(Self::accel_data_to_m_s2(
                        self.step_size_y_um,
                        data,
                    )));
                }
                _ => {}
            }
        }
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            PROP_PORT | PROP_ZABER_PORT => {
                self.props.set(PROP_PORT, val.clone()).ok();
                self.props.set(PROP_ZABER_PORT, val)
            }
            PROP_CONTROLLER => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(v))?;
                self.device_addr_x = v as u32;
                if !self.device_addr_y_initialized {
                    self.device_addr_y = self.device_addr_x;
                }
                Ok(())
            }
            PROP_CONTROLLER_Y => {
                let raw = val.to_string();
                if raw.trim().is_empty() {
                    self.device_addr_y_initialized = false;
                    self.device_addr_y = self.device_addr_x;
                    self.props.set(name, PropertyValue::String(String::new()))
                } else {
                    let v = raw
                        .parse::<i64>()
                        .map_err(|_| MmError::InvalidPropertyValue)?;
                    if !(1..=99).contains(&v) {
                        return Err(MmError::InvalidPropertyValue);
                    }
                    self.device_addr_y_initialized = true;
                    self.device_addr_y = v as u32;
                    self.props.set(name, PropertyValue::String(v.to_string()))
                }
            }
            PROP_AXIS_X => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(v))?;
                self.axis_x = v as u32;
                Ok(())
            }
            PROP_AXIS_Y => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(v))?;
                self.axis_y = v as u32;
                Ok(())
            }
            PROP_LOCKSTEP_X => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(v))?;
                self.lockstep_group_x = v as u32;
                Ok(())
            }
            PROP_LOCKSTEP_Y => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(v))?;
                self.lockstep_group_y = v as u32;
                Ok(())
            }
            PROP_MOTOR_STEPS_X => {
                let v = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(v as i64))?;
                self.motor_steps_x = v;
                Ok(())
            }
            PROP_MOTOR_STEPS_Y => {
                let v = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(v as i64))?;
                self.motor_steps_y = v;
                Ok(())
            }
            PROP_LINEAR_MOTION_X => {
                let v = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(v))?;
                self.linear_motion_x_mm = v;
                Ok(())
            }
            PROP_LINEAR_MOTION_Y => {
                let v = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(v))?;
                self.linear_motion_y_mm = v;
                Ok(())
            }
            PROP_SPEED_X if self.initialized => {
                let speed = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_setting(
                    self.device_addr_x,
                    self.axis_x,
                    "maxspeed",
                    Self::speed_mm_s_to_data(self.step_size_x_um, speed),
                )?;
                self.props.set(name, PropertyValue::Float(speed))
            }
            PROP_SPEED_Y if self.initialized => {
                let speed = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_setting(
                    self.device_addr_y,
                    self.axis_y,
                    "maxspeed",
                    Self::speed_mm_s_to_data(self.step_size_y_um, speed),
                )?;
                self.props.set(name, PropertyValue::Float(speed))
            }
            PROP_ACCEL_X if self.initialized => {
                let accel = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_setting(
                    self.device_addr_x,
                    self.axis_x,
                    "accel",
                    Self::accel_m_s2_to_data(self.step_size_x_um, accel),
                )?;
                self.props.set(name, PropertyValue::Float(accel))
            }
            PROP_ACCEL_Y if self.initialized => {
                let accel = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_setting(
                    self.device_addr_y,
                    self.axis_y,
                    "accel",
                    Self::accel_m_s2_to_data(self.step_size_y_um, accel),
                )?;
                self.props.set(name, PropertyValue::Float(accel))
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
            .cmd_device(self.device_addr_x, "")
            .ok()
            .and_then(|resp| Self::parse_status(&resp).map(|s| s != "IDLE"))
            .unwrap_or(false);
        let y_busy = if self.is_single_controller() {
            false
        } else {
            self.cmd_device(self.device_addr_y, "")
                .ok()
                .and_then(|resp| Self::parse_status(&resp).map(|s| s != "IDLE"))
                .unwrap_or(false)
        };
        x_busy || y_busy
    }
}

impl XYStage for ZaberXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let sx = (x / self.step_size_x_um).round() as i64;
        let sy = (y / self.step_size_y_um).round() as i64;
        self.move_axis(
            self.device_addr_x,
            self.axis_x,
            self.lockstep_group_x,
            "abs",
            sx,
        )?;
        self.move_axis(
            self.device_addr_y,
            self.axis_y,
            self.lockstep_group_y,
            "abs",
            sy,
        )?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        let x_steps = self.get_pos_steps(self.device_addr_x, self.axis_x)?;
        let y_steps = self.get_pos_steps(self.device_addr_y, self.axis_y)?;
        Ok((
            x_steps as f64 * self.step_size_x_um,
            y_steps as f64 * self.step_size_y_um,
        ))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let sx = (dx / self.step_size_x_um).round() as i64;
        let sy = (dy / self.step_size_y_um).round() as i64;
        self.move_axis(
            self.device_addr_x,
            self.axis_x,
            self.lockstep_group_x,
            "rel",
            sx,
        )?;
        self.move_axis(
            self.device_addr_y,
            self.axis_y,
            self.lockstep_group_y,
            "rel",
            sy,
        )?;
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        self.range_measured = false;
        if self.lockstep_group_x > 0 {
            self.cmd_device(
                self.device_addr_x,
                &format!("lockstep {} home", self.lockstep_group_x),
            )?;
        } else {
            match self.cmd_device(self.device_addr_x, "tools findrange") {
                Err(MmError::UnsupportedCommand) => {
                    self.cmd_device(self.device_addr_x, "home")?;
                }
                other => {
                    other?;
                }
            }
        }
        if !self.is_single_controller() {
            if self.lockstep_group_y > 0 {
                self.cmd_device(
                    self.device_addr_y,
                    &format!("lockstep {} home", self.lockstep_group_y),
                )?;
            } else {
                match self.cmd_device(self.device_addr_y, "tools findrange") {
                    Err(MmError::UnsupportedCommand) => {
                        self.cmd_device(self.device_addr_y, "home")?;
                    }
                    other => {
                        other?;
                    }
                }
            }
        }
        self.range_measured = true;
        let mirror_x = self
            .props
            .get(PROP_MIRROR_X)
            .ok()
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
            != 0;
        let mirror_y = self
            .props
            .get(PROP_MIRROR_Y)
            .ok()
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
            != 0;
        let x_axis = if self.lockstep_group_x > 0 {
            0
        } else {
            self.axis_x
        };
        let y_axis = if self.lockstep_group_y > 0 {
            0
        } else {
            self.axis_y
        };
        let x_cmd = if self.lockstep_group_x > 0 {
            format!(
                "lockstep {} move {}",
                self.lockstep_group_x,
                if mirror_x { "max" } else { "min" }
            )
        } else {
            format!("move {}", if mirror_x { "max" } else { "min" })
        };
        let y_cmd = if self.lockstep_group_y > 0 {
            format!(
                "lockstep {} move {}",
                self.lockstep_group_y,
                if mirror_y { "max" } else { "min" }
            )
        } else {
            format!("move {}", if mirror_y { "max" } else { "min" })
        };
        self.cmd_addr_axis(self.device_addr_x, x_axis, &x_cmd)?;
        self.cmd_addr_axis(self.device_addr_y, y_axis, &y_cmd)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let cmd_x = if self.lockstep_group_x > 0 {
            format!("lockstep {} stop", self.lockstep_group_x)
        } else {
            "stop".to_string()
        };
        let result_x = self.cmd_device(self.device_addr_x, &cmd_x).map(|_| ());
        if self.is_single_controller() {
            return result_x;
        }
        let cmd_y = if self.lockstep_group_y > 0 {
            format!("lockstep {} stop", self.lockstep_group_y)
        } else {
            "stop".to_string()
        };
        let result_y = self.cmd_device(self.device_addr_y, &cmd_y).map(|_| ());
        match (result_x, result_y) {
            (_, Err(e)) => Err(e),
            (Err(e), Ok(())) => Err(e),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    fn set_origin(&mut self) -> MmResult<()> {
        Ok(())
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        (self.step_size_x_um, self.step_size_y_um)
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        if !self.range_measured {
            return Err(MmError::LocallyDefined(
                "device has no reference position".into(),
            ));
        }
        let x_min = self.get_setting(self.device_addr_x, self.axis_x, "limit.min")?;
        let x_max = self.get_setting(self.device_addr_x, self.axis_x, "limit.max")?;
        let y_min = self.get_setting(self.device_addr_y, self.axis_y, "limit.min")?;
        let y_max = self.get_setting(self.device_addr_y, self.axis_y, "limit.max")?;
        Ok((
            x_min as f64 * self.step_size_x_um,
            x_max as f64 * self.step_size_x_um,
            y_min as f64 * self.step_size_y_um,
            y_max as f64 * self.step_size_y_um,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect("/1 0 get system.axiscount\n", "@01 00 IDLE -- 2")
            .expect("/1 2 get resolution\n", "@01 02 IDLE -- 64")
            .expect("/1 1 get resolution\n", "@01 01 IDLE -- 64")
            .expect("/1 2 get pos\n", "@01 02 IDLE -- 0")
            .expect("/1 1 get pos\n", "@01 01 IDLE -- 0")
    }

    #[test]
    fn initialize() {
        let t = make_init_transport()
            .expect("/1 2 get pos\n", "@01 02 IDLE -- 0")
            .expect("/1 1 get pos\n", "@01 01 IDLE -- 0");
        let mut s = ZaberXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn move_absolute() {
        // 100 µm / 0.15625 = 640, 200 µm / 0.15625 = 1280
        let t = make_init_transport()
            .expect("/1 2 move abs 640\n", "@01 02 IDLE -- 640")
            .expect("/1 1 move abs 1280\n", "@01 01 IDLE -- 1280")
            .expect("/1 2 get pos\n", "@01 02 IDLE -- 640")
            .expect("/1 1 get pos\n", "@01 01 IDLE -- 1280");
        let mut s = ZaberXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(100.0, 200.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 100.0).abs() < 0.01);
        assert!((y - 200.0).abs() < 0.01);
    }

    #[test]
    fn move_relative() {
        // 50 µm / 0.15625 = 320, -50 µm / 0.15625 = -320
        let t = make_init_transport()
            .expect("/1 2 move rel 320\n", "@01 02 IDLE -- 320")
            .expect("/1 1 move rel -320\n", "@01 01 IDLE -- -320")
            .expect("/1 2 get pos\n", "@01 02 IDLE -- 320")
            .expect("/1 1 get pos\n", "@01 01 IDLE -- -320");
        let mut s = ZaberXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(50.0, -50.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 50.0).abs() < 0.01);
        assert!((y + 50.0).abs() < 0.01);
    }

    #[test]
    fn home() {
        let t = make_init_transport()
            .expect("/1 0 tools findrange\n", "@01 00 IDLE -- OK")
            .expect("/1 2 move min\n", "@01 02 IDLE -- OK")
            .expect("/1 1 move min\n", "@01 01 IDLE -- OK")
            .expect("/1 2 get pos\n", "@01 02 IDLE -- 0")
            .expect("/1 1 get pos\n", "@01 01 IDLE -- 0");
        let mut s = ZaberXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn home_falls_back_to_home_when_findrange_is_unsupported() {
        let t = make_init_transport()
            .expect("/1 0 tools findrange\n", "@01 00 IDLE RJ BADCOMMAND")
            .expect("/1 0 home\n", "@01 00 IDLE -- OK")
            .expect("/1 2 move min\n", "@01 02 IDLE -- OK")
            .expect("/1 1 move min\n", "@01 01 IDLE -- OK")
            .expect("/1 2 get pos\n", "@01 02 IDLE -- 0")
            .expect("/1 1 get pos\n", "@01 01 IDLE -- 0");
        let mut s = ZaberXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn limits_unsupported_without_measured_range() {
        let s = ZaberXYStage::new();
        assert!(matches!(
            s.get_limits_um().unwrap_err(),
            MmError::LocallyDefined(_)
        ));
    }

    #[test]
    fn busy_queries_live_controller_status() {
        let t = make_init_transport().expect("/1 0 \n", "@01 00 BUSY -- 0");
        let mut s = ZaberXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
    }

    #[test]
    fn speed_accel_properties_use_live_axis_settings() {
        let t = make_init_transport()
            .expect("/1 2 get maxspeed\n", "@01 02 IDLE -- 10486")
            .expect("/1 2 set maxspeed 20972\n", "@01 02 IDLE -- OK")
            .expect("/1 1 get accel\n", "@01 01 IDLE -- 1049")
            .expect("/1 1 set accel 2097\n", "@01 01 IDLE -- OK");
        let mut s = ZaberXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_property(PROP_SPEED_X).unwrap().as_f64().unwrap() - 1.0).abs() < 0.001);
        s.set_property(PROP_SPEED_X, PropertyValue::Float(2.0))
            .unwrap();
        assert!((s.get_property(PROP_ACCEL_Y).unwrap().as_f64().unwrap() - 1.0).abs() < 0.001);
        s.set_property(PROP_ACCEL_Y, PropertyValue::Float(2.0))
            .unwrap();
    }

    #[test]
    fn home_enables_live_limits() {
        let t = make_init_transport()
            .expect("/1 0 tools findrange\n", "@01 00 IDLE -- OK")
            .expect("/1 2 move min\n", "@01 02 IDLE -- OK")
            .expect("/1 1 move min\n", "@01 01 IDLE -- OK")
            .expect("/1 2 get limit.min\n", "@01 02 IDLE -- 0")
            .expect("/1 2 get limit.max\n", "@01 02 IDLE -- 640")
            .expect("/1 1 get limit.min\n", "@01 01 IDLE -- 0")
            .expect("/1 1 get limit.max\n", "@01 01 IDLE -- 1280");
        let mut s = ZaberXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert_eq!(s.get_limits_um().unwrap(), (0.0, 100.0, 0.0, 200.0));
    }

    #[test]
    fn velocity_move_uses_zaber_data_units() {
        let t = make_init_transport()
            .expect("/1 2 move vel 10486\n", "@01 02 IDLE -- OK")
            .expect("/1 1 move vel -10486\n", "@01 01 IDLE -- OK");
        let mut s = ZaberXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.move_velocity_mm_s(1.0, -1.0).unwrap();
    }

    #[test]
    fn no_transport_error() {
        assert!(ZaberXYStage::new().initialize().is_err());
    }
}
