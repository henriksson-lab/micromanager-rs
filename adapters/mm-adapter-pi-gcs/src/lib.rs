/// Physik Instrumente (PI) GCS (General Command Set) Z-stage adapter.
///
/// Protocol (TX `\n`, RX `\n`):
///   `SVO A 1\n`        → enable servo for axis A
///   `MOV A {pos}\n`    → move to absolute position in mm
///   `POS? A\n`         → query position; response `A={value}\n`
///   `ERR?\n`           → query last error code; 0 = success
///
/// Step size: 0.01 µm default. Axis name configurable (default "A").
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

pub struct PiGcsZStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    pos_um: f64,
    axis: String,
    step_size_um: f64,
    limit_um: f64,
}

impl PiGcsZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Axis", PropertyValue::String("A".into()), false).unwrap();
        props.define_property("StepSizeUm", PropertyValue::Float(0.01), false).unwrap();
        props.define_property("LimitUm", PropertyValue::Float(500.0), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            pos_um: 0.0,
            axis: "A".into(),
            step_size_um: 0.01,
            limit_um: 500.0,
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

    fn send(&mut self, command: &str) -> MmResult<String> {
        let c = format!("{}\n", command);
        self.call_transport(|t| Ok(t.send_recv(&c)?.trim().to_string()))
    }

    /// Parse a GCS response that may contain `key=value`; extract value after last `=`.
    fn parse_value(resp: &str) -> MmResult<f64> {
        let part = resp.rfind('=').map(|i| &resp[i + 1..]).unwrap_or(resp);
        part.trim().parse::<f64>().map_err(|_| {
            MmError::LocallyDefined(format!("PI GCS parse error: '{}'", resp))
        })
    }

    fn check_error(&mut self) -> MmResult<()> {
        let resp = self.send("ERR?")?;
        let code: i32 = resp.trim().parse().unwrap_or(-1);
        if code == 0 { Ok(()) }
        else { Err(MmError::LocallyDefined(format!("PI GCS error code: {}", code))) }
    }
}

impl Default for PiGcsZStage { fn default() -> Self { Self::new() } }

impl Device for PiGcsZStage {
    fn name(&self) -> &str { "PIZStage" }
    fn description(&self) -> &str { "Physik Instrumente GCS Z-stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let axis = self.axis.clone();
        // Enable servo
        self.send(&format!("SVO {} 1", axis))?;
        // Query current position
        let resp = self.send(&format!("POS? {}", axis))?;
        let pos_mm = Self::parse_value(&resp)?;
        self.pos_um = pos_mm * 1000.0;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Axis" {
            if let PropertyValue::String(ref s) = val { self.axis = s.clone(); }
        } else if name == "StepSizeUm" {
            if let PropertyValue::Float(f) = val { self.step_size_um = f; }
        } else if name == "LimitUm" {
            if let PropertyValue::Float(f) = val { self.limit_um = f; }
        }
        self.props.set(name, val)
    }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Stage }
    fn busy(&self) -> bool { false }
}

impl Stage for PiGcsZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let axis = self.axis.clone();
        let pos_mm = z / 1000.0;
        self.send(&format!("MOV {} {:.6}", axis, pos_mm))?;
        self.check_error()?;
        self.pos_um = z;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.pos_um) }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let target = self.pos_um + dz;
        self.set_position_um(target)
    }

    fn home(&mut self) -> MmResult<()> { self.set_position_um(0.0) }

    fn stop(&mut self) -> MmResult<()> {
        let axis = self.axis.clone();
        let _ = self.send(&format!("STP {}", axis));
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((0.0, self.limit_um)) }
    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize_reads_position() {
        // SVO → empty ok, POS? → A=0.05000 (50 µm = 0.05 mm)
        let t = MockTransport::new().any("").any("A=0.05000");
        let mut s = PiGcsZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new().any("").any("A=0.0").any("").any("0");
        let mut s = PiGcsZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
        assert!((s.get_position_um().unwrap() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn move_relative() {
        let t = MockTransport::new().any("").any("A=0.0").any("").any("0");
        let mut s = PiGcsZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(25.0).unwrap();
        assert!((s.get_position_um().unwrap() - 25.0).abs() < 1e-6);
    }

    #[test]
    fn error_code_fails() {
        let t = MockTransport::new().any("").any("A=0.0").any("").any("5");
        let mut s = PiGcsZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_position_um(10.0).is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(PiGcsZStage::new().initialize().is_err());
    }
}
