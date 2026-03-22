/// Ludl MAC5000/MAC6000 Z stage.
///
/// Protocol (TX `\r`, RX `\n`):
///   `MOVE Z=<n>\r` → `:A`
///   `WHERE Z\r`    → `:A <z>`
///   `HOME Z\r`     → `:A`
///   `HALT\r`       → `:A`
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

const STEPS_PER_UM: f64 = 10.0;

pub struct LudlZStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    pos_um: f64,
}

impl LudlZStage {
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

    fn check_a(resp: &str) -> MmResult<&str> {
        let s = resp.trim();
        if let Some(rest) = s.strip_prefix(":A") { Ok(rest.trim()) }
        else { Err(MmError::LocallyDefined(format!("Ludl error: {}", s))) }
    }
}

impl Default for LudlZStage { fn default() -> Self { Self::new() } }

impl Device for LudlZStage {
    fn name(&self) -> &str { "LudlZStage" }
    fn description(&self) -> &str { "Ludl MAC5000/MAC6000 Z stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let r = self.cmd("WHERE Z")?;
        let body = Self::check_a(&r)?;
        let steps: i64 = body.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
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

impl Stage for LudlZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = (z * STEPS_PER_UM).round() as i64;
        let r = self.cmd(&format!("MOVE Z={}", steps))?;
        Self::check_a(&r)?;
        self.pos_um = z;
        Ok(())
    }
    fn get_position_um(&self) -> MmResult<f64> { Ok(self.pos_um) }
    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        self.set_position_um(self.pos_um + dz)
    }
    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("HOME Z")?;
        Self::check_a(&r)?;
        self.pos_um = 0.0;
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> { let _ = self.cmd("HALT"); Ok(()) }
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
        let t = MockTransport::new().any(":A 500"); // 50 µm
        let mut s = LudlZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new().any(":A 0").any(":A");
        let mut s = LudlZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(200.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 200.0);
    }

    #[test]
    fn no_transport_error() { assert!(LudlZStage::new().initialize().is_err()); }
}
