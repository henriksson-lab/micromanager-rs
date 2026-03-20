/// Sutter Instruments MPC-200 XY stage.
///
/// Protocol identical to Ludl MAC series (TX `\r`, RX `\n`, `:A`/`:N` responses):
///   `VER\r`              → `:A <version>`
///   `MOVE X=<n> Y=<n>\r` → `:A`   (steps, 0.1 µm resolution)
///   `WHERE X Y\r`        → `:A <x> <y>`
///   `HOME X Y\r`         → `:A`
///   `HALT\r`             → `:A`
///   `HERE X=0 Y=0\r`    → `:A`
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, XYStage};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

const STEPS_PER_UM: f64 = 10.0;

pub struct SutterXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl SutterXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Version", PropertyValue::String(String::new()), true).unwrap();
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

    fn check_a(resp: &str) -> MmResult<&str> {
        let s = resp.trim();
        if let Some(rest) = s.strip_prefix(":A") { Ok(rest.trim()) }
        else { Err(MmError::LocallyDefined(format!("Sutter error: {}", s))) }
    }

    fn parse_xy(resp: &str) -> MmResult<(f64, f64)> {
        let body = Self::check_a(resp)?;
        let parts: Vec<&str> = body.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(MmError::LocallyDefined(format!("Cannot parse WHERE: {}", resp)));
        }
        let x: i64 = parts[0].parse().unwrap_or(0);
        let y: i64 = parts[1].parse().unwrap_or(0);
        Ok((x as f64 / STEPS_PER_UM, y as f64 / STEPS_PER_UM))
    }
}

impl Default for SutterXYStage { fn default() -> Self { Self::new() } }

impl Device for SutterXYStage {
    fn name(&self) -> &str { "SutterXYStage" }
    fn description(&self) -> &str { "Sutter Instruments MPC-200 XY stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let ver = self.cmd("VER")?;
        let ver_str = Self::check_a(&ver)?.to_string();
        self.props.entry_mut("Version").map(|e| e.value = PropertyValue::String(ver_str));
        let pos = self.cmd("WHERE X Y")?;
        let (x, y) = Self::parse_xy(&pos)?;
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

impl XYStage for SutterXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let xs = (x * STEPS_PER_UM).round() as i64;
        let ys = (y * STEPS_PER_UM).round() as i64;
        let r = self.cmd(&format!("MOVE X={} Y={}", xs, ys))?;
        Self::check_a(&r)?;
        self.x_um = x; self.y_um = y;
        Ok(())
    }
    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> { Ok((self.x_um, self.y_um)) }
    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        self.set_xy_position_um(self.x_um + dx, self.y_um + dy)
    }
    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("HOME X Y")?;
        Self::check_a(&r)?;
        self.x_um = 0.0; self.y_um = 0.0;
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> { let _ = self.cmd("HALT"); Ok(()) }
    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> { Ok((-100_000.0, 100_000.0, -100_000.0, 100_000.0)) }
    fn get_step_size_um(&self) -> (f64, f64) { (0.1, 0.1) }
    fn set_origin(&mut self) -> MmResult<()> {
        let _ = self.cmd("HERE X=0 Y=0");
        self.x_um = 0.0; self.y_um = 0.0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any(":A MPC200 v2.5")
            .any(":A 500 1000")  // 50 µm, 100 µm
    }

    #[test]
    fn initialize() {
        let mut s = SutterXYStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 50.0).abs() < 1e-9);
        assert!((y - 100.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any(":A");
        let mut s = SutterXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(200.0, 300.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (200.0, 300.0));
    }

    #[test]
    fn no_transport_error() { assert!(SutterXYStage::new().initialize().is_err()); }
}
