/// Ludl Electronic Products MAC5000/MAC6000 XY stage.
///
/// Protocol (TX `\r`, RX `\n`):
///   `VER\r`            → `:A <version>` or `:N <code>`
///   `MOVE X=<n> Y=<n>\r` → `:A` (steps, 0.1 µm resolution)
///   `WHERE X Y\r`      → `:A <x> <y>`
///   `HOME X Y\r`       → `:A`
///   `HALT\r`           → `:A`
///   `HERE X=0 Y=0\r`   → `:A`  (set origin)
///
/// Step size: 0.1 µm / step.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, XYStage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const STEPS_PER_UM: f64 = 10.0;

pub struct LudlXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl LudlXYStage {
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

    /// Check `:A ...` response; strip prefix and return remainder.
    fn check_a(resp: &str) -> MmResult<&str> {
        let s = resp.trim();
        if let Some(rest) = s.strip_prefix(":A") {
            Ok(rest.trim())
        } else {
            Err(MmError::LocallyDefined(format!("Ludl error: {}", s)))
        }
    }

    /// Parse `:A <x> <y>` → (x_um, y_um)
    fn parse_xy(resp: &str) -> MmResult<(f64, f64)> {
        let body = Self::check_a(resp)?;
        let parts: Vec<&str> = body.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(MmError::LocallyDefined(format!("Cannot parse WHERE response: {}", resp)));
        }
        let x: i64 = parts[0].parse().unwrap_or(0);
        let y: i64 = parts[1].parse().unwrap_or(0);
        Ok((x as f64 / STEPS_PER_UM, y as f64 / STEPS_PER_UM))
    }
}

impl Default for LudlXYStage { fn default() -> Self { Self::new() } }

impl Device for LudlXYStage {
    fn name(&self) -> &str { "LudlXYStage" }
    fn description(&self) -> &str { "Ludl MAC5000/MAC6000 XY stage" }

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

impl XYStage for LudlXYStage {
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
        let new_x = self.x_um + dx;
        let new_y = self.y_um + dy;
        self.set_xy_position_um(new_x, new_y)
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
    use micromanager::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any(":A MAC6000 v5.0")    // VER
            .any(":A 1000 2000")       // WHERE X Y → 100 µm, 200 µm
    }

    #[test]
    fn initialize() {
        let mut s = LudlXYStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 100.0).abs() < 1e-9);
        assert!((y - 200.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any(":A");
        let mut s = LudlXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(300.0, 400.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (300.0, 400.0));
    }

    #[test]
    fn error_response_fails() {
        let t = make_transport().any(":N 21");
        let mut s = LudlXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_xy_position_um(99999.0, 0.0).is_err());
    }

    #[test]
    fn parse_xy_ok() {
        let (x, y) = LudlXYStage::parse_xy(":A 1000 -500").unwrap();
        assert!((x - 100.0).abs() < 1e-9);
        assert!((y - (-50.0)).abs() < 1e-9);
    }

    #[test]
    fn no_transport_error() { assert!(LudlXYStage::new().initialize().is_err()); }
}
