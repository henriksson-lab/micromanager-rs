/// pgFocus open-source laser autofocus stage adapter.
///
/// pgFocus (UMass Medical School) is a reflection-based focus stabilization system.
/// It communicates via a USB serial port using simple ASCII commands (CR terminated).
///
/// Key commands:
///   `version\r`       → `<version_string>\r\n`
///   `offset <val>\r`  → (no response; set focus offset 0..127)
///   `f\r`             → (start continuous focusing / lock)
///   `s\r`             → (stop / unlock)
///   `l\r`             → `<offset_float>\r\n` (query current offset)
///   `mpv <n>\r`       → (set microns-per-volt, n > 0)
///
/// This adapter models pgFocus as a Stage that controls the focus offset.
/// Range: 0.0 to 127.0 arbitrary units (interpreted as µm here).
/// Step size: 1.0 µm (1 offset unit per step).
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

const MAX_OFFSET: f64 = 127.0;

pub struct PgFocusStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    offset: f64,
    locking: bool,
}

impl PgFocusStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, transport: None, initialized: false, offset: 0.0, locking: false }
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

    /// Send a fire-and-forget command (no response expected / response ignored).
    fn send(&mut self, command: &str) -> MmResult<()> {
        let c = format!("{}\r", command);
        self.call_transport(|t| { t.send(&c) })
    }
}

impl Default for PgFocusStage { fn default() -> Self { Self::new() } }

impl Device for PgFocusStage {
    fn name(&self) -> &str { "pgFocus-Stage" }
    fn description(&self) -> &str { "pgFocus open-source laser autofocus stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Check firmware version
        let ver = self.cmd("version")?;
        if ver.is_empty() {
            return Err(MmError::LocallyDefined("pgFocus: no version response".into()));
        }
        // Query current offset
        let off_str = self.cmd("l")?;
        self.offset = off_str.trim().parse().unwrap_or(0.0);
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.locking {
            let _ = self.send("s"); // stop focusing
            self.locking = false;
        }
        self.initialized = false;
        Ok(())
    }

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

impl Stage for PgFocusStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let clamped = z.max(0.0).min(MAX_OFFSET);
        self.send(&format!("offset {:.0}", clamped))?;
        self.offset = clamped;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.offset) }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let new_z = self.offset + dz;
        self.set_position_um(new_z)
    }

    fn home(&mut self) -> MmResult<()> {
        self.set_position_um(0.0)
    }

    fn stop(&mut self) -> MmResult<()> {
        if self.locking {
            self.send("s")?;
            self.locking = false;
        }
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((0.0, MAX_OFFSET)) }
    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any("pgFocus_v1.2")  // version
            .any("32.0")          // l → current offset
    }

    #[test]
    fn initialize() {
        let mut s = PgFocusStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 32.0).abs() < 1e-9);
    }

    #[test]
    fn set_position() {
        let t = make_transport(); // send() calls don't consume mock responses
        let mut s = PgFocusStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(64.0).unwrap();
        assert!((s.get_position_um().unwrap() - 64.0).abs() < 1e-9);
    }

    #[test]
    fn clamps_to_max() {
        let t = make_transport();
        let mut s = PgFocusStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(999.0).unwrap();
        assert!((s.get_position_um().unwrap() - MAX_OFFSET).abs() < 1e-9);
    }

    #[test]
    fn clamps_to_zero() {
        let t = make_transport();
        let mut s = PgFocusStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(-10.0).unwrap();
        assert!((s.get_position_um().unwrap()).abs() < 1e-9);
    }

    #[test]
    fn relative_move() {
        let t = make_transport();
        let mut s = PgFocusStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(10.0).unwrap();
        assert!((s.get_position_um().unwrap() - 42.0).abs() < 1e-9);
    }

    #[test]
    fn no_version_fails() {
        let t = MockTransport::new().any(""); // empty version
        let mut s = PgFocusStage::new().with_transport(Box::new(t));
        assert!(s.initialize().is_err());
    }

    #[test]
    fn no_transport_error() { assert!(PgFocusStage::new().initialize().is_err()); }
}
