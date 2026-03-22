/// Wienecke & Sinske WSB ZPiezo stage (WS or CAN protocol).
///
/// ASCII command interface (CR terminated):
///   `POS Z\r`         → "<z_nm>\r\n"
///   `MOVE Z <nm>\r`   → "OK\r\n" or "ERR <msg>"
///   `RMOVE Z <dnm>\r` → "OK\r\n" or "ERR <msg>"
///   `STOP\r`          → "OK\r\n"
///
/// Step size: 0.001 µm (1 nm).  Positions in nm on the wire.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

const NM_PER_UM: f64 = 1000.0;

pub struct WSZStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    pos_um: f64,
}

impl WSZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, transport: None, initialized: false, pos_um: 0.0 }
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
            Err(MmError::LocallyDefined(format!("WS Z error: {}", resp)))
        } else {
            Ok(())
        }
    }
}

impl Default for WSZStage { fn default() -> Self { Self::new() } }

impl Device for WSZStage {
    fn name(&self) -> &str { "WS-ZStage" }
    fn description(&self) -> &str { "Wienecke & Sinske WSB ZPiezo stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let resp = self.cmd("POS Z")?;
        let nm: i64 = resp.trim().parse().unwrap_or(0);
        self.pos_um = nm as f64 / NM_PER_UM;
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

impl Stage for WSZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let znm = (z * NM_PER_UM).round() as i64;
        let resp = self.cmd(&format!("MOVE Z {}", znm))?;
        Self::check_ok(&resp)?;
        self.pos_um = z;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.pos_um) }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let dznm = (dz * NM_PER_UM).round() as i64;
        let resp = self.cmd(&format!("RMOVE Z {}", dznm))?;
        Self::check_ok(&resp)?;
        self.pos_um += dz;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        let resp = self.cmd("HOME")?;
        Self::check_ok(&resp)?;
        self.pos_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd("STOP");
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((0.0, 200.0)) }
    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize() {
        let t = MockTransport::new().any("50000"); // 50 µm
        let mut s = WSZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new().any("0").any("OK");
        let mut s = WSZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 100.0);
    }

    #[test]
    fn move_relative() {
        let t = MockTransport::new().any("10000").any("OK");
        let mut s = WSZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(5.0).unwrap();
        assert!((s.get_position_um().unwrap() - 15.0).abs() < 1e-9);
    }

    #[test]
    fn error_fails() {
        let t = MockTransport::new().any("0").any("ERR: limit");
        let mut s = WSZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_position_um(999.0).is_err());
    }

    #[test]
    fn no_transport_error() { assert!(WSZStage::new().initialize().is_err()); }
}
