/// Zaber ASCII protocol Z/linear stage.
///
/// Protocol (TX `\n`, RX `\r\n`):
///   Command: `/<device> <axis> <command>\n`
///   Response: `@<device_pad> <axis_pad> <status> <flags> <data>\r\n`
///
///   Init:
///     `/<d> <a> get resolution\n`  → `@.. .. IDLE -- <resolution>`
///     `/<d> <a> get limit.min\n`   → `@.. .. IDLE -- <min_steps>`
///     `/<d> <a> get limit.max\n`   → `@.. .. IDLE -- <max_steps>`
///     `/<d> <a> get pos\n`         → `@.. .. IDLE -- <steps>`
///   Move:
///     `/<d> <a> move abs <steps>\n` → `@.. .. IDLE -- <steps>`
///     `/<d> <a> move rel <steps>\n` → `@.. .. IDLE -- <steps>`
///   Home:
///     `/<d> <a> home\n`             → `@.. .. IDLE -- OK`
///   Stop:
///     `/<d> 0 stop\n`               → `@.. 00 IDLE -- OK`
///
/// Step size: (linear_motion_mm / motor_steps / resolution) * 1000 µm/step
/// Defaults: linear_motion=2.0 mm, motor_steps=200, resolution queried from device.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::RefCell;

const DEFAULT_MOTOR_STEPS: f64 = 200.0;
const DEFAULT_LINEAR_MOTION_MM: f64 = 2.0;
const CONV_FACTOR: f64 = 1.6384;

const PROP_PORT: &str = "Port";
const PROP_ZABER_PORT: &str = "Zaber Serial Port";
const PROP_CONTROLLER: &str = "Controller Device Number";
const PROP_AXIS: &str = "Axis Number";
const PROP_LOCKSTEP: &str = "Lockstep Group";
const PROP_MOTOR_STEPS: &str = "Motor Steps Per Rev";
const PROP_LINEAR_MOTION: &str = "Linear Motion Per Motor Rev [mm]";
const PROP_SPEED: &str = "Speed [mm/s]";
const PROP_ACCEL: &str = "Acceleration [m/s^2]";

pub struct ZaberStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    device_addr: u32,
    axis: u32,
    lockstep_group: u32,
    motor_steps: f64,
    linear_motion_mm: f64,
    step_size_um: f64,
    limit_min_um: f64,
    limit_max_um: f64,
    position_um: f64,
}

impl ZaberStage {
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
            .define_pre_init_property(PROP_AXIS, PropertyValue::Integer(1))
            .unwrap();
        props.set_property_limits(PROP_AXIS, 1.0, 9.0).unwrap();
        props
            .define_pre_init_property(PROP_LOCKSTEP, PropertyValue::Integer(0))
            .unwrap();
        props.set_property_limits(PROP_LOCKSTEP, 0.0, 3.0).unwrap();
        props
            .define_pre_init_property(
                PROP_MOTOR_STEPS,
                PropertyValue::Integer(DEFAULT_MOTOR_STEPS as i64),
            )
            .unwrap();
        props
            .define_pre_init_property(
                PROP_LINEAR_MOTION,
                PropertyValue::Float(DEFAULT_LINEAR_MOTION_MM),
            )
            .unwrap();
        props
            .define_property(PROP_SPEED, PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property(PROP_ACCEL, PropertyValue::Float(0.0), false)
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            device_addr: 1,
            axis: 1,
            lockstep_group: 0,
            motor_steps: DEFAULT_MOTOR_STEPS,
            linear_motion_mm: DEFAULT_LINEAR_MOTION_MM,
            step_size_um: 0.15625,
            limit_min_um: 0.0,
            limit_max_um: 0.0,
            position_um: 0.0,
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
            let r = t.send_recv(&full)?;
            let resp = r.trim().to_string();
            Self::reject_error(&resp)?;
            Ok(resp)
        })
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        self.cmd_addr_axis(self.device_addr, self.axis, command)
    }

    fn cmd_device(&self, command: &str) -> MmResult<String> {
        self.cmd_addr_axis(self.device_addr, 0, command)
    }

    fn cmd_motion_axis(&self, command: &str) -> MmResult<String> {
        if self.lockstep_group > 0 {
            self.cmd_device(&format!("lockstep {} {}", self.lockstep_group, command))
        } else {
            self.cmd(command)
        }
    }

    /// Parse data field (5th token) from `@01 01 IDLE -- <data>`.
    fn parse_data(resp: &str) -> Option<&str> {
        resp.split_whitespace().nth(4)
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

    fn get_setting_i64(&self, setting: &str) -> MmResult<i64> {
        let resp = self.cmd(&format!("get {}", setting))?;
        Self::parse_data(&resp)
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or_else(|| MmError::LocallyDefined(format!("bad response: {}", resp)))
    }

    fn get_setting_for_axis_i64(&self, axis: u32, setting: &str) -> MmResult<i64> {
        let resp = self.cmd_addr_axis(self.device_addr, axis, &format!("get {}", setting))?;
        Self::parse_data(&resp)
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or_else(|| MmError::LocallyDefined(format!("bad response: {}", resp)))
    }

    fn set_setting_i64(&self, setting: &str, value: i64) -> MmResult<()> {
        self.cmd(&format!("set {} {}", setting, value))?;
        Ok(())
    }

    fn speed_data_to_mm_s(&self, data: i64) -> f64 {
        (data as f64 / CONV_FACTOR) * self.step_size_um / 1000.0
    }

    fn speed_mm_s_to_data(&self, speed: f64) -> i64 {
        let mut data = (speed * CONV_FACTOR * 1000.0 / self.step_size_um).round() as i64;
        if data == 0 && speed != 0.0 {
            data = 1;
        }
        data
    }

    fn accel_data_to_m_s2(&self, data: i64) -> f64 {
        (data as f64 * 10.0 / CONV_FACTOR) * self.step_size_um / 1000.0
    }

    fn accel_m_s2_to_data(&self, accel: f64) -> i64 {
        let mut data = (accel * CONV_FACTOR * 100.0 / self.step_size_um).round() as i64;
        if data == 0 && accel != 0.0 {
            data = 1;
        }
        data
    }

    pub fn move_velocity_mm_s(&mut self, velocity: f64) -> MmResult<()> {
        let data = self.speed_mm_s_to_data(velocity);
        self.cmd_motion_axis(&format!("move vel {}", data))?;
        Ok(())
    }
}

