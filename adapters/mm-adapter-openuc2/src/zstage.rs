//! UC2 Z Stage — absolute positioning via JSON motor command.
//! stepperid 3 = Z.

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

use crate::xystage::JsonWriter;

pub struct Uc2ZStage {
    props: PropertyMap,
    initialized: bool,
    pos_steps: i64,
    step_size_um: f64,
    writer: Option<JsonWriter>,
}

impl Uc2ZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("StepSizeUm", PropertyValue::Float(0.1), false).unwrap();
        Self {
            props,
            initialized: false,
            pos_steps: 0,
            step_size_um: 0.1,
            writer: None,
        }
    }

    pub fn with_writer(mut self, writer: JsonWriter) -> Self {
        self.writer = Some(writer);
        self
    }

    fn send(&self, cmd: &str) -> MmResult<String> {
        let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
        writer(cmd)
    }

    fn move_abs(&self, steps: i64) -> MmResult<()> {
        let cmd = format!(
            r#"{{"task":"/motor_act","motor":{{"steppers":[{{"stepperid":3,"position":{},"speed":2000,"isabs":1}}]}}}}"#,
            steps
        );
        self.send(&cmd)?;
        Ok(())
    }
}

impl Default for Uc2ZStage {
    fn default() -> Self { Self::new() }
}

impl Device for Uc2ZStage {
    fn name(&self) -> &str { "UC2ZStage" }
    fn description(&self) -> &str { "Z Stage for openUC2" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.writer.is_none() { return Err(MmError::CommHubMissing); }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> { self.props.set(name, val) }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Stage }
    fn busy(&self) -> bool { false }
}

impl Stage for Uc2ZStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        if !self.initialized { return Err(MmError::NotConnected); }
        let steps = (pos / self.step_size_um).round() as i64;
        self.move_abs(steps)?;
        self.pos_steps = steps;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        Ok(self.pos_steps as f64 * self.step_size_um)
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let pos = self.get_position_um()?;
        self.set_position_um(pos + d)
    }

    fn home(&mut self) -> MmResult<()> {
        if !self.initialized { return Err(MmError::NotConnected); }
        self.move_abs(0)?;
        self.pos_steps = 0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> { Ok(()) }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((0.0, 25000.0)) }

    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }

    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn make_stage() -> Uc2ZStage {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let writer: JsonWriter = Arc::new(move |cmd| {
            log.lock().unwrap().push(cmd.to_string());
            Ok("ok".to_string())
        });
        Uc2ZStage::new().with_writer(writer)
    }

    #[test]
    fn move_and_read() {
        let mut stage = make_stage();
        stage.initialize().unwrap();
        stage.set_position_um(500.0).unwrap();
        let pos = stage.get_position_um().unwrap();
        assert!((pos - 500.0).abs() < 0.5);
    }
}
