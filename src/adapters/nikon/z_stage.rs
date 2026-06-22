/// Nikon Remote Focus Accessory Z-stage.
///
/// Protocol (TX `\r`, RX `\r`):
///   `MZ {steps}\r`  → move to absolute position; response `:A\r`
///   `WZ\r`          → query position; response `:A{steps}\r`
///
/// Success prefix `:A`, error prefix `:N{code}`.
/// Step size: 0.1 µm / step.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::{Cell, RefCell};

const DEFAULT_STEP_SIZE_UM: f64 = 0.1;

pub struct NikonZStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    pos_um: Cell<f64>,
    step_size_um: f64,
}

impl NikonZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            pos_um: Cell::new(0.0),
            step_size_um: DEFAULT_STEP_SIZE_UM,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = RefCell::new(Some(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        let mut transport = self.transport.borrow_mut();
        match transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let c = format!("{}\r", command);
        self.call_transport(|t| Ok(t.send_recv(&c)?.trim().to_string()))
    }

    fn read_position_um(&self) -> MmResult<f64> {
        let resp = self.cmd("WZ")?;
        let val = Self::check_response(&resp)?;
        let steps: i64 = val
            .trim()
            .parse()
            .map_err(|_| MmError::LocallyDefined(format!("Bad Nikon Z position: {}", resp)))?;
        let pos_um = steps as f64 * self.step_size_um;
        self.pos_um.set(pos_um);
        Ok(pos_um)
    }

    fn check_response(resp: &str) -> MmResult<String> {
        if let Some(rest) = resp.strip_prefix(":A") {
            Ok(rest.to_string())
        } else if let Some(code) = resp.strip_prefix(":N") {
            Err(MmError::LocallyDefined(format!(
                "Nikon Z error code: {}",
                code
            )))
        } else {
            Err(MmError::LocallyDefined(format!(
                "Nikon Z unexpected response: '{}'",
                resp
            )))
        }
    }
}

impl Default for NikonZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for NikonZStage {
    fn name(&self) -> &str {
        "ZStage"
    }
    fn description(&self) -> &str {
        "Nikon Remote Focus Accessory driver adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        self.read_position_um()?;
        if !self.props.has_property("StepSizeUm") {
            self.props.define_property(
                "StepSizeUm",
                PropertyValue::Float(DEFAULT_STEP_SIZE_UM),
                false,
            )?;
        }
        self.step_size_um = DEFAULT_STEP_SIZE_UM;
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
        if name == "Port" && self.initialized {
            return Err(MmError::InvalidPropertyValue);
        }
        if name != "StepSizeUm" {
            return self.props.set(name, val);
        }
        if !self.props.has_property(name) {
            return self.props.set(name, val);
        }
        let step_size_um = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
        if step_size_um <= 0.0 {
            return Err(MmError::InvalidPropertyValue);
        }
        self.props.set(name, val)?;
        self.step_size_um = step_size_um;
        Ok(())
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

impl Stage for NikonZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = (z / self.step_size_um + 0.5) as i64;
        let resp = self.cmd(&format!("MZ {}", steps))?;
        Self::check_response(&resp)?;
        self.pos_um.set(steps as f64 * self.step_size_um);
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        if self.initialized {
            self.read_position_um()
        } else {
            Ok(self.pos_um.get())
        }
    }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        self.set_position_um(self.get_position_um()? + dz)
    }

    fn home(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
    fn stop(&mut self) -> MmResult<()> {
        Ok(())
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

    #[test]
    fn step_size_property_created_after_successful_initialize() {
        let t = MockTransport::new().expect("WZ\r", ":A0");
        let mut s = NikonZStage::new().with_transport(Box::new(t));
        assert!(!s.has_property("StepSizeUm"));
        assert!(s
            .set_property("StepSizeUm", PropertyValue::Float(0.2))
            .is_err());

        s.initialize().unwrap();

        assert!(s.has_property("StepSizeUm"));
        assert_eq!(
            s.get_property("StepSizeUm").unwrap(),
            PropertyValue::Float(DEFAULT_STEP_SIZE_UM)
        );
    }

    #[test]
    fn failed_initialize_does_not_create_step_size_property() {
        let t = MockTransport::new().expect("WZ\r", ":N1");
        let mut s = NikonZStage::new().with_transport(Box::new(t));

        assert!(s.initialize().is_err());

        assert!(!s.has_property("StepSizeUm"));
    }

    #[test]
    fn initialize_reads_position() {
        let t = MockTransport::new().any(":A500").any(":A500");
        let mut s = NikonZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new().any(":A0").any(":A").any(":A100");
        let mut s = NikonZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(10.0).unwrap();
        assert!((s.get_position_um().unwrap() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute_reports_quantized_position() {
        let t = MockTransport::new()
            .expect("WZ\r", ":A0")
            .expect("MZ 100\r", ":A")
            .expect("WZ\r", ":A100");
        let mut s = NikonZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(10.04).unwrap();
        assert!((s.get_position_um().unwrap() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn error_response_fails() {
        let t = MockTransport::new().any(":N-1");
        let mut s = NikonZStage::new().with_transport(Box::new(t));
        assert!(s.initialize().is_err());
    }

    #[test]
    fn malformed_position_response_fails() {
        let t = MockTransport::new().expect("WZ\r", ":Anot-a-number");
        let mut s = NikonZStage::new().with_transport(Box::new(t));
        assert!(s.initialize().is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(NikonZStage::new().initialize().is_err());
    }
}