impl Default for ZaberStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ZaberStage {
    fn name(&self) -> &str {
        "ZaberStage"
    }
    fn description(&self) -> &str {
        "Zaber linear stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        let resolution = self.get_setting_i64("resolution")? as f64;
        self.step_size_um = self.linear_motion_mm / self.motor_steps / resolution * 1000.0;
        let min_steps = self.get_setting_i64("limit.min")?;
        let max_steps = self.get_setting_i64("limit.max")?;
        self.limit_min_um = min_steps as f64;
        self.limit_max_um = max_steps as f64;
        let pos_steps = self.get_setting_i64("pos")?;
        self.position_um = pos_steps as f64 * self.step_size_um;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if self.initialized {
            match name {
                PROP_SPEED => {
                    let data = self.get_setting_i64("maxspeed")?;
                    return Ok(PropertyValue::Float(self.speed_data_to_mm_s(data)));
                }
                PROP_ACCEL => {
                    let data = self.get_setting_i64("accel")?;
                    return Ok(PropertyValue::Float(self.accel_data_to_m_s2(data)));
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
                self.device_addr = v as u32;
                Ok(())
            }
            PROP_AXIS => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(v))?;
                self.axis = v as u32;
                Ok(())
            }
            PROP_LOCKSTEP => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(v))?;
                self.lockstep_group = v as u32;
                Ok(())
            }
            PROP_MOTOR_STEPS => {
                let v = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(v as i64))?;
                self.motor_steps = v;
                Ok(())
            }
            PROP_LINEAR_MOTION => {
                let v = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(v))?;
                self.linear_motion_mm = v;
                Ok(())
            }
            PROP_SPEED if self.initialized => {
                let speed = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_setting_i64("maxspeed", self.speed_mm_s_to_data(speed))?;
                self.props.set(name, PropertyValue::Float(speed))
            }
            PROP_ACCEL if self.initialized => {
                let accel = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_setting_i64("accel", self.accel_m_s2_to_data(accel))?;
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
        DeviceType::Stage
    }
    fn busy(&self) -> bool {
        self.cmd_device("")
            .ok()
            .and_then(|resp| Self::parse_status(&resp).map(|s| s != "IDLE"))
            .unwrap_or(false)
    }
}

