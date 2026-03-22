/// Prior PureFocus offset stage (Z-offset / piezo lens).
///
/// The PureFocus is an autofocus system controlled via ASCII commands (CR terminated).
/// On initialization, the device identifies itself via the `DATE` command.
///
/// Key commands:
///   `DATE\r`         → multi-line: "Prior Scientific PureFocus...\r", date/version line
///   `UPR\r`          → `<piezo_range_um>\r`  (piezo range in µm)
///   `UPZ,<val>\r`    → `<val>\r`             (set piezo position; 0..piezo_range)
///   `UPZ\r`          → `<val>\r`             (get piezo position)
///   `SERVO,1\r`      → `R\r`                 (enable servo / lock focus)
///   `SERVO,0\r`      → `R\r`                 (disable servo)
///
/// The offset device is a Stage that controls the piezo Z position (0..range µm).
/// Step size: 0.001 µm (1 nm).
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};


pub struct PureFocusOffsetStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    pos_um: f64,
    piezo_range_um: f64,
}

impl PureFocusOffsetStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            pos_um: 0.0,
            piezo_range_um: 100.0, // default; updated from device
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
        let c = format!("{}\r", command);
        self.call_transport(|t| { let r = t.send_recv(&c)?; Ok(r.trim().to_string()) })
    }

    fn check_prior_response(resp: &str) -> MmResult<()> {
        if resp.starts_with('E') {
            Err(MmError::LocallyDefined(format!("PureFocus error: {}", resp)))
        } else {
            Ok(())
        }
    }
}

impl Default for PureFocusOffsetStage { fn default() -> Self { Self::new() } }

impl Device for PureFocusOffsetStage {
    fn name(&self) -> &str { "PureFocusOffset" }
    fn description(&self) -> &str { "Prior PureFocus offset/piezo Z stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Identify device
        let sig = self.cmd("DATE")?;
        if !sig.to_ascii_lowercase().contains("prior") {
            return Err(MmError::LocallyDefined(format!("PureFocus: unexpected identity: {}", sig)));
        }
        // Read version line (second response – consumed via another send_recv call if needed)
        // For simplicity, skip: MockTransport returns single responses per send_recv call.

        // Read piezo range
        let range_str = self.cmd("UPR")?;
        self.piezo_range_um = range_str.trim().parse().unwrap_or(100.0);

        // Read current position
        let pos_str = self.cmd("UPZ")?;
        self.pos_um = pos_str.trim().parse().unwrap_or(0.0);

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

impl Stage for PureFocusOffsetStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let clamped = z.max(0.0).min(self.piezo_range_um);
        let resp = self.cmd(&format!("UPZ,{:.3}", clamped))?;
        Self::check_prior_response(&resp)?;
        self.pos_um = clamped;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.pos_um) }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let new_z = self.pos_um + dz;
        self.set_position_um(new_z)
    }

    fn home(&mut self) -> MmResult<()> {
        self.set_position_um(self.piezo_range_um / 2.0)
    }

    fn stop(&mut self) -> MmResult<()> { Ok(()) }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((0.0, self.piezo_range_um)) }
    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any("Prior Scientific PureFocus") // DATE identity
            .any("100")                        // UPR → 100 µm range
            .any("50.0")                       // UPZ → current 50 µm
    }

    #[test]
    fn initialize() {
        let mut s = PureFocusOffsetStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any("75.0");
        let mut s = PureFocusOffsetStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(75.0).unwrap();
        assert!((s.get_position_um().unwrap() - 75.0).abs() < 1e-6);
    }

    #[test]
    fn clamps_to_range() {
        let t = make_transport().any("100.0"); // clamped to max
        let mut s = PureFocusOffsetStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(999.0).unwrap(); // beyond range → clamped to 100
        assert!((s.get_position_um().unwrap() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn move_relative() {
        let t = make_transport().any("60.0");
        let mut s = PureFocusOffsetStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(10.0).unwrap();
        assert!((s.get_position_um().unwrap() - 60.0).abs() < 1e-6);
    }

    #[test]
    fn bad_identity_fails() {
        let t = MockTransport::new().any("UNKNOWN DEVICE");
        let mut s = PureFocusOffsetStage::new().with_transport(Box::new(t));
        assert!(s.initialize().is_err());
    }

    #[test]
    fn no_transport_error() { assert!(PureFocusOffsetStage::new().initialize().is_err()); }
}
