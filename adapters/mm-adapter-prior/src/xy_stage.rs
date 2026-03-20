/// Prior Scientific ProScan XY stage.
///
/// Protocol (TX `\r`, RX `\r`):
///   `DATE\r`       → firmware/date string (version check)
///   `G,x,y\r`      → absolute move (steps); response `R\r` or `E<code>\r`
///   `GR,dx,dy\r`   → relative move (steps); same response
///   `PX\r`         → X position in steps
///   `PY\r`         → Y position in steps
///   `SIS\r`        → home (Set Initial Stage position)
///   `K\r`          → halt
///   `$\r`          → status byte (bit 0 = X busy, bit 1 = Y busy)
///
/// Step size: 0.1 µm / step (10 steps per µm).
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, XYStage};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

const STEPS_PER_UM: f64 = 10.0;

pub struct PriorXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl PriorXYStage {
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

    fn check_r(resp: &str) -> MmResult<()> {
        let s = resp.trim();
        if s == "R" { Ok(()) }
        else { Err(MmError::LocallyDefined(format!("Prior error: {}", s))) }
    }

    fn read_xy(&mut self) -> MmResult<(f64, f64)> {
        let rx = self.cmd("PX")?;
        let ry = self.cmd("PY")?;
        let xs: i64 = rx.trim().parse().unwrap_or(0);
        let ys: i64 = ry.trim().parse().unwrap_or(0);
        Ok((xs as f64 / STEPS_PER_UM, ys as f64 / STEPS_PER_UM))
    }
}

impl Default for PriorXYStage { fn default() -> Self { Self::new() } }

impl Device for PriorXYStage {
    fn name(&self) -> &str { "PriorXYStage" }
    fn description(&self) -> &str { "Prior Scientific ProScan XY stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let ver = self.cmd("DATE")?;
        self.props.entry_mut("Version").map(|e| e.value = PropertyValue::String(ver));
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

impl XYStage for PriorXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let xs = (x * STEPS_PER_UM).round() as i64;
        let ys = (y * STEPS_PER_UM).round() as i64;
        let r = self.cmd(&format!("G,{},{}", xs, ys))?;
        Self::check_r(&r)?;
        self.x_um = x; self.y_um = y;
        Ok(())
    }
    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> { Ok((self.x_um, self.y_um)) }
    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let dxs = (dx * STEPS_PER_UM).round() as i64;
        let dys = (dy * STEPS_PER_UM).round() as i64;
        let r = self.cmd(&format!("GR,{},{}", dxs, dys))?;
        Self::check_r(&r)?;
        self.x_um += dx; self.y_um += dy;
        Ok(())
    }
    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("SIS")?;
        Self::check_r(&r)?;
        self.x_um = 0.0; self.y_um = 0.0;
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> { let _ = self.cmd("K"); Ok(()) }
    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> { Ok((-100_000.0, 100_000.0, -100_000.0, 100_000.0)) }
    fn get_step_size_um(&self) -> (f64, f64) { (0.1, 0.1) }
    fn set_origin(&mut self) -> MmResult<()> { self.x_um = 0.0; self.y_um = 0.0; Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any("Prior ProScan v3.01")  // DATE
            .any("1000")                  // PX → 100 µm
            .any("2000")                  // PY → 200 µm
    }

    #[test]
    fn initialize() {
        let mut s = PriorXYStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 100.0).abs() < 1e-9);
        assert!((y - 200.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any("R");
        let mut s = PriorXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(500.0, 600.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (500.0, 600.0));
    }

    #[test]
    fn move_relative() {
        let t = make_transport().any("R");
        let mut s = PriorXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(50.0, 75.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 150.0).abs() < 1e-9);
        assert!((y - 275.0).abs() < 1e-9);
    }

    #[test]
    fn error_response_fails() {
        let t = make_transport().any("E8");
        let mut s = PriorXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_xy_position_um(9999.0, 0.0).is_err());
    }

    #[test]
    fn home() {
        let t = make_transport().any("R");
        let mut s = PriorXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn no_transport_error() { assert!(PriorXYStage::new().initialize().is_err()); }
}
