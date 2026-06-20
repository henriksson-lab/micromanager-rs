/// Conix Research XYZ controller Z stage.
///
/// Protocol (TX `\r`, RX `\r`):
///   `W Z\r`       → `:A <z>`  (current position in µm)
///   `M Z<z>\r`    → `:A`      (move to absolute position in µm)
///   `H\r`         → `:A`      (set origin)
///   `\\r`         → `:A`      (halt; backslash + CR)
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::RefCell;

fn check_a(resp: &str) -> MmResult<&str> {
    let s = resp.trim();
    if let Some(rest) = s.strip_prefix(":A") {
        Ok(rest.trim())
    } else {
        Err(MmError::LocallyDefined(format!("Conix error: {}", s)))
    }
}

pub struct ConixZStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    pos_um: f64,
    controller_is_rfa: bool,
    step_size_um: f64,
}

impl ConixZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            pos_um: 0.0,
            controller_is_rfa: false,
            step_size_um: 0.1,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        *self.transport.get_mut() = Some(t);
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
        self.call_transport(|t| {
            let r = t.send_recv(&c)?;
            Ok(r.trim().to_string())
        })
    }

    fn query_position_um(&self) -> MmResult<f64> {
        let r = if self.controller_is_rfa {
            self.cmd("WZ")?
        } else {
            self.cmd("W Z")?
        };
        let body = check_a(&r)?;
        let value = body
            .split_whitespace()
            .next()
            .ok_or_else(|| MmError::LocallyDefined(format!("Cannot parse Z position: {}", r)))?
            .parse::<f64>()
            .map_err(|_| MmError::LocallyDefined(format!("Cannot parse Z position: {}", r)))?;
        Ok(if self.controller_is_rfa {
            value * self.step_size_um
        } else {
            value
        })
    }

    fn poll_busy(&self) -> MmResult<bool> {
        if self.controller_is_rfa {
            return Ok(false);
        }
        let r = self.cmd("/")?;
        Ok(r.trim_start().starts_with('B'))
    }
}

