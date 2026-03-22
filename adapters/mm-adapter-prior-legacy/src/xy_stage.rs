/// Prior H128 legacy XY stage.
///
/// Protocol (CR terminated, H128 legacy firmware):
///   `G,<x>,<y>\r`  → `R\r`  (move absolute; R = acknowledged)
///   `PX\r`         → `<steps>\r`  (query X position)
///   `PY\r`         → `<steps>\r`  (query Y position)
///   `I\r`          → `R\r`  (stop)
///   `Z\r`          → `0\r`  (set origin / zero all axes)
///   `SMX,<n>\r`    → `0\r`  (set max speed 1..100)
///   `SMX\r`        → `<n>\r` (get max speed)
///
/// Step size: 0.1 µm/step.
/// Home is not supported on H128 (returns unsupported).
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, XYStage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const STEP_SIZE_UM: f64 = 0.1;

pub struct PriorLegacyXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl PriorLegacyXYStage {
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

    fn check_ack(resp: &str) -> MmResult<()> {
        if resp.starts_with('R') {
            Ok(())
        } else if resp.starts_with('E') && resp.len() > 2 {
            Err(MmError::LocallyDefined(format!("Prior H128 error: {}", resp)))
        } else {
            Err(MmError::LocallyDefined(format!("Prior H128 unexpected response: {}", resp)))
        }
    }

    fn query_axis_steps(&mut self, axis: char) -> MmResult<i64> {
        let resp = self.cmd(&format!("P{}", axis))?;
        if resp.starts_with('E') && resp.len() > 2 {
            return Err(MmError::LocallyDefined(format!("Prior H128 error: {}", resp)));
        }
        Ok(resp.trim().parse().unwrap_or(0))
    }
}

impl Default for PriorLegacyXYStage { fn default() -> Self { Self::new() } }

impl Device for PriorLegacyXYStage {
    fn name(&self) -> &str { "PriorLegacy-XYStage" }
    fn description(&self) -> &str { "Prior H128 legacy XY stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let x_steps = self.query_axis_steps('X')?;
        let y_steps = self.query_axis_steps('Y')?;
        self.x_um = x_steps as f64 * STEP_SIZE_UM;
        self.y_um = y_steps as f64 * STEP_SIZE_UM;
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

impl XYStage for PriorLegacyXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let xs = (x / STEP_SIZE_UM).round() as i64;
        let ys = (y / STEP_SIZE_UM).round() as i64;
        let resp = self.cmd(&format!("G,{},{}", xs, ys))?;
        Self::check_ack(&resp)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> { Ok((self.x_um, self.y_um)) }

    fn set_relative_xy_position_um(&mut self, _dx: f64, _dy: f64) -> MmResult<()> {
        // H128 does not have a relative move command
        Err(MmError::LocallyDefined("Prior H128: relative move not supported".into()))
    }

    fn home(&mut self) -> MmResult<()> {
        // H128 does not support homing
        Err(MmError::LocallyDefined("Prior H128: homing not supported".into()))
    }

    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd("I");
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((-100_000.0, 100_000.0, -100_000.0, 100_000.0))
    }

    fn get_step_size_um(&self) -> (f64, f64) { (STEP_SIZE_UM, STEP_SIZE_UM) }

    fn set_origin(&mut self) -> MmResult<()> {
        let resp = self.cmd("Z")?;
        if !resp.starts_with('0') && !resp.starts_with('R') {
            return Err(MmError::LocallyDefined(format!("Prior H128 set-origin error: {}", resp)));
        }
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }
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
        let mut s = PriorLegacyXYStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 100.0).abs() < 1e-9);
        assert!((y - 200.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any("R");
        let mut s = PriorLegacyXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(300.0, 400.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (300.0, 400.0));
    }

    #[test]
    fn error_response_fails() {
        let t = make_transport().any("E12");
        let mut s = PriorLegacyXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_xy_position_um(0.0, 0.0).is_err());
    }

    #[test]
    fn set_origin() {
        let t = make_transport().any("0");
        let mut s = PriorLegacyXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_origin().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn relative_move_unsupported() {
        let t = make_transport();
        let mut s = PriorLegacyXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_relative_xy_position_um(10.0, 10.0).is_err());
    }

    #[test]
    fn no_transport_error() { assert!(PriorLegacyXYStage::new().initialize().is_err()); }
}
