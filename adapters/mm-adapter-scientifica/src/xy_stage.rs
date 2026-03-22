/// Scientifica SliceScope / MotionSite XY stage.
///
/// Protocol (newline-terminated):
///   `abs <x> <y>\r` → "A\r\n" (success) or "E...\r\n" (error)
///   `rel <dx> <dy>\r`→ same echo
///   `PX\r`           → "<integer steps>" (X position)
///   `PY\r`           → "<integer steps>" (Y position)
///   `home\r`         → "A\r\n"
///   `stop\r`         → "A\r\n"
///   `LimX\r`         → "<min> <max>"
///   `LimY\r`         → "<min> <max>"
///
/// Step size: 0.1 µm per step (V1 firmware).
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, XYStage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const STEPS_PER_UM: f64 = 10.0;   // 0.1 µm / step → 10 steps / µm

pub struct ScientificaXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl ScientificaXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, transport: None, initialized: false, x_um: 0.0, y_um: 0.0 }
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
        let c = format!("{}\r", command);
        self.call_transport(|t| { let r = t.send_recv(&c)?; Ok(r.trim().to_string()) })
    }

    fn check_ok(resp: &str) -> MmResult<()> {
        if resp.starts_with('A') { Ok(()) }
        else { Err(MmError::LocallyDefined(format!("Scientifica error: {}", resp))) }
    }

    fn read_xy(&mut self) -> MmResult<(f64, f64)> {
        let rx = self.cmd("PX")?;
        let ry = self.cmd("PY")?;
        let x_steps: i64 = rx.trim().parse().unwrap_or(0);
        let y_steps: i64 = ry.trim().parse().unwrap_or(0);
        Ok((x_steps as f64 / STEPS_PER_UM, y_steps as f64 / STEPS_PER_UM))
    }
}

impl Default for ScientificaXYStage { fn default() -> Self { Self::new() } }

impl Device for ScientificaXYStage {
    fn name(&self) -> &str { "ScientificaXYStage" }
    fn description(&self) -> &str { "Scientifica XY stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let (x, y) = self.read_xy()?;
        self.x_um = x;
        self.y_um = y;
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

impl XYStage for ScientificaXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let xs = (x * STEPS_PER_UM).round() as i64;
        let ys = (y * STEPS_PER_UM).round() as i64;
        let r = self.cmd(&format!("abs {} {}", xs, ys))?;
        Self::check_ok(&r)?;
        self.x_um = x; self.y_um = y;
        Ok(())
    }
    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> { Ok((self.x_um, self.y_um)) }
    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let dxs = (dx * STEPS_PER_UM).round() as i64;
        let dys = (dy * STEPS_PER_UM).round() as i64;
        let r = self.cmd(&format!("rel {} {}", dxs, dys))?;
        Self::check_ok(&r)?;
        self.x_um += dx; self.y_um += dy;
        Ok(())
    }
    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("home")?;
        Self::check_ok(&r)?;
        self.x_um = 0.0; self.y_um = 0.0;
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> { let _ = self.cmd("stop"); Ok(()) }
    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((-50_000.0, 50_000.0, -50_000.0, 50_000.0))
    }
    fn get_step_size_um(&self) -> (f64, f64) { (0.1, 0.1) }
    fn set_origin(&mut self) -> MmResult<()> { self.x_um = 0.0; self.y_um = 0.0; Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any("1000")   // PX → 100 µm
            .any("2000")   // PY → 200 µm
    }

    #[test]
    fn initialize() {
        let mut s = ScientificaXYStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 100.0).abs() < 1e-9);
        assert!((y - 200.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any("A");
        let mut s = ScientificaXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(300.0, 400.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (300.0, 400.0));
    }

    #[test]
    fn move_relative() {
        let t = make_transport().any("A");
        let mut s = ScientificaXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(50.0, 25.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 150.0).abs() < 1e-9);
        assert!((y - 225.0).abs() < 1e-9);
    }

    #[test]
    fn error_response_fails() {
        let t = make_transport().any("E: position limit");
        let mut s = ScientificaXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_xy_position_um(999_999.0, 0.0).is_err());
    }

    #[test]
    fn no_transport_error() { assert!(ScientificaXYStage::new().initialize().is_err()); }
}
