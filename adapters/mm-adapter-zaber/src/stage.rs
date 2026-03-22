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
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

const DEFAULT_MOTOR_STEPS: f64 = 200.0;
const DEFAULT_LINEAR_MOTION_MM: f64 = 2.0;

pub struct ZaberStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    device_addr: u32,
    axis: u32,
    step_size_um: f64,
    limit_min_um: f64,
    limit_max_um: f64,
    position_um: f64,
}

impl ZaberStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            device_addr: 1,
            axis: 1,
            step_size_um: 0.15625,
            limit_min_um: 0.0,
            limit_max_um: 0.0,
            position_um: 0.0,
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

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let full = format!("/{} {} {}\n", self.device_addr, self.axis, command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    fn cmd_device(&mut self, command: &str) -> MmResult<String> {
        let full = format!("/{} 0 {}\n", self.device_addr, command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    /// Parse data field (5th token) from `@01 01 IDLE -- <data>`.
    fn parse_data(resp: &str) -> Option<&str> {
        resp.split_whitespace().nth(4)
    }

    fn get_setting_i64(&mut self, setting: &str) -> MmResult<i64> {
        let resp = self.cmd(&format!("get {}", setting))?;
        Self::parse_data(&resp)
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or_else(|| MmError::LocallyDefined(format!("bad response: {}", resp)))
    }
}

impl Default for ZaberStage { fn default() -> Self { Self::new() } }

impl Device for ZaberStage {
    fn name(&self) -> &str { "ZaberStage" }
    fn description(&self) -> &str { "Zaber linear stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let resolution = self.get_setting_i64("resolution")? as f64;
        self.step_size_um = DEFAULT_LINEAR_MOTION_MM / DEFAULT_MOTOR_STEPS / resolution * 1000.0;
        let min_steps = self.get_setting_i64("limit.min")?;
        let max_steps = self.get_setting_i64("limit.max")?;
        self.limit_min_um = min_steps as f64 * self.step_size_um;
        self.limit_max_um = max_steps as f64 * self.step_size_um;
        let pos_steps = self.get_setting_i64("pos")?;
        self.position_um = pos_steps as f64 * self.step_size_um;
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
    fn device_type(&self) -> DeviceType { DeviceType::Stage }
    fn busy(&self) -> bool { false }
}

impl Stage for ZaberStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let steps = (pos / self.step_size_um).round() as i64;
        self.cmd(&format!("move abs {}", steps))?;
        self.position_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.position_um) }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let steps = (d / self.step_size_um).round() as i64;
        self.cmd(&format!("move rel {}", steps))?;
        self.position_um += d;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        self.cmd("home")?;
        self.position_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        self.cmd_device("stop")?;
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((self.limit_min_um, self.limit_max_um)) }

    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect("/1 1 get resolution\n", "@01 01 IDLE -- 64")
            .expect("/1 1 get limit.min\n",  "@01 01 IDLE -- 0")
            .expect("/1 1 get limit.max\n",  "@01 01 IDLE -- 305175")
            .expect("/1 1 get pos\n",         "@01 01 IDLE -- 0")
    }

    #[test]
    fn initialize() {
        let mut s = ZaberStage::new().with_transport(Box::new(make_init_transport()));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap()).abs() < 0.001);
        let (lo, hi) = s.get_limits().unwrap();
        assert!(lo < hi);
    }

    #[test]
    fn move_absolute() {
        // 100 µm / 0.15625 µm/step = 640 steps
        let t = make_init_transport()
            .expect("/1 1 move abs 640\n", "@01 01 IDLE -- 640");
        let mut s = ZaberStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
        assert!((s.get_position_um().unwrap() - 100.0).abs() < 0.01);
    }

    #[test]
    fn move_relative() {
        // 50 µm / 0.15625 µm/step = 320 steps
        let t = make_init_transport()
            .expect("/1 1 move rel 320\n", "@01 01 IDLE -- 320");
        let mut s = ZaberStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(50.0).unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 0.01);
    }

    #[test]
    fn home() {
        let t = make_init_transport()
            .expect("/1 1 home\n", "@01 01 IDLE -- OK");
        let mut s = ZaberStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert!(s.get_position_um().unwrap().abs() < 0.001);
    }

    #[test]
    fn stop() {
        let t = make_init_transport()
            .expect("/1 0 stop\n", "@01 00 IDLE -- OK");
        let mut s = ZaberStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.stop().unwrap();
    }

    #[test]
    fn no_transport_error() { assert!(ZaberStage::new().initialize().is_err()); }
}
