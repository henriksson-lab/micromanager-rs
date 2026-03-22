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
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, XYStage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const DEFAULT_MOTOR_STEPS: f64 = 200.0;
const DEFAULT_LINEAR_MOTION_MM: f64 = 2.0;

pub struct ZaberXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    device_addr: u32,
    axis_x: u32,
    axis_y: u32,
    step_size_x_um: f64,
    step_size_y_um: f64,
    x_um: f64,
    y_um: f64,
}

impl ZaberXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            device_addr: 1,
            axis_x: 2,
            axis_y: 1,
            step_size_x_um: 0.15625,
            step_size_y_um: 0.15625,
            x_um: 0.0,
            y_um: 0.0,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where F: FnOnce(&mut dyn Transport) -> MmResult<R> {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&mut self, axis: u32, command: &str) -> MmResult<String> {
        let full = format!("/{} {} {}\n", self.device_addr, axis, command);
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }

    fn cmd_device(&mut self, command: &str) -> MmResult<String> {
        let full = format!("/{} 0 {}\n", self.device_addr, command);
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }

    fn parse_data(resp: &str) -> Option<i64> {
        resp.split_whitespace().nth(4).and_then(|s| s.parse().ok())
    }

    fn get_resolution(&mut self, axis: u32) -> MmResult<f64> {
        let resp = self.cmd(axis, "get resolution")?;
        Self::parse_data(&resp)
            .map(|r| r as f64)
            .ok_or_else(|| MmError::LocallyDefined(format!("bad response: {}", resp)))
    }

    fn get_pos_steps(&mut self, axis: u32) -> MmResult<i64> {
        let resp = self.cmd(axis, "get pos")?;
        Self::parse_data(&resp)
            .ok_or_else(|| MmError::LocallyDefined(format!("bad response: {}", resp)))
    }
}

impl Default for ZaberXYStage { fn default() -> Self { Self::new() } }

impl Device for ZaberXYStage {
    fn name(&self) -> &str { "ZaberXYStage" }
    fn description(&self) -> &str { "Zaber XY stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let res_x = self.get_resolution(self.axis_x)?;
        self.step_size_x_um = DEFAULT_LINEAR_MOTION_MM / DEFAULT_MOTOR_STEPS / res_x * 1000.0;
        let res_y = self.get_resolution(self.axis_y)?;
        self.step_size_y_um = DEFAULT_LINEAR_MOTION_MM / DEFAULT_MOTOR_STEPS / res_y * 1000.0;
        let ax = self.axis_x;
        let ay = self.axis_y;
        let x_steps = self.get_pos_steps(ax)?;
        let y_steps = self.get_pos_steps(ay)?;
        self.x_um = x_steps as f64 * self.step_size_x_um;
        self.y_um = y_steps as f64 * self.step_size_y_um;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> { self.props.set(name, val) }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::XYStage }
    fn busy(&self) -> bool { false }
}

impl XYStage for ZaberXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let sx = (x / self.step_size_x_um).round() as i64;
        let sy = (y / self.step_size_y_um).round() as i64;
        let ax = self.axis_x;
        let ay = self.axis_y;
        self.cmd(ax, &format!("move abs {}", sx))?;
        self.cmd(ay, &format!("move abs {}", sy))?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> { Ok((self.x_um, self.y_um)) }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let sx = (dx / self.step_size_x_um).round() as i64;
        let sy = (dy / self.step_size_y_um).round() as i64;
        let ax = self.axis_x;
        let ay = self.axis_y;
        self.cmd(ax, &format!("move rel {}", sx))?;
        self.cmd(ay, &format!("move rel {}", sy))?;
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        self.cmd_device("home")?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        self.cmd_device("stop")?;
        Ok(())
    }

    fn set_origin(&mut self) -> MmResult<()> { Ok(()) }

    fn get_step_size_um(&self) -> (f64, f64) { (self.step_size_x_um, self.step_size_y_um) }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((0.0, 0.0, 0.0, 0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect("/1 2 get resolution\n", "@01 02 IDLE -- 64")
            .expect("/1 1 get resolution\n", "@01 01 IDLE -- 64")
            .expect("/1 2 get pos\n",         "@01 02 IDLE -- 0")
            .expect("/1 1 get pos\n",         "@01 01 IDLE -- 0")
    }

    #[test]
    fn initialize() {
        let mut s = ZaberXYStage::new().with_transport(Box::new(make_init_transport()));
        s.initialize().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn move_absolute() {
        // 100 µm / 0.15625 = 640, 200 µm / 0.15625 = 1280
        let t = make_init_transport()
            .expect("/1 2 move abs 640\n",  "@01 02 IDLE -- 640")
            .expect("/1 1 move abs 1280\n", "@01 01 IDLE -- 1280");
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
            .expect("/1 2 move rel 320\n",  "@01 02 IDLE -- 320")
            .expect("/1 1 move rel -320\n", "@01 01 IDLE -- -320");
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
            .expect("/1 0 home\n", "@01 00 IDLE -- OK");
        let mut s = ZaberXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn no_transport_error() { assert!(ZaberXYStage::new().initialize().is_err()); }
}
