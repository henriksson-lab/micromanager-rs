/// Scientifica SliceScope / MotionSite Z stage.
///
/// Protocol (same as XY, newline-terminated):
///   `absz <steps>\r`  → "A\r\n" (success)
///   `PZ\r`            → "<integer steps>"
///   `home\r`          → "A\r\n"
///   `stop\r`          → "A\r\n"
///
/// Step size: 0.1 µm / step.
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Stage};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, FocusDirection, PropertyValue};

const STEPS_PER_UM: f64 = 10.0;

pub struct ScientificaZStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    pos_um: f64,
}

impl ScientificaZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, transport: None, initialized: false, pos_um: 0.0 }
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
}

impl Default for ScientificaZStage { fn default() -> Self { Self::new() } }

impl Device for ScientificaZStage {
    fn name(&self) -> &str { "ScientificaZStage" }
    fn description(&self) -> &str { "Scientifica Z stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let r = self.cmd("PZ")?;
        let steps: i64 = r.trim().parse().unwrap_or(0);
        self.pos_um = steps as f64 / STEPS_PER_UM;
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

impl Stage for ScientificaZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = (z * STEPS_PER_UM).round() as i64;
        let r = self.cmd(&format!("absz {}", steps))?;
        Self::check_ok(&r)?;
        self.pos_um = z;
        Ok(())
    }
    fn get_position_um(&self) -> MmResult<f64> { Ok(self.pos_um) }
    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let new_z = self.pos_um + dz;
        self.set_position_um(new_z)
    }
    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("home")?;
        Self::check_ok(&r)?;
        self.pos_um = 0.0;
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> { let _ = self.cmd("stop"); Ok(()) }
    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((-10_000.0, 10_000.0)) }
    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    #[test]
    fn initialize() {
        let t = MockTransport::new().any("500"); // PZ → 50 µm
        let mut s = ScientificaZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new().any("0").any("A");
        let mut s = ScientificaZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 100.0);
    }

    #[test]
    fn move_relative() {
        let t = MockTransport::new().any("1000").any("A"); // start 100µm, add 50µm
        let mut s = ScientificaZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(50.0).unwrap();
        assert!((s.get_position_um().unwrap() - 150.0).abs() < 1e-9);
    }

    #[test]
    fn error_response_fails() {
        let t = MockTransport::new().any("0").any("E: limit");
        let mut s = ScientificaZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_position_um(999_999.0).is_err());
    }

    #[test]
    fn no_transport_error() { assert!(ScientificaZStage::new().initialize().is_err()); }
}
