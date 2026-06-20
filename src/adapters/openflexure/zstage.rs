//! OpenFlexure Z Stage — relative moves via `mrz <steps>`.

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::types::{DeviceType, FocusDirection, PropertyValue};

use super::xystage::Commander;

pub struct OfZStage {
    props: PropertyMap,
    initialized: bool,
    steps_z: i64,
    step_size_um: f64,
    commander: Option<Commander>,
}

impl OfZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("StepSizeUm", PropertyValue::Float(0.05), false)
            .unwrap();
        Self {
            props,
            initialized: false,
            steps_z: 0,
            step_size_um: 0.05,
            commander: None,
        }
    }

    pub fn with_commander(mut self, c: Commander) -> Self {
        self.commander = Some(c);
        self
    }

    fn send(&self, cmd: &str) -> MmResult<String> {
        let c = self.commander.as_ref().ok_or(MmError::NotConnected)?;
        c(cmd)
    }

    pub fn sync_state(&mut self) -> MmResult<()> {
        let resp = self.send("p")?;
        let mut parts = resp.split_whitespace();
        let _x: i64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let _y: i64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let z: i64 = parts
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(self.steps_z);
        self.steps_z = z;
        Ok(())
    }

    pub fn set_origin(&mut self) -> MmResult<()> {
        self.send("zero")?;
        self.sync_state()?;
        Ok(())
    }
}

impl Default for OfZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for OfZStage {
    fn name(&self) -> &str {
        "OFZStage"
    }
    fn description(&self) -> &str {
        "OpenFlexure Z stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.commander.is_none() {
            return Err(MmError::CommHubMissing);
        }
        let response = self.send("blocking_moves false")?;
        if !response.contains("done") {
            return Err(MmError::SerialInvalidResponse);
        }
        self.sync_state()?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send("release");
            self.initialized = false;
        }
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
        false
    }
}

impl Stage for OfZStage {
    fn set_position_um(&mut self, _pos: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn get_position_um(&self) -> MmResult<f64> {
        Ok(self.steps_z as f64 * self.step_size_um)
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        if !self.initialized {
            return Err(MmError::NotConnected);
        }
        let delta_steps = (d / self.step_size_um).round() as i64;
        if delta_steps != 0 {
            self.send(&format!("mrz {}", delta_steps))?;
        }
        self.steps_z += delta_steps;
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
    use std::sync::{Arc, Mutex};

    fn make_stage() -> (OfZStage, Arc<Mutex<Vec<String>>>) {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let log_for_return = Arc::clone(&log);
        let commander: Commander = Arc::new(move |cmd| {
            log.lock().unwrap().push(cmd.to_string());
            if cmd == "blocking_moves false" {
                Ok("done".to_string())
            } else if cmd == "p" {
                Ok("0 0 0".to_string())
            } else {
                Ok("ok".to_string())
            }
        });
        (OfZStage::new().with_commander(commander), log_for_return)
    }

    #[test]
    fn relative_move() {
        let (mut stage, _) = make_stage();
        stage.initialize().unwrap();
        stage.set_relative_position_um(10.0).unwrap();
        let pos = stage.get_position_um().unwrap();
        assert!((pos - 10.0).abs() < 0.1);
    }

    #[test]
    fn set_origin_sends_zero_and_resyncs_cached_position() {
        let (mut stage, log) = make_stage();
        stage.initialize().unwrap();
        stage.set_relative_position_um(10.0).unwrap();

        stage.set_origin().unwrap();

        let pos = stage.get_position_um().unwrap();
        assert!(pos.abs() < 0.1);
        assert!(log.lock().unwrap().iter().any(|cmd| cmd == "zero"));
        assert!(log.lock().unwrap().iter().filter(|cmd| *cmd == "p").count() >= 2);
    }
}