impl Default for ConixZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ConixZStage {
    fn name(&self) -> &str {
        "ConixZStage"
    }
    fn description(&self) -> &str {
        "Conix Z stage driver"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        let who = self.cmd("WHO")?;
        let version = check_a(&who)?;
        self.controller_is_rfa = !version.to_ascii_uppercase().contains("XYZ");
        if !self.controller_is_rfa {
            let r = self.cmd("COMUNITS UM")?;
            check_a(&r)?;
        } else if !self.props.has_property("StepSizeUm") {
            self.props.define_property(
                "StepSizeUm",
                PropertyValue::Float(self.step_size_um),
                false,
            )?;
        }
        self.pos_um = self.query_position_um()?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "StepSizeUm" => Ok(PropertyValue::Float(self.step_size_um)),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "StepSizeUm" => {
                if !self.controller_is_rfa {
                    return Err(MmError::UnsupportedCommand);
                }
                let step_size = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.step_size_um = step_size;
                self.props.set(name, PropertyValue::Float(step_size))
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
        self.poll_busy().unwrap_or(false)
    }
}

impl Stage for ConixZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let r = if self.controller_is_rfa {
            self.cmd(&format!("MZ {}", (z / self.step_size_um).round() as i64))?
        } else {
            self.cmd(&format!("M Z{}", z))?
        };
        check_a(&r)?;
        self.pos_um = z;
        Ok(())
    }
    fn get_position_um(&self) -> MmResult<f64> {
        self.query_position_um()
    }
    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        self.set_position_um(self.pos_um + dz)
    }
    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("! Z")?;
        check_a(&r)?;
        self.pos_um = 0.0;
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> {
        let r = self.cmd("\\")?;
        if r.trim() != ":N-21" {
            check_a(&r)?;
        }
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
    fn initialize() {
        let t = MockTransport::new()
            .expect("WHO\r", ":A XYZ v1.5")
            .expect("COMUNITS UM\r", ":A")
            .expect("W Z\r", ":A 150.5")
            .expect("W Z\r", ":A 150.5");
        let mut s = ConixZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 150.5).abs() < 1e-6);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new()
            .expect("WHO\r", ":A XYZ v1.5")
            .expect("COMUNITS UM\r", ":A")
            .expect("W Z\r", ":A 0")
            .expect("M Z500\r", ":A")
            .expect("W Z\r", ":A 500");
        let mut s = ConixZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(500.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 500.0);
    }

    #[test]
    fn move_relative() {
        let t = MockTransport::new()
            .expect("WHO\r", ":A XYZ v1.5")
            .expect("COMUNITS UM\r", ":A")
            .expect("W Z\r", ":A 100")
            .expect("M Z150\r", ":A")
            .expect("W Z\r", ":A 150");
        let mut s = ConixZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(50.0).unwrap();
        assert!((s.get_position_um().unwrap() - 150.0).abs() < 1e-6);
    }

    #[test]
    fn home_uses_z_home_command() {
        let t = MockTransport::new()
            .expect("WHO\r", ":A XYZ v1.5")
            .expect("COMUNITS UM\r", ":A")
            .expect("W Z\r", ":A 100")
            .expect("! Z\r", ":A");
        let mut s = ConixZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert_eq!(s.pos_um, 0.0);
    }

    #[test]
    fn stop_accepts_halt_while_moving_response() {
        let t = MockTransport::new()
            .expect("WHO\r", ":A XYZ v1.5")
            .expect("COMUNITS UM\r", ":A")
            .expect("W Z\r", ":A 100")
            .expect("\\\r", ":N-21");
        let mut s = ConixZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.stop().unwrap();
    }

    #[test]
    fn no_transport_error() {
        assert!(ConixZStage::new().initialize().is_err());
    }

    #[test]
    fn busy_polls_status_for_xyz_controller() {
        let t = MockTransport::new()
            .expect("WHO\r", ":A XYZ v1.5")
            .expect("COMUNITS UM\r", ":A")
            .expect("W Z\r", ":A 100")
            .expect("/\r", "B");
        let mut s = ConixZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
    }

    #[test]
    fn rfa_controller_uses_step_position_path() {
        let t = MockTransport::new()
            .expect("WHO\r", ":A RFA v1")
            .expect("WZ\r", ":A 123")
            .expect("MZ 150\r", ":A")
            .expect("WZ\r", ":A 150");
        let mut s = ConixZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.pos_um - 12.3).abs() < 1e-9);
        s.set_position_um(15.0).unwrap();
        assert!((s.get_position_um().unwrap() - 15.0).abs() < 1e-9);
    }

    #[test]
    fn rfa_controller_exposes_step_size_property() {
        let t = MockTransport::new()
            .expect("WHO\r", ":A RFA v1")
            .expect("WZ\r", ":A 123")
            .expect("MZ 62\r", ":A")
            .expect("WZ\r", ":A 62");
        let mut s = ConixZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.get_property("StepSizeUm").unwrap(),
            PropertyValue::Float(0.1)
        );
        s.set_property("StepSizeUm", PropertyValue::Float(0.2))
            .unwrap();
        s.set_position_um(12.4).unwrap();
        assert!((s.get_position_um().unwrap() - 12.4).abs() < 1e-9);
    }

    #[test]
    fn initialized_port_change_is_rejected() {
        let t = MockTransport::new()
            .expect("WHO\r", ":A XYZ v1.5")
            .expect("COMUNITS UM\r", ":A")
            .expect("W Z\r", ":A 100");
        let mut s = ConixZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s
            .set_property("Port", PropertyValue::String("COM2".to_string()))
            .is_err());
        assert_eq!(
            s.get_property("Port").unwrap(),
            PropertyValue::String("Undefined".to_string())
        );
    }
}
