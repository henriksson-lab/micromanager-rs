/// Conix Research XYZ controller Z stage.
///
/// Protocol (TX `\r`, RX `\r`):
///   `W Z\r`       → `:A <z>`  (current position in µm)
///   `M Z<z>\r`    → `:A`      (move to absolute position in µm)
///   `H\r`         → `:A`      (set origin)
///   `\\r`         → `:A`      (halt; backslash + CR)
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Stage};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, FocusDirection, PropertyValue};

fn check_a(resp: &str) -> MmResult<&str> {
    let s = resp.trim();
    if let Some(rest) = s.strip_prefix(":A") { Ok(rest.trim()) }
    else { Err(MmError::LocallyDefined(format!("Conix error: {}", s))) }
}

pub struct ConixZStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    pos_um: f64,
}

impl ConixZStage {
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
}

impl Default for ConixZStage { fn default() -> Self { Self::new() } }

impl Device for ConixZStage {
    fn name(&self) -> &str { "ConixZStage" }
    fn description(&self) -> &str { "Conix Research XYZ controller Z stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let r = self.cmd("W Z")?;
        let body = check_a(&r)?;
        self.pos_um = body.split_whitespace().next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
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

impl Stage for ConixZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let r = self.cmd(&format!("M Z{}", z))?;
        check_a(&r)?;
        self.pos_um = z;
        Ok(())
    }
    fn get_position_um(&self) -> MmResult<f64> { Ok(self.pos_um) }
    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        self.set_position_um(self.pos_um + dz)
    }
    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("H")?;
        check_a(&r)?;
        self.pos_um = 0.0;
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> { let _ = self.cmd("\\"); Ok(()) }
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
        let t = MockTransport::new().any(":A 150.5");
        let mut s = ConixZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 150.5).abs() < 1e-6);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new().any(":A 0").any(":A");
        let mut s = ConixZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(500.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 500.0);
    }

    #[test]
    fn move_relative() {
        let t = MockTransport::new().any(":A 100").any(":A");
        let mut s = ConixZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(50.0).unwrap();
        assert!((s.get_position_um().unwrap() - 150.0).abs() < 1e-6);
    }

    #[test]
    fn no_transport_error() { assert!(ConixZStage::new().initialize().is_err()); }
}
