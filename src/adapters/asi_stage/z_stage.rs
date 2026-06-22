/// ASI Z-stage (Applied Scientific Instrumentation).
///
/// Protocol (ASCII, `\r` terminated):
///   `M Z=<val>\r`  → move to absolute position; val in tenths of microns (10 units = 1 µm)
///                    response: `:A\r` (ok) or `:N<code>\r` (error)
///   `W Z\r`        → query position; response `:A <val>\r` or `:A Z=<val>\r`
///   `R Z=<val>\r`  → move relative; same response as M
///   `/\r`          → status query; response `:A\r` when idle, `:B\r` when busy
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::RefCell;

const UNITS_PER_UM: f64 = 10.0; // ASI uses tenths of microns

pub struct AsiZStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    axis: String,
    position_um: f64,
}

impl AsiZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_pre_init_property("Axis", PropertyValue::String("Z".into()))
            .unwrap();
        props.set_allowed_values("Axis", &["F", "P", "Z"]).unwrap();
        props
            .define_property("Version", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("BuildName", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("AxisDirection", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .set_allowed_values("AxisDirection", &["1", "-1"])
            .unwrap();
        props
            .define_property("StepSize_um", PropertyValue::Float(0.1), true)
            .unwrap();

        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            axis: "Z".to_string(),
            position_um: 0.0,
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
        match self.transport.borrow_mut().as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let cmd = format!("{}\r", command);
        self.call_transport(|t| {
            t.purge()?;
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }

    fn check_response(resp: &str) -> MmResult<()> {
        if resp.starts_with(":N") {
            return Err(MmError::LocallyDefined(format!("ASI error: {}", resp)));
        }
        if !resp.starts_with(":A") {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(())
    }

    /// Parse `:A <value>` or `:A Z=<value>` → value in µm.
    fn parse_z_position(resp: &str) -> MmResult<f64> {
        let resp = resp.trim();
        if resp.starts_with(":N") {
            return Err(MmError::LocallyDefined(format!("ASI error: {}", resp)));
        }
        let val_str = resp
            .split_whitespace()
            .skip_while(|s| s.starts_with(":A"))
            .find_map(|s| {
                if let Some((_, value)) = s.split_once('=') {
                    Some(value)
                } else {
                    Some(s)
                }
            })
            .ok_or_else(|| MmError::LocallyDefined(format!("Cannot parse Z position: {}", resp)))?;
        let val: f64 = val_str
            .parse()
            .map_err(|_| MmError::LocallyDefined(format!("Non-numeric Z: {}", val_str)))?;
        Ok(val / UNITS_PER_UM)
    }

    fn axis(&self) -> &str {
        &self.axis
    }
}

impl Default for AsiZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for AsiZStage {
    fn name(&self) -> &str {
        "ZStage"
    }
    fn description(&self) -> &str {
        "ASI Z Stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        let resp = self.cmd(&format!("W {}", self.axis()))?;
        self.position_um = Self::parse_z_position(&resp)?;
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
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "Axis" if self.initialized => Err(MmError::InvalidPropertyValue),
            "Axis" => {
                self.props.set(name, val)?;
                self.axis = self.props.get("Axis")?.as_str().to_string();
                Ok(())
            }
            _ => self.props.set(name, val),
        }
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
        self.cmd("/")
            .map(|resp| resp.trim().starts_with(":B"))
            .unwrap_or(false)
    }
}

impl Stage for AsiZStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let units = pos * UNITS_PER_UM;
        let resp = self.cmd(&format!("M {}={:.6}", self.axis(), units))?;
        Self::check_response(&resp)?;
        self.position_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        let resp = self.cmd(&format!("W {}", self.axis()))?;
        Self::parse_z_position(&resp)
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let units = d * UNITS_PER_UM;
        let resp = self.cmd(&format!("R {}={:.6}", self.axis(), units))?;
        Self::check_response(&resp)?;
        self.position_um += d;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd("\\");
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Err(MmError::UnsupportedCommand)
    }
    fn get_focus_direction(&self) -> FocusDirection {
        FocusDirection::TowardSample
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
    fn initialize_reads_position() {
        let t = MockTransport::new()
            .expect("W Z\r", ":A 1000")
            .expect("W Z\r", ":A 1000");
        let mut stage = AsiZStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 100.0);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new()
            .expect("W Z\r", ":A Z=0")
            .expect("M Z=2500.000000\r", ":A")
            .expect("W Z\r", ":A Z=2500");
        let mut stage = AsiZStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_position_um(250.0).unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 250.0);
    }

    #[test]
    fn move_relative() {
        let t = MockTransport::new()
            .expect("W Z\r", ":A Z=1000")
            .expect("R Z=500.000000\r", ":A")
            .expect("W Z\r", ":A Z=1500");
        let mut stage = AsiZStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_relative_position_um(50.0).unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 150.0);
    }

    #[test]
    fn error_response_propagated() {
        let t = MockTransport::new()
            .expect("W Z\r", ":A Z=0")
            .expect("M Z=10000.000000\r", ":N-1");
        let mut stage = AsiZStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        assert!(stage.set_position_um(1000.0).is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(AsiZStage::new().initialize().is_err());
    }

    #[test]
    fn axis_property_selects_command_axis() {
        let t = MockTransport::new()
            .expect("W F\r", ":A 0")
            .expect("M F=15.000000\r", ":A");
        let mut stage = AsiZStage::new().with_transport(Box::new(t));
        stage
            .set_property("Axis", PropertyValue::String("F".into()))
            .unwrap();
        stage.initialize().unwrap();
        stage.set_position_um(1.5).unwrap();
    }

    #[test]
    fn initialized_axis_change_is_rejected_and_preserves_axis() {
        let t = MockTransport::new()
            .expect("W F\r", ":A 0")
            .expect("M F=10.000000\r", ":A");
        let mut stage = AsiZStage::new().with_transport(Box::new(t));
        stage
            .set_property("Axis", PropertyValue::String("F".into()))
            .unwrap();
        stage.initialize().unwrap();

        assert_eq!(
            stage
                .set_property("Axis", PropertyValue::String("P".into()))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            stage.get_property("Axis").unwrap(),
            PropertyValue::String("F".into())
        );
        stage.set_position_um(1.0).unwrap();
    }

    #[test]
    fn home_is_noop_like_upstream_single_axis_stage() {
        let t = MockTransport::new().expect("W P\r", ":A 0");
        let mut stage = AsiZStage::new().with_transport(Box::new(t));
        stage
            .set_property("Axis", PropertyValue::String("P".into()))
            .unwrap();
        stage.initialize().unwrap();
        stage.home().unwrap();
    }

    #[test]
    fn limits_are_unsupported() {
        assert_eq!(
            AsiZStage::new().get_limits().unwrap_err(),
            MmError::UnsupportedCommand
        );
    }

    #[test]
    fn step_size_property_matches_upstream_tenths_of_micron_units() {
        assert_eq!(
            AsiZStage::new().get_property("StepSize_um").unwrap(),
            PropertyValue::Float(0.1)
        );
    }
}
