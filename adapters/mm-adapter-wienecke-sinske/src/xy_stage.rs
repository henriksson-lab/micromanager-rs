/// Wienecke & Sinske WSB PiezoDrive CAN XY stage.
///
/// The hardware uses a CAN29 bus protocol internally.  For serial port communication
/// the controller accepts ASCII commands.  Key commands (CR terminated):
///
///   `POS X\r`            → "<x_nm>\r\n"
///   `POS Y\r`            → "<y_nm>\r\n"
///   `MOVE X <nm>\r`      → "OK\r\n" or "ERR <msg>"
///   `MOVE Y <nm>\r`      → "OK\r\n" or "ERR <msg>"
///   `RMOVE X <dnm>\r`    → "OK\r\n" or "ERR <msg>"
///   `RMOVE Y <dnm>\r`    → "OK\r\n" or "ERR <msg>"
///   `STOP\r`             → "OK\r\n"
///
/// Step size: 0.001 µm (1 nm).  Positions in nm on the wire.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, XYStage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const NM_PER_UM: f64 = 1000.0;

pub struct WSXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl WSXYStage {
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
        if resp.starts_with("ERR") {
            Err(MmError::LocallyDefined(format!("WS error: {}", resp)))
        } else {
            Ok(())
        }
    }

    fn query_axis(&mut self, axis: &str) -> MmResult<f64> {
        let resp = self.cmd(&format!("POS {}", axis))?;
        let nm: i64 = resp.trim().parse().unwrap_or(0);
        Ok(nm as f64 / NM_PER_UM)
    }
}

impl Default for WSXYStage { fn default() -> Self { Self::new() } }

impl Device for WSXYStage {
    fn name(&self) -> &str { "WS-XYStage" }
    fn description(&self) -> &str { "Wienecke & Sinske WSB PiezoDrive XY stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        self.x_um = self.query_axis("X")?;
        self.y_um = self.query_axis("Y")?;
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

impl XYStage for WSXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let xnm = (x * NM_PER_UM).round() as i64;
        let ynm = (y * NM_PER_UM).round() as i64;
        let rx = self.cmd(&format!("MOVE X {}", xnm))?;
        Self::check_ok(&rx)?;
        let ry = self.cmd(&format!("MOVE Y {}", ynm))?;
        Self::check_ok(&ry)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> { Ok((self.x_um, self.y_um)) }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let dxnm = (dx * NM_PER_UM).round() as i64;
        let dynm = (dy * NM_PER_UM).round() as i64;
        let rx = self.cmd(&format!("RMOVE X {}", dxnm))?;
        Self::check_ok(&rx)?;
        let ry = self.cmd(&format!("RMOVE Y {}", dynm))?;
        Self::check_ok(&ry)?;
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("HOME")?;
        Self::check_ok(&r)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd("STOP");
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((-200.0, 200.0, -200.0, 200.0))
    }

    fn get_step_size_um(&self) -> (f64, f64) { (0.001, 0.001) }

    fn set_origin(&mut self) -> MmResult<()> {
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
            .any("100000")  // POS X → 100 µm
            .any("200000")  // POS Y → 200 µm
    }

    #[test]
    fn initialize() {
        let mut s = WSXYStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 100.0).abs() < 1e-9);
        assert!((y - 200.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any("OK").any("OK");
        let mut s = WSXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(50.0, 75.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (50.0, 75.0));
    }

    #[test]
    fn move_relative() {
        let t = make_transport().any("OK").any("OK");
        let mut s = WSXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(10.0, 5.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 110.0).abs() < 1e-9);
        assert!((y - 205.0).abs() < 1e-9);
    }

    #[test]
    fn error_response_fails() {
        let t = make_transport().any("ERR: limit");
        let mut s = WSXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_xy_position_um(999_999.0, 0.0).is_err());
    }

    #[test]
    fn no_transport_error() { assert!(WSXYStage::new().initialize().is_err()); }
}
