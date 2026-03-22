/// ASI Tiger Controller — Z stage.
///
/// Protocol (TX `\r`, RX `\r\n`):
///   `M Z=<z>\r`   → `:A \r\n`      absolute move (1/10 µm units)
///   `R Z=<dz>\r`  → `:A \r\n`      relative move
///   `W Z\r`       → `:A Z=<z>\r\n` query Z
///   `Z\r`         → `:A \r\n`      set zero
///   `HM Z+\r`     → `:A \r\n`      home Z
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

const UNITS_PER_UM: f64 = 10.0;

pub struct AsiTigerZStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position_um: f64,
}

impl AsiTigerZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, transport: None, initialized: false, position_um: 0.0 }
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
        let full = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    fn cmd_ok(&mut self, command: &str) -> MmResult<String> {
        let resp = self.cmd(command)?;
        if resp.starts_with(":N") {
            Err(MmError::LocallyDefined(format!("ASI Tiger error: {}", resp)))
        } else {
            Ok(resp)
        }
    }
}

impl Default for AsiTigerZStage { fn default() -> Self { Self::new() } }

impl Device for AsiTigerZStage {
    fn name(&self) -> &str { "AsiTigerZStage" }
    fn description(&self) -> &str { "ASI Tiger Z Stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let resp = self.cmd_ok("W Z")?;
        let val = resp.split_whitespace()
            .find(|s| s.starts_with("Z="))
            .and_then(|s| s[2..].parse::<f64>().ok())
            .unwrap_or(0.0);
        self.position_um = val / UNITS_PER_UM;
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

impl Stage for AsiTigerZStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let cmd = format!("M Z={:.0}", pos * UNITS_PER_UM);
        self.cmd_ok(&cmd)?;
        self.position_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.position_um) }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let cmd = format!("R Z={:.0}", d * UNITS_PER_UM);
        self.cmd_ok(&cmd)?;
        self.position_um += d;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        self.cmd_ok("HM Z+")?;
        self.position_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> { self.cmd_ok("/ ")?; Ok(()) }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((-5000.0, 5000.0)) }

    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize() {
        let t = MockTransport::new().expect("W Z\r", ":A Z=0");
        let mut s = AsiTigerZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap()).abs() < 0.001);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new()
            .expect("W Z\r", ":A Z=0")
            .expect("M Z=500\r", ":A");
        let mut s = AsiTigerZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(50.0).unwrap(); // 50µm → 500 units
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 0.01);
    }

    #[test]
    fn move_relative() {
        let t = MockTransport::new()
            .expect("W Z\r", ":A Z=500")
            .expect("R Z=100\r", ":A");
        let mut s = AsiTigerZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(10.0).unwrap();
        assert!((s.get_position_um().unwrap() - 60.0).abs() < 0.01);
    }

    #[test]
    fn no_transport_error() { assert!(AsiTigerZStage::new().initialize().is_err()); }
}
