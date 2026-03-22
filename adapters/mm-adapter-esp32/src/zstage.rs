//! ESP32 Z Stage — single-axis stage using `mrz <steps>` command.

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

pub type StageWriter = std::sync::Arc<dyn Fn(&str) -> MmResult<()> + Send + Sync>;

pub struct Esp32ZStage {
    props: PropertyMap,
    initialized: bool,
    pos_um: f64,
    step_size_um: f64,
    min_um: f64,
    max_um: f64,
    writer: Option<StageWriter>,
}

impl Esp32ZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("ZLowUm", PropertyValue::Float(-5000.0), false).unwrap();
        props.define_property("ZHighUm", PropertyValue::Float(5000.0), false).unwrap();
        props.define_property("StepSizeUm", PropertyValue::Float(0.1), false).unwrap();
        Self {
            props,
            initialized: false,
            pos_um: 0.0,
            step_size_um: 0.1,
            min_um: -5000.0,
            max_um: 5000.0,
            writer: None,
        }
    }

    pub fn with_writer(mut self, writer: StageWriter) -> Self {
        self.writer = Some(writer);
        self
    }

    fn send_move(&self, steps: i64) -> MmResult<()> {
        let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
        writer(&format!("mrz {}", steps))
    }
}

impl Default for Esp32ZStage {
    fn default() -> Self { Self::new() }
}

impl Device for Esp32ZStage {
    fn name(&self) -> &str { "ZStage" }
    fn description(&self) -> &str { "ESP32 Z stage" }

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

impl Stage for Esp32ZStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        if !self.initialized { return Err(MmError::NotConnected); }
        let delta = pos - self.pos_um;
        let steps = (delta / self.step_size_um).round() as i64;
        if steps != 0 { self.send_move(steps)?; }
        self.pos_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.pos_um) }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let new_pos = self.pos_um + d;
        self.set_position_um(new_pos)
    }

    fn home(&mut self) -> MmResult<()> { self.set_position_um(0.0) }

    fn stop(&mut self) -> MmResult<()> {
        let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
        writer("stop")
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((self.min_um, self.max_um)) }

    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }

    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn make_stage() -> (Esp32ZStage, Arc<Mutex<Vec<String>>>) {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        let writer: StageWriter = Arc::new(move |cmd| { log2.lock().unwrap().push(cmd.to_string()); Ok(()) });
        (Esp32ZStage::new().with_writer(writer), log)
    }

    #[test]
    fn move_absolute() {
        let (mut stage, log) = make_stage();
        stage.initialize().unwrap();
        stage.set_position_um(10.0).unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 10.0);
        let cmds = log.lock().unwrap();
        assert!(cmds[0].starts_with("mrz "));
    }

    #[test]
    fn limits() {
        let (stage, _) = make_stage();
        let (lo, hi) = stage.get_limits().unwrap();
        assert!(lo < hi);
    }
}
