/// ChuoSeiki MD-5000 XY stage controller.
///
/// Protocol (CRLF-terminated):
///   `DLM C\r\n`       → (no meaningful response — clear error)
///   `RVR\r\n`         → "RVR <firmware>\r\n" (version check)
///   `ABA X <steps>\r\n`→ "... 00" (last 2 chars = error code; "00" = OK)
///   `ABA Y <steps>\r\n`→ same
///   `RLP\r\n`         → "RLP X <steps>,Y <steps>\r\n"
///
/// Step size: 1 µm/step (assumed; MD-5000 default resolution).
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, XYStage};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

pub struct ChuoSeikiXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl ChuoSeikiXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("FirmwareVersion", PropertyValue::String(String::new()), true).unwrap();
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
        let c = format!("{}\r\n", command);
        self.call_transport(|t| { let r = t.send_recv(&c)?; Ok(r.trim().to_string()) })
    }

    /// Check that the last 2 chars of response are "00" (no error).
    fn check_ok(resp: &str) -> MmResult<()> {
        let s = resp.trim();
        if s.len() >= 2 && &s[s.len() - 2..] == "00" {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("ChuoSeiki error: {}", s)))
        }
    }

    /// Parse "RLP X <steps>,Y <steps>" → (x_um, y_um)
    fn parse_rlp(resp: &str) -> MmResult<(f64, f64)> {
        let s = resp.trim();
        // Strip "RLP " prefix
        let s = if s.starts_with("RLP ") { &s[4..] } else { s };
        // Split on ","
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() < 2 {
            return Err(MmError::LocallyDefined(format!("Cannot parse RLP: {}", resp)));
        }
        // Each part is "X <steps>" or "Y <steps>"
        let x_steps: i64 = parts[0].split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
        let y_steps: i64 = parts[1].split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
        Ok((x_steps as f64, y_steps as f64))
    }

    fn move_axis(&mut self, axis: char, steps: i64) -> MmResult<()> {
        let r = self.cmd(&format!("ABA {} {}", axis, steps))?;
        Self::check_ok(&r)
    }
}

impl Default for ChuoSeikiXYStage { fn default() -> Self { Self::new() } }

impl Device for ChuoSeikiXYStage {
    fn name(&self) -> &str { "ChuoSeikiXYStage" }
    fn description(&self) -> &str { "ChuoSeiki MD-5000 XY stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let _ = self.cmd("DLM C");
        let ver = self.cmd("RVR")?;
        if !ver.contains("RVR") {
            return Err(MmError::LocallyDefined(format!("Unexpected RVR response: {}", ver)));
        }
        self.props.entry_mut("FirmwareVersion").map(|e| e.value = PropertyValue::String(ver));
        let pos_resp = self.cmd("RLP")?;
        let (x, y) = Self::parse_rlp(&pos_resp)?;
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

impl XYStage for ChuoSeikiXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        self.move_axis('X', x.round() as i64)?;
        self.move_axis('Y', y.round() as i64)?;
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
        self.move_axis('X', 0)?;
        self.move_axis('Y', 0)?;
        self.x_um = 0.0; self.y_um = 0.0;
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> { Ok(()) }
    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((-100_000.0, 100_000.0, -100_000.0, 100_000.0))
    }
    fn get_step_size_um(&self) -> (f64, f64) { (1.0, 1.0) }
    fn set_origin(&mut self) -> MmResult<()> { self.x_um = 0.0; self.y_um = 0.0; Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any("OK")                       // DLM C
            .any("RVR MD5000 v1.2")          // RVR
            .any("RLP X 100,Y 200")          // RLP
    }

    #[test]
    fn initialize() {
        let mut s = ChuoSeikiXYStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (100.0, 200.0));
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any("ABA X 30000").any("ABA Y 40000");
        let mut s = ChuoSeikiXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        // Responses need to end with "00"
        s.set_xy_position_um(300.0, 400.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (300.0, 400.0));
    }

    #[test]
    fn parse_rlp_ok() {
        let (x, y) = ChuoSeikiXYStage::parse_rlp("RLP X 1000,Y -500").unwrap();
        assert_eq!(x, 1000.0);
        assert_eq!(y, -500.0);
    }

    #[test]
    fn check_ok_passes() {
        assert!(ChuoSeikiXYStage::check_ok("ABA X 100 00").is_ok());
        assert!(ChuoSeikiXYStage::check_ok("ABA X 100 01").is_err());
    }

    #[test]
    fn no_transport_error() { assert!(ChuoSeikiXYStage::new().initialize().is_err()); }
}
