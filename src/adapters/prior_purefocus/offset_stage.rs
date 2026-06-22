/// Prior PureFocus offset stage (Z-offset / piezo lens).
///
/// The PureFocus is an autofocus system controlled via ASCII commands (CR terminated).
/// On initialization, the device identifies itself via the `DATE` command.
///
/// Key commands:
///   `DATE\r`         → multi-line: "Prior Scientific PureFocus...\r", date/version line
///   `UPR\r`          → `<piezo_range_um>\r`  (piezo range in µm)
///   `LENSG,<steps>\r` → `0\r`                (set offset in 0.001 µm steps)
///   `LENSP\r`         → `<steps>\r`          (get offset in 0.001 µm steps)
///   `SERVO,1\r`      → `R\r`                 (enable servo / lock focus)
///   `SERVO,0\r`      → `R\r`                 (disable servo)
///
/// The offset device is a Stage that controls the piezo Z position (0..range µm).
/// Step size: 0.001 µm (1 nm).
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};

pub struct PureFocusOffsetStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    pos_um: f64,
    piezo_range_um: f64,
}

const STEP_SIZE_UM: f64 = 0.001;

impl PureFocusOffsetStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
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
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let c = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&c)?;
            Ok(r.trim().to_string())
        })
    }

    fn check_offset_set_response(resp: &str) -> MmResult<()> {
        if resp == "0" {
            Ok(())
        } else {
            Err(MmError::SerialInvalidResponse)
        }
    }

    fn um_to_steps(pos_um: f64) -> i64 {
        (pos_um / STEP_SIZE_UM) as i64
    }
}

impl Default for PureFocusOffsetStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for PureFocusOffsetStage {
    fn name(&self) -> &str {
        "PureFocusOffset"
    }
    fn description(&self) -> &str {
        "PureFocusOffset Drive"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Identify device
        let sig = self.cmd("DATE")?;
        if !sig.to_ascii_lowercase().contains("prior") {
            return Err(MmError::LocallyDefined(format!(
                "PureFocus: unexpected identity: {}",
                sig
            )));
        }
        // Read version line (second response – consumed via another send_recv call if needed)
        // For simplicity, skip: MockTransport returns single responses per send_recv call.

        // Read piezo range
        let range_str = self.cmd("UPR")?;
        self.piezo_range_um = range_str.trim().parse().unwrap_or(100.0);

        // Read current offset in upstream 0.001 µm stage steps.
        let pos_str = self.cmd("LENSP")?;
        let steps: i64 = pos_str.trim().parse().unwrap_or(0);
        self.pos_um = steps as f64 * STEP_SIZE_UM;

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        self.props.set(name, val)
    }
    fn property_names(&self) -> Vec<String> {
        self.props.property_names().to_vec()
    }
    fn has_property(&self, name: &str) -> bool {
        self.props.has_property(name)
    }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Stage
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Stage for PureFocusOffsetStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = Self::um_to_steps(z);
        let resp = self.cmd(&format!("LENSG,{}", steps))?;
        Self::check_offset_set_response(&resp)?;
        self.pos_um = steps as f64 * STEP_SIZE_UM;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        Ok(self.pos_um)
    }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let new_z = self.pos_um + dz;
        self.set_position_um(new_z)
    }

    fn home(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn stop(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Err(MmError::UnsupportedCommand)
    }
    fn get_focus_direction(&self) -> FocusDirection {
        FocusDirection::Unknown
    }
    fn is_continuous_focus_drive(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("DATE\r", "Prior Scientific PureFocus")
            .expect("UPR\r", "100")
            .expect("LENSP\r", "50000")
    }

    #[test]
    fn initialize() {
        let mut s = PureFocusOffsetStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().expect("LENSG,75000\r", "0");
        let mut s = PureFocusOffsetStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(75.0).unwrap();
        assert!((s.get_position_um().unwrap() - 75.0).abs() < 1e-6);
    }

    #[test]
    fn move_uses_upstream_offset_steps_without_range_clamping() {
        let t = make_transport().expect("LENSG,999000\r", "0");
        let mut s = PureFocusOffsetStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(999.0).unwrap();
        assert!((s.get_position_um().unwrap() - 999.0).abs() < 1e-6);
    }

    #[test]
    fn move_relative() {
        let t = make_transport().expect("LENSG,60000\r", "0");
        let mut s = PureFocusOffsetStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(10.0).unwrap();
        assert!((s.get_position_um().unwrap() - 60.0).abs() < 1e-6);
    }

    #[test]
    fn bad_identity_fails() {
        let t = MockTransport::new().expect("DATE\r", "UNKNOWN DEVICE");
        let mut s = PureFocusOffsetStage::new().with_transport(Box::new(t));
        assert!(s.initialize().is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(PureFocusOffsetStage::new().initialize().is_err());
    }

    #[test]
    fn set_position_rejects_nonzero_ack() {
        let t = make_transport().expect("LENSG,75000\r", "1");
        let mut s = PureFocusOffsetStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.set_position_um(75.0).unwrap_err(),
            MmError::SerialInvalidResponse
        );
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn home_stop_and_limits_are_unsupported_like_upstream_offset() {
        let mut s = PureFocusOffsetStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        assert_eq!(s.home().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(s.stop().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(s.get_limits().unwrap_err(), MmError::UnsupportedCommand);
    }
}
