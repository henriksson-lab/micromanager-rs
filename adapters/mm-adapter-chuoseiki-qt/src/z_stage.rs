/// ChuoSeiki QT single-axis (Z) stage.
///
/// Protocol (CR+LF terminated):
///   `?:CHUOSEIKI\r\n`       → "CHUOSEIKI\r\n"
///   `AGO:A<z>\r\n`          → OK or `!<n>`
///   `MGO:A<dz>\r\n`         → OK or `!<n>`
///   `Q:A0\r\n`              → `<+/->XXXXXXXXD` (position + state)
///   `H:A\r\n`               → OK or `!<n>`
///
/// Step size default: 1 µm/step.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

const DEFAULT_STEP_UM: f64 = 1.0;

pub struct ChuoSeikiQTZStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    pos_um: f64,
    step_um: f64,
}

impl ChuoSeikiQTZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            pos_um: 0.0,
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

    fn check_response(resp: &str) -> MmResult<()> {
        if resp.starts_with('!') {
            Err(MmError::LocallyDefined(format!("ChuoSeiki QT error: {}", resp)))
        } else {
            Ok(())
        }
    }

    fn read_z(&mut self) -> MmResult<f64> {
        let resp = self.cmd("Q:A0")?;
        // Response is at least 9 chars: sign + 8 digits, followed by state letter
        if resp.len() < 9 {
            return Err(MmError::LocallyDefined(format!("ChuoSeiki QT: bad Z response: {}", resp)));
        }
        let steps: i64 = resp[..9].trim().parse().unwrap_or(0);
        Ok(steps as f64 * self.step_um)
    }
}

impl Default for ChuoSeikiQTZStage { fn default() -> Self { Self::new() } }

impl Device for ChuoSeikiQTZStage {
    fn name(&self) -> &str { "ChuoSeikiQT-ZStage" }
    fn description(&self) -> &str { "ChuoSeiki QT single-axis Z stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let resp = self.cmd("?:CHUOSEIKI")?;
        if !resp.starts_with("CHUOSEIKI") {
            return Err(MmError::LocallyDefined(format!("ChuoSeiki QT: unexpected identity: {}", resp)));
        }
        let _ = self.cmd("X:1");
        self.pos_um = self.read_z()?;
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

impl Stage for ChuoSeikiQTZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = (z / self.step_um).round() as i64;
        let r = self.cmd(&format!("AGO:A{}", steps))?;
        Self::check_response(&r)?;
        self.pos_um = z;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.pos_um) }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let steps = (dz / self.step_um).round() as i64;
        let r = self.cmd(&format!("MGO:A{}", steps))?;
        Self::check_response(&r)?;
        self.pos_um += dz;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("H:A")?;
        Self::check_response(&r)?;
        self.pos_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd("L:A");
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((-50_000.0, 50_000.0)) }
    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any("CHUOSEIKI")     // identity
            .any("OK")            // X:1
            .any("+00000050K")    // Q:A0 → 50 steps * 1µm = 50 µm
    }

    #[test]
    fn initialize() {
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any("OK");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 100.0);
    }

    #[test]
    fn move_relative() {
        let t = make_transport().any("OK");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(25.0).unwrap();
        assert!((s.get_position_um().unwrap() - 75.0).abs() < 1e-9);
    }

    #[test]
    fn error_response_fails() {
        let t = make_transport().any("!6");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_position_um(999_999.0).is_err());
    }

    #[test]
    fn no_transport_error() { assert!(ChuoSeikiQTZStage::new().initialize().is_err()); }
}
