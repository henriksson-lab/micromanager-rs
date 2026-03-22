/// Nikon Remote Focus Accessory Z-stage.
///
/// Protocol (TX `\r`, RX `\r`):
///   `MZ {steps}\r`  → move to absolute position; response `:A\r`
///   `WZ\r`          → query position; response `:A{steps}\r`
///
/// Success prefix `:A`, error prefix `:N{code}`.
/// Step size: 0.1 µm / step.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

const STEPS_PER_UM: f64 = 10.0;

pub struct NikonZStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    pos_um: f64,
}

impl NikonZStage {
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
        self.call_transport(|t| Ok(t.send_recv(&c)?.trim().to_string()))
    }

    fn check_response(resp: &str) -> MmResult<String> {
        if let Some(rest) = resp.strip_prefix(":A") {
            Ok(rest.to_string())
        } else if let Some(code) = resp.strip_prefix(":N") {
            Err(MmError::LocallyDefined(format!("Nikon Z error code: {}", code)))
        } else {
            Err(MmError::LocallyDefined(format!("Nikon Z unexpected response: '{}'", resp)))
        }
    }
}

impl Default for NikonZStage { fn default() -> Self { Self::new() } }

impl Device for NikonZStage {
    fn name(&self) -> &str { "NikonZStage" }
    fn description(&self) -> &str { "Nikon Remote Focus Accessory Z-stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let resp = self.cmd("WZ")?;
        let val = Self::check_response(&resp)?;
        let steps: i64 = val.trim().parse().unwrap_or(0);
        self.pos_um = steps as f64 / STEPS_PER_UM;
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

impl Stage for NikonZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = (z * STEPS_PER_UM).round() as i64;
        let resp = self.cmd(&format!("MZ {}", steps))?;
        Self::check_response(&resp)?;
        self.pos_um = z;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.pos_um) }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        self.set_position_um(self.pos_um + dz)
    }

    fn home(&mut self) -> MmResult<()> { self.set_position_um(0.0) }
    fn stop(&mut self) -> MmResult<()> { Ok(()) }
    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((-1_000.0, 1_000.0)) }
    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize_reads_position() {
        let t = MockTransport::new().any(":A500");
        let mut s = NikonZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new().any(":A0").any(":A");
        let mut s = NikonZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(10.0).unwrap();
        assert!((s.get_position_um().unwrap() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn error_response_fails() {
        let t = MockTransport::new().any(":N-1");
        let mut s = NikonZStage::new().with_transport(Box::new(t));
        assert!(s.initialize().is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(NikonZStage::new().initialize().is_err());
    }
}
