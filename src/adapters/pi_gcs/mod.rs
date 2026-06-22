/// Physik Instrumente (PI) GCS (General Command Set) Z-stage adapter.
///
/// Protocol (TX `\n`, RX `\n`):
///   `SVO A 1\n`        → enable servo for axis A
///   `MOV A {pos}\n`    → move to absolute position in µm
///   `POS? A\n`         → query position; response `A={value}\n`
///   `ERR?\n`           → query last error code; 0 = success
///
/// Step size: 0.01 µm default. Axis name configurable (default "A").
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::{Cell, RefCell};

pub struct PiGcsZStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    pos_um: Cell<f64>,
    check_is_moving: Cell<bool>,
    axis: String,
    step_size_um: f64,
    limit_um: f64,
}

impl PiGcsZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Axis", PropertyValue::String("A".into()), false)
            .unwrap();
        props
            .define_property("Limit_um", PropertyValue::Float(500.0), false)
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            pos_um: Cell::new(0.0),
            check_is_moving: Cell::new(true),
            axis: "A".into(),
            step_size_um: 0.01,
            limit_um: 500.0,
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

    fn send(&self, command: &str) -> MmResult<String> {
        let c = format!("{}\n", command);
        self.call_transport(|t| Ok(t.send_recv(&c)?.trim().to_string()))
    }

    /// Parse a GCS response that may contain `key=value`; extract value after last `=`.
    fn parse_value(resp: &str) -> MmResult<f64> {
        let part = resp.rfind('=').map(|i| &resp[i + 1..]).unwrap_or(resp);
        part.trim()
            .parse::<f64>()
            .map_err(|_| MmError::LocallyDefined(format!("PI GCS parse error: '{}'", resp)))
    }

    fn check_error(&self) -> MmResult<()> {
        let resp = self.send("ERR?")?;
        let code: i32 = resp.trim().parse().unwrap_or(-1);
        if code == 0 {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!(
                "PI GCS error code: {}",
                code
            )))
        }
    }
}

impl Default for PiGcsZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for PiGcsZStage {
    fn name(&self) -> &str {
        "PIZStage"
    }
    fn description(&self) -> &str {
        "Physik Instrumente (PI) GCS Adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        self.check_is_moving.set(true);
        let _ = self.busy();
        let axis = self.axis.clone();
        // Enable servo
        self.send(&format!("SVO {} 1", axis))?;
        // Query current position
        let resp = self.send(&format!("POS? {}", axis))?;
        self.pos_um.set(Self::parse_value(&resp)?);
        if !self.props.has_property("StepSizeUm") {
            self.props
                .define_property("StepSizeUm", PropertyValue::Float(0.01), false)?;
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
        if name == "Port" && self.initialized {
            return Err(MmError::CanNotSetProperty);
        }

        self.props.set(name, val.clone())?;

        if name == "Axis" {
            if let PropertyValue::String(ref s) = val {
                self.axis = s.clone();
            }
        } else if name == "StepSizeUm" {
            if let PropertyValue::Float(f) = val {
                self.step_size_um = f;
            }
        } else if name == "Limit_um" {
            if let PropertyValue::Float(f) = val {
                self.limit_um = f;
            }
        }
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
        if !self.check_is_moving.get() {
            return false;
        }
        let result = self.call_transport(|t| {
            t.send("\x05")?;
            t.receive_line()
        });
        match result {
            Ok(answer) => Self::parse_value(&answer)
                .map(|moving| moving != 0.0)
                .unwrap_or(false),
            Err(_) => {
                let _ = self.check_error();
                let _ = self.check_error();
                self.check_is_moving.set(false);
                false
            }
        }
    }
}

impl Stage for PiGcsZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let axis = self.axis.clone();
        self.send(&format!("MOV {} {}", axis, z))?;
        self.check_error()?;
        self.pos_um.set(z);
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        let axis = self.axis.clone();
        let resp = self.send(&format!("POS? {}", axis))?;
        let pos = Self::parse_value(&resp)?;
        self.pos_um.set(pos);
        Ok(pos)
    }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let target = self.get_position_um()? + dz;
        self.set_position_um(target)
    }

    fn home(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn stop(&mut self) -> MmResult<()> {
        let axis = self.axis.clone();
        let _ = self.send(&format!("STP {}", axis));
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
    fn initialize_reads_position() {
        // SVO -> empty ok, POS? -> A=50.0 um
        let t = MockTransport::new()
            .any("0")
            .any("")
            .any("A=50.0")
            .any("A=50.0");
        let mut s = PiGcsZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new()
            .any("0")
            .any("")
            .any("A=0.0")
            .any("")
            .any("0")
            .any("A=100.0");
        let mut s = PiGcsZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
        assert!((s.get_position_um().unwrap() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn move_relative() {
        let t = MockTransport::new()
            .any("0")
            .any("")
            .any("A=0.0")
            .any("A=0.0")
            .any("")
            .any("0")
            .any("A=25.0");
        let mut s = PiGcsZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(25.0).unwrap();
        assert!((s.get_position_um().unwrap() - 25.0).abs() < 1e-6);
    }

    #[test]
    fn error_code_fails() {
        let t = MockTransport::new()
            .any("0")
            .any("")
            .any("A=0.0")
            .any("")
            .any("5");
        let mut s = PiGcsZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_position_um(10.0).is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(PiGcsZStage::new().initialize().is_err());
    }

    #[test]
    fn upstream_property_surface_and_unsupported_stage_methods() {
        let mut s = PiGcsZStage::new();
        assert!(s.has_property("Limit_um"));
        assert!(!s.has_property("LimitUm"));
        assert!(!s.has_property("StepSizeUm"));
        assert_eq!(s.description(), "Physik Instrumente (PI) GCS Adapter");
        assert_eq!(s.home(), Err(MmError::UnsupportedCommand));
        assert_eq!(s.get_limits(), Err(MmError::UnsupportedCommand));
    }

    #[test]
    fn initialized_port_change_is_rejected() {
        let t = MockTransport::new().any("0").any("").any("A=0.0");
        let mut s = PiGcsZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.set_property("Port", PropertyValue::String("COM2".into())),
            Err(MmError::CanNotSetProperty)
        );
        assert_eq!(
            s.get_property("Port").unwrap(),
            PropertyValue::String("Undefined".into())
        );
    }

    #[test]
    fn step_size_is_created_only_after_successful_initialize() {
        let mut no_transport = PiGcsZStage::new();
        assert_eq!(
            no_transport.set_property("StepSizeUm", PropertyValue::Float(0.2)),
            Err(MmError::UnknownLabel("StepSizeUm".into()))
        );
        assert!(!no_transport.has_property("StepSizeUm"));

        let t = MockTransport::new().any("0").any("").any("A=0.0");
        let mut s = PiGcsZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.get_property("StepSizeUm").unwrap(),
            PropertyValue::Float(0.01)
        );
        s.set_property("StepSizeUm", PropertyValue::Float(0.2))
            .unwrap();
        assert_eq!(
            s.get_property("StepSizeUm").unwrap(),
            PropertyValue::Float(0.2)
        );
    }

    #[test]
    fn busy_polls_upstream_moving_byte() {
        let t = MockTransport::new().expect("\x05", "1").expect("\x05", "0");
        let s = PiGcsZStage::new().with_transport(Box::new(t));
        assert!(s.busy());
        assert!(!s.busy());
    }

    #[test]
    fn busy_disables_poll_after_unsupported_controller_timeout() {
        let t = MockTransport::new();
        let s = PiGcsZStage::new().with_transport(Box::new(t));
        assert!(!s.busy());
        assert!(!s.busy());
    }
}
