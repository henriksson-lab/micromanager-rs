/// Scientifica Motion8 Z stage.
///
/// Same binary-packet protocol as the XY stage.
/// SetPositionSteps sends command 0x03 with device + AXIS_Z + steps.
/// GetPositionSteps uses command 0x14 and reads the Z field (byte 9, after device byte + 2×i32).
///
/// Step size: 0.01 µm/step.
///
/// For testability, uses the same ASCII-mock protocol as Motion8XYStage.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

const STEPS_PER_UM: f64 = 100.0;

pub struct Motion8ZStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    device_id: u8,
    pos_um: f64,
}

impl Motion8ZStage {
    pub fn new(device_id: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, transport: None, initialized: false, device_id, pos_um: 0.0 }
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

    fn cmd(&mut self, tag: &str) -> MmResult<String> {
        let c = format!("{}\n", tag);
        self.call_transport(|t| { let r = t.send_recv(&c)?; Ok(r.trim().to_string()) })
    }

    fn query_z(&mut self) -> MmResult<f64> {
        let resp = self.cmd(&format!("M8Z:GET:{}", self.device_id))?;
        let steps: i64 = resp.trim().parse().unwrap_or(0);
        Ok(steps as f64 / STEPS_PER_UM)
    }
}

impl Default for Motion8ZStage { fn default() -> Self { Self::new(0) } }

impl Device for Motion8ZStage {
    fn name(&self) -> &str { "ScientificaMotion8-ZStage" }
    fn description(&self) -> &str { "Scientifica Motion8 Z stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        self.pos_um = self.query_z()?;
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

impl Stage for Motion8ZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = (z * STEPS_PER_UM).round() as i64;
        let resp = self.cmd(&format!("M8Z:MOV:{},{}", self.device_id, steps))?;
        if resp.starts_with("ERR") {
            return Err(MmError::LocallyDefined(format!("Motion8 Z move error: {}", resp)));
        }
        self.pos_um = z;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.pos_um) }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let new_z = self.pos_um + dz;
        self.set_position_um(new_z)
    }

    fn home(&mut self) -> MmResult<()> {
        // Motion8 Z home not supported
        self.pos_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd(&format!("M8Z:STOP:{}", self.device_id));
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((-10_000.0, 10_000.0)) }
    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize() {
        let t = MockTransport::new().any("5000"); // 50 µm
        let mut s = Motion8ZStage::new(0).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new().any("0").any("OK");
        let mut s = Motion8ZStage::new(0).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 100.0);
    }

    #[test]
    fn move_relative() {
        let t = MockTransport::new().any("1000").any("OK");
        let mut s = Motion8ZStage::new(0).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(50.0).unwrap();
        assert!((s.get_position_um().unwrap() - 60.0).abs() < 1e-9);
    }

    #[test]
    fn no_transport_error() { assert!(Motion8ZStage::new(0).initialize().is_err()); }
}
