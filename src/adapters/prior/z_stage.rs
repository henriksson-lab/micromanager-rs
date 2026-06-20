/// Prior Scientific ProScan Z stage.
///
/// Protocol (TX `\r`, RX `\r`):
///   `PZ\r`          → Z position in steps
///   `U,steps\r`     → move Z up (positive relative) in steps; response `R\r`
///   `D,steps\r`     → move Z down (negative relative) in steps; response `R\r`
///
/// Step size: 0.1 µm / step.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::RefCell;

const STEPS_PER_UM: f64 = 10.0;

pub struct PriorZStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    pos_um: f64,
}

impl PriorZStage {
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
            let r = t.send_recv(&c)?;
            Ok(r.trim().to_string())
        })
    }

    fn check_r(resp: &str) -> MmResult<()> {
        if resp.trim() == "R" {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("Prior Z error: {}", resp)))
        }
    }
}

impl Default for PriorZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for PriorZStage {
    fn name(&self) -> &str {
        "PriorZStage"
    }
    fn description(&self) -> &str {
        "Prior Scientific ProScan Z stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        self.cmd("COMP 0")?;
        let r = self.cmd("PZ")?;
        let steps: i64 = r.trim().parse().unwrap_or(0);
        self.pos_um = steps as f64 / STEPS_PER_UM;
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
        self.cmd("$")
            .ok()
            .and_then(|s| s.trim().parse::<u8>().ok())
            .map(|status| status & 0x04 != 0)
            .unwrap_or(false)
    }
}

impl Stage for PriorZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let target_steps = (z * STEPS_PER_UM).round() as i64;
        let current_steps = (self.pos_um * STEPS_PER_UM).round() as i64;
        let delta = target_steps - current_steps;
        let cmd = if delta >= 0 {
            format!("U,{}", delta)
        } else {
            format!("D,{}", -delta)
        };
        let r = self.cmd(&cmd)?;
        Self::check_r(&r)?;
        self.pos_um = target_steps as f64 / STEPS_PER_UM;
        Ok(())
    }
    fn get_position_um(&self) -> MmResult<f64> {
        let r = self.cmd("PZ")?;
        let steps: i64 = r
            .trim()
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        Ok(steps as f64 / STEPS_PER_UM)
    }
    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let steps = (dz.abs() * STEPS_PER_UM).round() as i64;
        let cmd = if dz >= 0.0 {
            format!("U,{}", steps)
        } else {
            format!("D,{}", steps)
        };
        let r = self.cmd(&cmd)?;
        Self::check_r(&r)?;
        self.pos_um += dz;
        Ok(())
    }
    fn home(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
    fn stop(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
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
            .expect("COMP 0\r", "0")
            .expect("PZ\r", "500")
            .expect("PZ\r", "500"); // PZ -> 50 µm
        let mut s = PriorZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .expect("PZ\r", "0")
            .expect("U,1000\r", "R")
            .expect("PZ\r", "1000");
        let mut s = PriorZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 100.0);
    }

    #[test]
    fn move_absolute_down_uses_relative_delta() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .expect("PZ\r", "1000")
            .expect("D,300\r", "R")
            .expect("PZ\r", "700");
        let mut s = PriorZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(70.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 70.0);
    }

    #[test]
    fn move_up() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .expect("PZ\r", "0")
            .expect("U,250\r", "R")
            .expect("PZ\r", "250");
        let mut s = PriorZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(25.0).unwrap();
        assert!((s.get_position_um().unwrap() - 25.0).abs() < 1e-9);
    }

    #[test]
    fn move_down() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .expect("PZ\r", "1000")
            .expect("D,300\r", "R")
            .expect("PZ\r", "700"); // start at 100 µm
        let mut s = PriorZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(-30.0).unwrap();
        assert!((s.get_position_um().unwrap() - 70.0).abs() < 1e-9);
    }

    #[test]
    fn no_transport_error() {
        assert!(PriorZStage::new().initialize().is_err());
    }

    #[test]
    fn unsupported_home_and_limits() {
        let mut s = PriorZStage::new();
        assert_eq!(s.home().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(s.stop().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(s.get_limits().unwrap_err(), MmError::UnsupportedCommand);
    }
}
