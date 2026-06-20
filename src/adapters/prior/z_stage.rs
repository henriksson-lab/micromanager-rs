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

const DEFAULT_STEPS_PER_UM: f64 = 10.0;

pub struct PriorZStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    steps_per_um: f64,
    pos_um: f64,
}

impl PriorZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_property("StepSize_um", PropertyValue::Float(0.1), true)
            .unwrap();
        props
            .define_property("MaxSpeed", PropertyValue::Integer(20), false)
            .unwrap();
        props.set_property_limits("MaxSpeed", 1.0, 100.0).unwrap();
        props
            .define_property("Acceleration", PropertyValue::Integer(20), false)
            .unwrap();
        props
            .set_property_limits("Acceleration", 1.0, 100.0)
            .unwrap();
        props
            .define_property("SCurve", PropertyValue::Integer(20), false)
            .unwrap();
        props.set_property_limits("SCurve", 1.0, 100.0).unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            steps_per_um: DEFAULT_STEPS_PER_UM,
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

    fn check_zero(resp: &str, context: &str) -> MmResult<()> {
        let s = resp.trim();
        if s.starts_with('0') {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!(
                "Prior Z {} error: {}",
                context, s
            )))
        }
    }

    fn query_bounded_i64_property(&self, command: &str) -> MmResult<PropertyValue> {
        self.clear_port()?;
        let value = self
            .cmd(command)?
            .trim()
            .parse::<i64>()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        if !(1..=100).contains(&value) {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(PropertyValue::Integer(value))
    }

    fn clear_port(&self) -> MmResult<()> {
        self.call_transport(|t| t.purge())
    }

    fn discover_resolution(&mut self) {
        if let Ok(resp) = self.cmd("RES,Z") {
            if let Ok(res) = resp.trim().parse::<f64>() {
                if res > 0.0 {
                    self.steps_per_um = 1.0 / res;
                    if let Some(e) = self.props.entry_mut("StepSize_um") {
                        e.value = PropertyValue::Float(res);
                    }
                }
            }
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
        self.clear_port()?;
        self.cmd("COMP 0")?;
        self.discover_resolution();
        let r = self.cmd("PZ")?;
        let steps: i64 = r.trim().parse().unwrap_or(0);
        self.pos_um = steps as f64 / self.steps_per_um;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if self.initialized {
            match name {
                "MaxSpeed" => return self.query_bounded_i64_property("SMZ"),
                "Acceleration" => return self.query_bounded_i64_property("SAZ"),
                "SCurve" => return self.query_bounded_i64_property("SCZ"),
                _ => {}
            }
        }
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "MaxSpeed" if self.initialized => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(v))?;
                self.clear_port()?;
                Self::check_zero(&self.cmd(&format!("SMZ,{}", v))?, name)
            }
            "Acceleration" if self.initialized => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(v))?;
                self.clear_port()?;
                Self::check_zero(&self.cmd(&format!("SAZ,{}", v))?, name)
            }
            "SCurve" if self.initialized => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(v))?;
                self.clear_port()?;
                Self::check_zero(&self.cmd(&format!("SCZ,{}", v))?, name)
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
        self.cmd("$")
            .ok()
            .and_then(|s| s.trim().parse::<u8>().ok())
            .map(|status| status & 0x04 != 0)
            .unwrap_or(false)
    }
}

impl Stage for PriorZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let target_steps = (z * self.steps_per_um).round() as i64;
        let current_steps = (self.pos_um * self.steps_per_um).round() as i64;
        let delta = target_steps - current_steps;
        let cmd = if delta >= 0 {
            format!("U,{}", delta)
        } else {
            format!("D,{}", -delta)
        };
        let r = self.cmd(&cmd)?;
        Self::check_r(&r)?;
        self.pos_um = target_steps as f64 / self.steps_per_um;
        Ok(())
    }
    fn get_position_um(&self) -> MmResult<f64> {
        let r = self.cmd("PZ")?;
        let steps: i64 = r
            .trim()
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        Ok(steps as f64 / self.steps_per_um)
    }
    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let steps = (dz.abs() * self.steps_per_um).round() as i64;
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
            .expect("RES,Z\r", "0.1")
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
            .expect("RES,Z\r", "0.1")
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
            .expect("RES,Z\r", "0.1")
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
            .expect("RES,Z\r", "0.1")
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
            .expect("RES,Z\r", "0.1")
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
    fn unsupported_home_and_stop() {
        let mut s = PriorZStage::new();
        assert_eq!(s.home().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(s.stop().unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn limits_are_unsupported_like_upstream() {
        let s = PriorZStage::new();
        assert_eq!(s.get_limits().unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn z_motion_properties_use_z_commands_zero_ack_and_live_reads() {
        let t = MockTransport::new()
            .expect("COMP 0\r", "0")
            .expect("RES,Z\r", "0.1")
            .expect("PZ\r", "0")
            .expect("SMZ,41\r", "0")
            .expect("SMZ\r", "41")
            .expect("SAZ,42\r", "0")
            .expect("SAZ\r", "42")
            .expect("SCZ,43\r", "0")
            .expect("SCZ\r", "43");
        let mut s = PriorZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("MaxSpeed", PropertyValue::Integer(41))
            .unwrap();
        assert_eq!(
            s.get_property("MaxSpeed").unwrap(),
            PropertyValue::Integer(41)
        );
        s.set_property("Acceleration", PropertyValue::Integer(42))
            .unwrap();
        assert_eq!(
            s.get_property("Acceleration").unwrap(),
            PropertyValue::Integer(42)
        );
        s.set_property("SCurve", PropertyValue::Integer(43))
            .unwrap();
        assert_eq!(
            s.get_property("SCurve").unwrap(),
            PropertyValue::Integer(43)
        );
    }
}
