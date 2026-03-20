/// ASI Z-stage (Applied Scientific Instrumentation).
///
/// Protocol (ASCII, `\r` terminated):
///   `M Z=<val>\r`  → move to absolute position; val in tenths of microns (10 units = 1 µm)
///                    response: `:A\r` (ok) or `:N<code>\r` (error)
///   `W Z\r`        → query position; response `:A Z=<val>\r`
///   `R Z=<val>\r`  → move relative; same response as M
///   `/\r`          → status query; response `:A\r` when idle, `:B\r` when busy
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Stage};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, FocusDirection, PropertyValue};

const UNITS_PER_UM: f64 = 10.0; // ASI uses tenths of microns

pub struct AsiZStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position_um: f64,
}

impl AsiZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Position_um", PropertyValue::Float(0.0), false).unwrap();

        Self { props, transport: None, initialized: false, position_um: 0.0 }
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
        let cmd = command.to_string();
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }

    fn check_response(resp: &str) -> MmResult<()> {
        if resp.starts_with(":N") {
            return Err(MmError::LocallyDefined(format!("ASI error: {}", resp)));
        }
        Ok(())
    }

    /// Parse `:A Z=<value>` → value in µm.
    fn parse_z_position(resp: &str) -> MmResult<f64> {
        let resp = resp.trim();
        // Expected: ":A Z=12345" or "Z=12345"
        let val_str = resp
            .split_whitespace()
            .find(|s| s.starts_with("Z="))
            .and_then(|s| s.strip_prefix("Z="))
            .ok_or_else(|| MmError::LocallyDefined(format!("Cannot parse Z position: {}", resp)))?;
        let val: f64 = val_str.parse()
            .map_err(|_| MmError::LocallyDefined(format!("Non-numeric Z: {}", val_str)))?;
        Ok(val / UNITS_PER_UM)
    }
}

impl Default for AsiZStage {
    fn default() -> Self { Self::new() }
}

impl Device for AsiZStage {
    fn name(&self) -> &str { "ASI-ZStage" }
    fn description(&self) -> &str { "ASI Z-stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let resp = self.cmd("W Z")?;
        self.position_um = Self::parse_z_position(&resp)?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Position_um" => Ok(PropertyValue::Float(self.position_um)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Position_um" => {
                let um = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_position_um(um)
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Stage }
    fn busy(&self) -> bool { false }
}

impl Stage for AsiZStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let units = (pos * UNITS_PER_UM).round() as i64;
        let resp = self.cmd(&format!("M Z={}", units))?;
        Self::check_response(&resp)?;
        self.position_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.position_um) }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let units = (d * UNITS_PER_UM).round() as i64;
        let resp = self.cmd(&format!("R Z={}", units))?;
        Self::check_response(&resp)?;
        self.position_um += d;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        let resp = self.cmd("! Z")?;
        Self::check_response(&resp)?;
        self.position_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd("\\");
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((-100_000.0, 100_000.0)) }
    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::TowardSample }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    #[test]
    fn initialize_reads_position() {
        let t = MockTransport::new().expect("W Z", ":A Z=1000");
        let mut stage = AsiZStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 100.0);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new()
            .expect("W Z", ":A Z=0")
            .expect("M Z=2500", ":A");
        let mut stage = AsiZStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_position_um(250.0).unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 250.0);
    }

    #[test]
    fn move_relative() {
        let t = MockTransport::new()
            .expect("W Z", ":A Z=1000")
            .expect("R Z=500", ":A");
        let mut stage = AsiZStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_relative_position_um(50.0).unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 150.0);
    }

    #[test]
    fn error_response_propagated() {
        let t = MockTransport::new()
            .expect("W Z", ":A Z=0")
            .expect("M Z=10000", ":N-1");
        let mut stage = AsiZStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        assert!(stage.set_position_um(1000.0).is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(AsiZStage::new().initialize().is_err());
    }
}
