/// ChuoSeiki QT 2-axis XY stage.
///
/// Protocol (CR+LF terminated):
///   `?:CHUOSEIKI\r\n`           → "CHUOSEIKI\r\n"  (identity check)
///   `AGO:A<x>B<y>\r\n`          → OK or `!<n>` error
///   `MGO:A<dx>B<dy>\r\n`        → OK or `!<n>` error (relative move)
///   `Q:A0B0\r\n`                → `<+/->XXXXXXXXD,<+/->XXXXXXXXD\r\n`
///                                  (positions + state: D=moving, K=stopped, H=homing)
///   `H:AB\r\n`                  → OK or `!<n>` (home both axes)
///
/// Step size default: 1 µm/step.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, XYStage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const DEFAULT_STEP_UM: f64 = 1.0;

pub struct ChuoSeikiQTXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
    step_um: f64,
}

impl ChuoSeikiQTXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            x_um: 0.0,
            y_um: 0.0,
            step_um: DEFAULT_STEP_UM,
        }
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
        let c = format!("{}\r\n", command);
        self.call_transport(|t| { let r = t.send_recv(&c)?; Ok(r.trim().to_string()) })
    }

    /// Parse a controller error response; `!n` means error, anything else is ok.
    fn check_response(resp: &str) -> MmResult<()> {
        if resp.starts_with('!') {
            Err(MmError::LocallyDefined(format!("ChuoSeiki QT error: {}", resp)))
        } else {
            Ok(())
        }
    }

    /// Read position from `Q:A0B0` response.
    /// Response format: `+00001234K,+00001234K` (9 char pos + state letter)
    fn read_xy(&mut self) -> MmResult<(f64, f64)> {
        let resp = self.cmd("Q:A0B0")?;
        Self::parse_position_response(&resp, self.step_um)
    }

    fn parse_position_response(resp: &str, step_um: f64) -> MmResult<(f64, f64)> {
        // Expect at least 21 chars: 9+1 + ',' + 9+1
        if resp.len() < 21 {
            return Err(MmError::LocallyDefined(format!(
                "ChuoSeiki QT: unexpected position response: {}", resp
            )));
        }
        let x_steps: i64 = resp[..9].trim().parse().unwrap_or(0);
        let y_steps: i64 = resp[11..20].trim().parse().unwrap_or(0);
        Ok((x_steps as f64 * step_um, y_steps as f64 * step_um))
    }
}

impl Default for ChuoSeikiQTXYStage { fn default() -> Self { Self::new() } }

impl Device for ChuoSeikiQTXYStage {
    fn name(&self) -> &str { "ChuoSeikiQT-XYStage" }
    fn description(&self) -> &str { "ChuoSeiki QT 2-axis XY stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Identity check
        let resp = self.cmd("?:CHUOSEIKI")?;
        if !resp.starts_with("CHUOSEIKI") {
            return Err(MmError::LocallyDefined(format!("ChuoSeiki QT: unexpected identity: {}", resp)));
        }
        // Enable feedback after control commands
        let _ = self.cmd("X:1");
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

impl XYStage for ChuoSeikiQTXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let xs = (x / self.step_um).round() as i64;
        let ys = (y / self.step_um).round() as i64;
        let r = self.cmd(&format!("AGO:A{}B{}", xs, ys))?;
        Self::check_response(&r)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> { Ok((self.x_um, self.y_um)) }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let dxs = (dx / self.step_um).round() as i64;
        let dys = (dy / self.step_um).round() as i64;
        let r = self.cmd(&format!("MGO:A{}B{}", dxs, dys))?;
        Self::check_response(&r)?;
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("H:AB")?;
        Self::check_response(&r)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd("L:AB");
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((-100_000.0, 100_000.0, -100_000.0, 100_000.0))
    }

    fn get_step_size_um(&self) -> (f64, f64) { (self.step_um, self.step_um) }

    fn set_origin(&mut self) -> MmResult<()> {
        // QT controller does not have a set-origin command; reset cached values
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
            .any("CHUOSEIKI")      // identity
            .any("OK")             // X:1
            .any("+00000100K,+00000200K") // Q:A0B0 → 100, 200 steps * 1µm = 100, 200 µm
    }

    #[test]
    fn initialize() {
        let mut s = ChuoSeikiQTXYStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 100.0).abs() < 1e-9);
        assert!((y - 200.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any("OK");
        let mut s = ChuoSeikiQTXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(500.0, 300.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (500.0, 300.0));
    }

    #[test]
    fn move_relative() {
        let t = make_transport().any("OK");
        let mut s = ChuoSeikiQTXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(50.0, 25.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 150.0).abs() < 1e-9);
        assert!((y - 225.0).abs() < 1e-9);
    }

    #[test]
    fn error_response_fails() {
        let t = make_transport().any("!6"); // limit detected
        let mut s = ChuoSeikiQTXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_xy_position_um(999_999.0, 0.0).is_err());
    }

    #[test]
    fn no_transport_error() { assert!(ChuoSeikiQTXYStage::new().initialize().is_err()); }

    #[test]
    fn bad_identity_fails() {
        let t = MockTransport::new().any("UNKNOWN");
        let mut s = ChuoSeikiQTXYStage::new().with_transport(Box::new(t));
        assert!(s.initialize().is_err());
    }

    #[test]
    fn step_size() {
        let s = ChuoSeikiQTXYStage::new();
        assert_eq!(s.get_step_size_um(), (1.0, 1.0));
    }
}
