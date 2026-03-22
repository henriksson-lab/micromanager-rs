/// Prior H128 legacy Z stage.
///
/// Protocol (CR terminated):
///   `PZ\r`         → `<steps>\r`   (query Z position)
///   `C,<n>\r`      → (set step count, no ack needed before U/D)
///   `U\r`          → `R\r`   (move up n steps)
///   `D\r`          → `R\r`   (move down n steps)
///   `I\r`          → `R\r`   (stop)
///
/// Step size: 0.1 µm/step.
/// Absolute positioning is implemented as delta = target − current, then C,<delta> + U or D.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

const STEP_SIZE_UM: f64 = 0.1;

pub struct PriorLegacyZStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    cur_steps: i64,
}

impl PriorLegacyZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, transport: None, initialized: false, cur_steps: 0 }
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
            Err(MmError::LocallyDefined(format!("Prior H128 Z error: {}", resp)))
        } else {
            Err(MmError::LocallyDefined(format!("Prior H128 Z unexpected: {}", resp)))
        }
    }

    fn query_steps(&mut self) -> MmResult<i64> {
        let resp = self.cmd("PZ")?;
        if resp.starts_with('E') && resp.len() > 2 {
            return Err(MmError::LocallyDefined(format!("Prior H128 Z error: {}", resp)));
        }
        Ok(resp.trim().parse().unwrap_or(0))
    }

    /// Move delta steps: positive = up, negative = down.
    fn move_steps(&mut self, delta: i64) -> MmResult<()> {
        let abs_delta = delta.unsigned_abs();
        // Set step count
        let _ = self.cmd(&format!("C,{}", abs_delta));
        // Move direction
        let dir_cmd = if delta >= 0 { "U" } else { "D" };
        let resp = self.cmd(dir_cmd)?;
        Self::check_ack(&resp)?;
        self.cur_steps += delta;
        Ok(())
    }
}

impl Default for PriorLegacyZStage { fn default() -> Self { Self::new() } }

impl Device for PriorLegacyZStage {
    fn name(&self) -> &str { "PriorLegacy-ZStage" }
    fn description(&self) -> &str { "Prior H128 legacy Z stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        self.cur_steps = self.query_steps()?;
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

impl Stage for PriorLegacyZStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let target = (pos / STEP_SIZE_UM + 0.5) as i64;
        let delta = target - self.cur_steps;
        self.move_steps(delta)
    }

    fn get_position_um(&self) -> MmResult<f64> {
        Ok(self.cur_steps as f64 * STEP_SIZE_UM)
    }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let delta = (dz / STEP_SIZE_UM + 0.5) as i64;
        self.move_steps(delta)
    }

    fn home(&mut self) -> MmResult<()> {
        Err(MmError::LocallyDefined("Prior H128: homing not supported".into()))
    }

    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd("I");
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((-10_000.0, 10_000.0)) }
    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize() {
        let t = MockTransport::new().any("1000"); // 1000 steps * 0.1 µm = 100 µm
        let mut s = PriorLegacyZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute_up() {
        // start at step 0, move to 100 µm = 1000 steps up
        let t = MockTransport::new().any("0").any("OK").any("R");
        let mut s = PriorLegacyZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
        assert!((s.get_position_um().unwrap() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute_down() {
        // start at 1000 steps (100 µm), move to 0
        let t = MockTransport::new().any("1000").any("OK").any("R");
        let mut s = PriorLegacyZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(0.0).unwrap();
        assert!((s.get_position_um().unwrap()).abs() < 1e-9);
    }

    #[test]
    fn move_relative() {
        let t = MockTransport::new().any("0").any("OK").any("R");
        let mut s = PriorLegacyZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(50.0).unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn no_transport_error() { assert!(PriorLegacyZStage::new().initialize().is_err()); }
}
