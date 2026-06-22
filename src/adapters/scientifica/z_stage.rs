/// Scientifica SliceScope / MotionSite Z stage.
///
/// Protocol (same as XY, newline-terminated):
///   `absz <steps>\r`  → "A\r\n" (success)
///   `PZ\r`            → "<integer steps>"
///   `home\r`          → "A\r\n"
///   `stop\r`          → "A\r\n"
///
/// Step size: 0.1 µm / step.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::{Cell, RefCell};

const STEPS_PER_UM: f64 = 10.0;

pub struct ScientificaZStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    pos_um: Cell<f64>,
}

impl ScientificaZStage {
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
        let c = format!("{}\r", command);
        self.call_transport(|t| {
            t.purge()?;
            let r = t.send_recv(&c)?;
            Ok(r.trim().to_string())
        })
    }

    fn check_ok(resp: &str) -> MmResult<()> {
        if resp.starts_with('A') {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!(
                "Scientifica error: {}",
                resp
            )))
        }
    }

    fn query_steps(&self) -> MmResult<i64> {
        let r = self.cmd("PZ")?;
        if r.len() > 2 && r.starts_with('E') {
            return Err(MmError::LocallyDefined(format!(
                "Scientifica Z error: {}",
                r
            )));
        }
        if r.is_empty() {
            return Err(MmError::LocallyDefined(
                "Scientifica bad Z position: ".into(),
            ));
        }
        r.trim()
            .parse()
            .map_err(|_| MmError::LocallyDefined(format!("Scientifica bad Z position: {}", r)))
    }

    pub fn set_origin(&mut self) -> MmResult<()> {
        let r = self.cmd("pz 0 ")?;
        Self::check_ok(&r)?;
        self.pos_um.set(0.0);
        Ok(())
    }
}

impl Default for ScientificaZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ScientificaZStage {
    fn name(&self) -> &str {
        "ScientificaZStage"
    }
    fn description(&self) -> &str {
        "Scientifica Z stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        let steps = self.query_steps()?;
        self.pos_um.set(steps as f64 / STEPS_PER_UM);
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
        self.props.set(name, val)
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
        match self.cmd("s") {
            Ok(answer) if !answer.is_empty() => answer
                .chars()
                .next()
                .and_then(|c| c.to_digit(10))
                .map_or(false, |status| status != 0),
            _ => false,
        }
    }
}

impl Stage for ScientificaZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = (z * STEPS_PER_UM).round() as i64;
        let r = self.cmd(&format!("absz {}", steps))?;
        Self::check_ok(&r)?;
        self.pos_um.set(z);
        Ok(())
    }
    fn get_position_um(&self) -> MmResult<f64> {
        let steps = self.query_steps()?;
        let pos = steps as f64 / STEPS_PER_UM;
        self.pos_um.set(pos);
        Ok(pos)
    }
    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let new_z = self.pos_um.get() + dz;
        self.set_position_um(new_z)
    }
    fn home(&mut self) -> MmResult<()> {
        Err(MmError::LocallyDefined(
            "Scientifica Z home not supported".into(),
        ))
    }
    fn stop(&mut self) -> MmResult<()> {
        let r = self.cmd("STOP")?;
        Self::check_ok(&r)
    }
    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Err(MmError::LocallyDefined(
            "Scientifica Z limits not supported".into(),
        ))
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
    fn initialize() {
        let t = MockTransport::new()
            .expect("PZ\r", "500")
            .expect("PZ\r", "500");
        let mut s = ScientificaZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new()
            .expect("PZ\r", "0")
            .expect("absz 1000\r", "A")
            .expect("PZ\r", "1000");
        let mut s = ScientificaZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 100.0);
    }

    #[test]
    fn move_relative() {
        let t = MockTransport::new()
            .expect("PZ\r", "1000")
            .expect("absz 1500\r", "A")
            .expect("PZ\r", "1500");
        let mut s = ScientificaZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(50.0).unwrap();
        assert!((s.get_position_um().unwrap() - 150.0).abs() < 1e-9);
    }

    #[test]
    fn error_response_fails() {
        let t = MockTransport::new().any("0").any("E: limit");
        let mut s = ScientificaZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_position_um(999_999.0).is_err());
    }

    #[test]
    fn unsupported_home_and_limits() {
        let t = MockTransport::new().any("0");
        let mut s = ScientificaZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.home().is_err());
        assert!(s.get_limits().is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(ScientificaZStage::new().initialize().is_err());
    }

    #[test]
    fn malformed_position_response_fails_initialize() {
        let t = MockTransport::new().any("not-a-number");
        let mut s = ScientificaZStage::new().with_transport(Box::new(t));
        assert!(s.initialize().is_err());
    }

    #[test]
    fn busy_queries_status_command() {
        let t = MockTransport::new().expect("s\r", "1");
        let s = ScientificaZStage::new().with_transport(Box::new(t));
        assert!(s.busy());
    }

    #[test]
    fn set_origin_sends_upstream_zero_command() {
        let t = MockTransport::new()
            .expect("PZ\r", "500")
            .expect("pz 0 \r", "A")
            .expect("PZ\r", "0");
        let mut s = ScientificaZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_origin().unwrap();
        assert_eq!(s.get_position_um().unwrap(), 0.0);
    }

    #[test]
    fn initialized_port_change_is_rejected_and_preserves_value() {
        let t = MockTransport::new().expect("PZ\r", "0");
        let mut s = ScientificaZStage::new().with_transport(Box::new(t));
        s.set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        s.initialize().unwrap();
        assert!(s
            .set_property("Port", PropertyValue::String("COM2".into()))
            .is_err());
        assert_eq!(
            s.get_property("Port").unwrap(),
            PropertyValue::String("COM1".into())
        );
    }
}
