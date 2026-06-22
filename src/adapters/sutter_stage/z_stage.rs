/// Sutter Instruments MPC-200 Z stage (single axis).
///
/// Protocol (TX `\r`, RX `\n`, `:A`/`:N`):
///   `MOVE Z=<n>\r`   → `:A`
///   `MOVREL Z=<n>\r` → `:A`
///   `WHERE Z\r`      → `:A <z>`
///   `HOME Z\r`       → `:A`
///   `HALT\r`         → `:A`
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::RefCell;

const STEPS_PER_UM: f64 = 10.0;
const AXIS_PROPERTY: &str = "SutterStageSingleAxisName";

pub struct SutterZStage {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    /// Which axis letter (X, Y, Z, R, T, F, A, B, C)
    axis: char,
    pos_um: f64,
}

impl SutterZStage {
    pub fn new(axis: char) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                AXIS_PROPERTY,
                PropertyValue::String(axis.to_string()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values(
                AXIS_PROPERTY,
                &["X", "Y", "Z", "R", "T", "F", "A", "B", "C"],
            )
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            axis,
            pos_um: 0.0,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(RefCell::new(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_ref() {
            Some(t) => f(t.borrow_mut().as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let c = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&c)?;
            Ok(r.trim().to_string())
        })
    }

    fn check_a(resp: &str) -> MmResult<&str> {
        let s = resp.trim();
        if let Some(rest) = s.strip_prefix(":A") {
            Ok(rest.trim())
        } else {
            Err(MmError::LocallyDefined(format!("Sutter error: {}", s)))
        }
    }

    fn get_position_steps(&self) -> MmResult<i64> {
        let r = self.cmd(&format!("WHERE {}", self.axis))?;
        let body = Self::check_a(&r)?;
        body.split_whitespace()
            .next()
            .ok_or_else(|| MmError::LocallyDefined(format!("Cannot parse WHERE: {}", r)))?
            .parse()
            .map_err(|_| MmError::LocallyDefined(format!("Cannot parse WHERE: {}", r)))
    }

    fn set_high_command_level(&self) -> MmResult<()> {
        self.call_transport(|t| t.send_bytes(&[255, 65]))
    }

    fn set_step_size(&mut self, val: PropertyValue) -> MmResult<()> {
        let step_size = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
        if step_size <= 0.0 {
            return Err(MmError::InvalidPropertyValue);
        }
        self.props.set("StepSize", PropertyValue::Float(step_size))
    }

    fn autofocus(&self, val: PropertyValue) -> MmResult<()> {
        let param = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
        let resp = self.cmd(&format!("AF Z={}", param))?;
        Self::check_a(&resp)?;
        Ok(())
    }
}

impl Default for SutterZStage {
    fn default() -> Self {
        Self::new('Z')
    }
}

impl Device for SutterZStage {
    fn name(&self) -> &str {
        "Stage"
    }
    fn description(&self) -> &str {
        "SutterStage stage driver adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.set_high_command_level()?;
        if !self.props.has_property("StepSize") {
            self.props
                .define_property("StepSize", PropertyValue::Float(1.0), false)?;
        }
        if self.axis == 'Z' && !self.props.has_property("Autofocus") {
            self.props
                .define_property("Autofocus", PropertyValue::Integer(5), false)?;
        }
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
        if name == AXIS_PROPERTY {
            let axis = val.as_str();
            if axis.len() != 1 {
                return Err(MmError::InvalidPropertyValue);
            }
            self.props.set(name, val.clone())?;
            self.axis = axis.chars().next().unwrap();
            return Ok(());
        }
        match name {
            "StepSize" => self.set_step_size(val),
            "Autofocus" => {
                self.autofocus(val.clone())?;
                self.props.set(name, val)
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
        match self.cmd(&format!("STATUS {}", self.axis)) {
            Ok(resp) => resp.as_bytes().first() == Some(&b'B'),
            Err(_) => false,
        }
    }
}

impl Stage for SutterZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = (z * STEPS_PER_UM).round() as i64;
        let r = self.cmd(&format!("MOVE {}={}", self.axis, steps))?;
        Self::check_a(&r)?;
        self.pos_um = z;
        Ok(())
    }
    fn get_position_um(&self) -> MmResult<f64> {
        Ok(self.get_position_steps()? as f64 / STEPS_PER_UM)
    }
    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let steps = (dz * STEPS_PER_UM).round() as i64;
        let r = self.cmd(&format!("MOVREL {}={}", self.axis, steps))?;
        Self::check_a(&r)?;
        self.pos_um += dz;
        Ok(())
    }
    fn home(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd("HALT");
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
    fn initialize_z() {
        let t = MockTransport::new().expect("WHERE Z\r", ":A 1000"); // 100 µm
        let mut s = SutterZStage::new('Z').with_transport(Box::new(t));
        assert!(!s.has_property("StepSize"));
        assert!(!s.has_property("Autofocus"));
        s.initialize().unwrap();
        assert!(s.has_property("StepSize"));
        assert!(s.has_property("Autofocus"));
        assert!((s.get_position_um().unwrap() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new()
            .any(":A")
            .expect("WHERE Z\r", ":A 5000");
        let mut s = SutterZStage::new('Z').with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(500.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 500.0);
    }

    #[test]
    fn axis_r() {
        // R axis works the same way
        let t = MockTransport::new().expect("WHERE R\r", ":A 0");
        let mut s = SutterZStage::new('R').with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.has_property("StepSize"));
        assert!(!s.has_property("Autofocus"));
        assert_eq!(s.get_position_um().unwrap(), 0.0);
    }

    #[test]
    fn upstream_axis_property_drives_selected_serial_axis() {
        let t = MockTransport::new()
            .expect("MOVE R=125\r", ":A")
            .expect("WHERE R\r", ":A 125");
        let mut s = SutterZStage::new('Z').with_transport(Box::new(t));
        s.set_property(AXIS_PROPERTY, PropertyValue::String("R".into()))
            .unwrap();
        s.initialize().unwrap();
        s.set_position_um(12.5).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 12.5);
    }

    #[test]
    fn upstream_axis_property_rejects_unknown_axis() {
        let mut s = SutterZStage::new('Z');
        assert_eq!(
            s.set_property(AXIS_PROPERTY, PropertyValue::String("Q".into())),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            s.get_property(AXIS_PROPERTY).unwrap(),
            PropertyValue::String("Z".into())
        );
    }

    #[test]
    fn no_transport_error() {
        assert!(SutterZStage::new('Z').initialize().is_err());
    }

    #[test]
    fn limits_are_unsupported() {
        assert_eq!(
            SutterZStage::new('Z').get_limits(),
            Err(MmError::UnsupportedCommand)
        );
    }

    #[test]
    fn home_is_unsupported_like_upstream_single_axis_stage() {
        assert_eq!(
            SutterZStage::new('Z').home(),
            Err(MmError::UnsupportedCommand)
        );
    }

    #[test]
    fn busy_polls_axis_status() {
        let t = MockTransport::new().expect("STATUS Z\r", "B");
        let mut s = SutterZStage::new('Z').with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
    }

    #[test]
    fn relative_move_uses_movrel_for_single_axis() {
        let t = MockTransport::new()
            .expect("MOVREL Z=-250\r", ":A")
            .expect("WHERE Z\r", ":A 750");
        let mut s = SutterZStage::new('Z').with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(-25.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 75.0);
    }

    #[test]
    fn malformed_where_errors_instead_of_zeroing_position() {
        let t = MockTransport::new().expect("WHERE Z\r", ":A bad");
        let mut s = SutterZStage::new('Z').with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.get_position_um().is_err());
    }

    #[test]
    fn step_size_rejects_non_positive_values_without_cache_drift() {
        let mut s = SutterZStage::new('Z');
        s.props
            .define_property("StepSize", PropertyValue::Float(1.0), false)
            .unwrap();
        assert_eq!(
            s.set_property("StepSize", PropertyValue::Float(-0.5)),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            s.get_property("StepSize").unwrap(),
            PropertyValue::Float(1.0)
        );
    }

    #[test]
    fn autofocus_property_sends_upstream_z_command() {
        let t = MockTransport::new().expect("AF Z=7\r", ":A");
        let mut s = SutterZStage::new('Z').with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("Autofocus", PropertyValue::Integer(7))
            .unwrap();
        assert_eq!(
            s.get_property("Autofocus").unwrap(),
            PropertyValue::Integer(7)
        );
    }
}
