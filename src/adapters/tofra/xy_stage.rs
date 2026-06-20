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
    step_size_um: f64,
    x_um: f64,
    y_um: f64,
}

impl TofraXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            ctrl_x: "3".into(),
            ctrl_y: "4".into(),
            step_size_um: DEFAULT_LEAD_UM / (DEFAULT_STEP_DIVIDE * DEFAULT_MOTOR_STEPS),
            x_um: 0.0,
            y_um: 0.0,
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

    fn axis_init_cmd() -> String {
        let ss = DEFAULT_LEAD_UM / (DEFAULT_STEP_DIVIDE * DEFAULT_MOTOR_STEPS);
        let slvel = Self::um_to_cpp_steps(DEFAULT_SLEW_VEL_UM, ss);
        let invel = Self::um_to_cpp_steps(DEFAULT_INIT_VEL_UM, ss);
        let accel = Self::um_to_cpp_steps(DEFAULT_ACCEL_UM, ss);
        format!(
            "j{}h{}m{}V{}v{}L{}n2f{}R",
            DEFAULT_STEP_DIVIDE as i64,
            DEFAULT_HC,
            DEFAULT_RC,
            slvel,
            invel,
            accel,
            DEFAULT_LIMIT_POL
        )
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
        let ss = DEFAULT_LEAD_UM / (DEFAULT_STEP_DIVIDE * DEFAULT_MOTOR_STEPS);
        self.step_size_um = ss;
        let init = Self::axis_init_cmd();
        let cx = self.ctrl_x.clone();
        let cy = self.ctrl_y.clone();
        let rx = self.cmd(&cx, &init)?;
        Self::check_response(&rx)?;
        let ry = self.cmd(&cy, &init)?;
        Self::check_response(&ry)?;
        let px_resp = self.cmd(&cx, "?0")?;
        let x_steps = Self::parse_pos(&px_resp)?;
        let py_resp = self.cmd(&cy, "?0")?;
        let y_steps = Self::parse_pos(&py_resp)?;
        self.x_um = x_steps as f64 * self.step_size_um;
        self.y_um = y_steps as f64 * self.step_size_um;
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
        let sx = Self::um_to_cpp_steps(x, self.step_size_um);
        let sy = Self::um_to_cpp_steps(y, self.step_size_um);
        let cx = self.ctrl_x.clone();
        let cy = self.ctrl_y.clone();
        let rx = self.cmd(&cx, &format!("A{}R", sx))?;
        Self::check_response(&rx)?;
        let ry = self.cmd(&cy, &format!("A{}R", sy))?;
        Self::check_response(&ry)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        let px_resp = self.cmd(&self.ctrl_x, "?0")?;
        let py_resp = self.cmd(&self.ctrl_y, "?0")?;
        Ok((
            Self::parse_pos(&px_resp)? as f64 * self.step_size_um,
            Self::parse_pos(&py_resp)? as f64 * self.step_size_um,
        ))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let cx = self.ctrl_x.clone();
        let cy = self.ctrl_y.clone();
        if dx != 0.0 {
            let steps = Self::um_to_cpp_steps(dx, self.step_size_um);
            if steps != 0 {
                let cmd = if steps > 0 {
                    format!("P{}R", steps)
                } else {
                    format!("D{}R", -steps)
                };
                let r = self.cmd(&cx, &cmd)?;
                Self::check_response(&r)?;
            }
        }
        if dy != 0.0 {
            let steps = Self::um_to_cpp_steps(dy, self.step_size_um);
            if steps != 0 {
                let cmd = if steps > 0 {
                    format!("P{}R", steps)
                } else {
                    format!("D{}R", -steps)
                };
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
        let rx = self.cmd(&cx, "z0R")?;
        Self::check_response(&rx)?;
        let ry = self.cmd(&cy, "z0R")?;
        Self::check_response(&ry)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let cx = self.ctrl_x.clone();
        let cy = self.ctrl_y.clone();
        let rx = self.cmd(&cx, "T")?;
        Self::check_response(&rx)?;
        let ry = self.cmd(&cy, "T")?;
        Self::check_response(&ry)?;
        Ok(())
    }

    fn set_origin(&mut self) -> MmResult<()> {
        let cx = self.ctrl_x.clone();
        let cy = self.ctrl_y.clone();
        let rx = self.cmd(&cx, "z0R")?;
        Self::check_response(&rx)?;
        let ry = self.cmd(&cy, "z0R")?;
        Self::check_response(&ry)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        (self.step_size_um, self.step_size_um)
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
            .expect("/3z0R\r", "/00")
            .expect("/4z0R\r", "/00")
            .expect("/3?0\r", "/000")
            .expect("/4?0\r", "/000");
        let mut s = TofraXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
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
}