impl Stage for ZaberStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let steps = (pos / self.step_size_um).round() as i64;
        self.cmd_motion_axis(&format!("move abs {}", steps))?;
        self.position_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        let steps = self.get_setting_i64("pos")?;
        Ok(steps as f64 * self.step_size_um)
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let steps = (d / self.step_size_um).round() as i64;
        self.cmd_motion_axis(&format!("move rel {}", steps))?;
        self.position_um += d;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        self.cmd_motion_axis("home")?;
        self.position_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        if self.lockstep_group > 0 {
            self.cmd_device(&format!("lockstep {} stop", self.lockstep_group))?;
        } else {
            self.cmd_device("stop")?;
        }
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        let min = self.get_setting_for_axis_i64(self.axis, "limit.min")?;
        let max = self.get_setting_for_axis_i64(self.axis, "limit.max")?;
        Ok((min as f64, max as f64))
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

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect("/1 1 get resolution\n", "@01 01 IDLE -- 64")
            .expect("/1 1 get limit.min\n", "@01 01 IDLE -- 0")
            .expect("/1 1 get limit.max\n", "@01 01 IDLE -- 305175")
            .expect("/1 1 get pos\n", "@01 01 IDLE -- 0")
    }

    #[test]
    fn initialize() {
        let t = make_init_transport()
            .expect("/1 1 get pos\n", "@01 01 IDLE -- 0")
            .expect("/1 1 get limit.min\n", "@01 01 IDLE -- 0")
            .expect("/1 1 get limit.max\n", "@01 01 IDLE -- 305175");
        let mut s = ZaberStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap()).abs() < 0.001);
        let (lo, hi) = s.get_limits().unwrap();
        assert_eq!(lo, 0.0);
        assert_eq!(hi, 305175.0);
    }

    #[test]
    fn move_absolute() {
        // 100 µm / 0.15625 µm/step = 640 steps
        let t = make_init_transport()
            .expect("/1 1 move abs 640\n", "@01 01 IDLE -- 640")
            .expect("/1 1 get pos\n", "@01 01 IDLE -- 640");
        let mut s = ZaberStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
        assert!((s.get_position_um().unwrap() - 100.0).abs() < 0.01);
    }

    #[test]
    fn move_relative() {
        // 50 µm / 0.15625 µm/step = 320 steps
        let t = make_init_transport()
            .expect("/1 1 move rel 320\n", "@01 01 IDLE -- 320")
            .expect("/1 1 get pos\n", "@01 01 IDLE -- 320");
        let mut s = ZaberStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(50.0).unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 0.01);
    }

    #[test]
    fn home() {
        let t = make_init_transport()
            .expect("/1 1 home\n", "@01 01 IDLE -- OK")
            .expect("/1 1 get pos\n", "@01 01 IDLE -- 0");
        let mut s = ZaberStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert!(s.get_position_um().unwrap().abs() < 0.001);
    }

    #[test]
    fn stop() {
        let t = make_init_transport().expect("/1 0 stop\n", "@01 00 IDLE -- OK");
        let mut s = ZaberStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.stop().unwrap();
    }

    #[test]
    fn busy_queries_live_status() {
        let t = make_init_transport().expect("/1 0 \n", "@01 00 BUSY -- 0");
        let mut s = ZaberStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
    }

    #[test]
    fn badcommand_response_maps_to_unsupported_command() {
        let t = make_init_transport().expect("/1 1 home\n", "@01 01 IDLE RJ BADCOMMAND");
        let mut s = ZaberStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.home().unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn speed_accel_properties_use_live_settings() {
        let t = make_init_transport()
            .expect("/1 1 get maxspeed\n", "@01 01 IDLE -- 10486")
            .expect("/1 1 set maxspeed 20972\n", "@01 01 IDLE -- OK")
            .expect("/1 1 get accel\n", "@01 01 IDLE -- 1049")
            .expect("/1 1 set accel 2097\n", "@01 01 IDLE -- OK");
        let mut s = ZaberStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_property(PROP_SPEED).unwrap().as_f64().unwrap() - 1.0).abs() < 0.001);
        s.set_property(PROP_SPEED, PropertyValue::Float(2.0))
            .unwrap();
        assert!((s.get_property(PROP_ACCEL).unwrap().as_f64().unwrap() - 1.0).abs() < 0.001);
        s.set_property(PROP_ACCEL, PropertyValue::Float(2.0))
            .unwrap();
    }

    #[test]
    fn lockstep_move_and_stop_use_device_axis() {
        let t = make_init_transport()
            .expect("/1 0 lockstep 1 move abs 640\n", "@01 00 IDLE -- 640")
            .expect("/1 0 lockstep 1 stop\n", "@01 00 IDLE -- OK");
        let mut s = ZaberStage::new().with_transport(Box::new(t));
        s.set_property(PROP_LOCKSTEP, PropertyValue::Integer(1))
            .unwrap();
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
        s.stop().unwrap();
    }

    #[test]
    fn velocity_move_uses_zaber_data_units() {
        let t = make_init_transport().expect("/1 1 move vel 10486\n", "@01 01 IDLE -- OK");
        let mut s = ZaberStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.move_velocity_mm_s(1.0).unwrap();
    }

    #[test]
    fn no_transport_error() {
        assert!(ZaberStage::new().initialize().is_err());
    }
}
