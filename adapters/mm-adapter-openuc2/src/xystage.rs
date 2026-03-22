//! UC2 XY Stage — absolute positioning via JSON motor commands.
//! stepperid 1 = X, stepperid 2 = Y.

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, XYStage};
use micromanager::types::{DeviceType, PropertyValue};

pub type JsonWriter = std::sync::Arc<dyn Fn(&str) -> MmResult<String> + Send + Sync>;

pub struct Uc2XYStage {
    props: PropertyMap,
    initialized: bool,
    pos_x_steps: i64,
    pos_y_steps: i64,
    step_size_um: f64,
    writer: Option<JsonWriter>,
}

impl Uc2XYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("StepSizeUm", PropertyValue::Float(0.05), false).unwrap();
        Self {
            props,
            initialized: false,
            pos_x_steps: 0,
            pos_y_steps: 0,
            step_size_um: 0.05,
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

    fn move_abs(&self, x: i64, y: i64) -> MmResult<()> {
        let cmd = format!(
            r#"{{"task":"/motor_act","motor":{{"steppers":[{{"stepperid":1,"position":{},"speed":5000,"isabs":1}},{{"stepperid":2,"position":{},"speed":5000,"isabs":1}}]}}}}"#,
            x, y
        );
        self.send(&cmd)?;
        Ok(())
    }
}

impl Default for Uc2XYStage {
    fn default() -> Self { Self::new() }
}

impl Device for Uc2XYStage {
    fn name(&self) -> &str { "UC2XYStage" }
    fn description(&self) -> &str { "XY Stage for openUC2" }

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
    fn device_type(&self) -> DeviceType { DeviceType::XYStage }
    fn busy(&self) -> bool { false }
}

impl XYStage for Uc2XYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        if !self.initialized { return Err(MmError::NotConnected); }
        let xs = (x / self.step_size_um).round() as i64;
        let ys = (y / self.step_size_um).round() as i64;
        self.move_abs(xs, ys)?;
        self.pos_x_steps = xs;
        self.pos_y_steps = ys;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        Ok((
            self.pos_x_steps as f64 * self.step_size_um,
            self.pos_y_steps as f64 * self.step_size_um,
        ))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let (x, y) = self.get_xy_position_um()?;
        self.set_xy_position_um(x + dx, y + dy)
    }

    fn home(&mut self) -> MmResult<()> {
        if !self.initialized { return Err(MmError::NotConnected); }
        self.move_abs(0, 0)?;
        self.pos_x_steps = 0;
        self.pos_y_steps = 0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> { Ok(()) }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((0.0, 100000.0 * self.step_size_um, 0.0, 100000.0 * self.step_size_um))
    }

    fn get_step_size_um(&self) -> (f64, f64) { (self.step_size_um, self.step_size_um) }

    fn set_origin(&mut self) -> MmResult<()> {
        self.pos_x_steps = 0;
        self.pos_y_steps = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn make_stage() -> (Uc2XYStage, Arc<Mutex<Vec<String>>>) {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        let writer: JsonWriter = Arc::new(move |cmd| {
            log2.lock().unwrap().push(cmd.to_string());
            Ok("ok".to_string())
        });
        (Uc2XYStage::new().with_writer(writer), log)
    }

    #[test]
    fn set_get_position() {
        let (mut stage, log) = make_stage();
        stage.initialize().unwrap();
        stage.set_xy_position_um(100.0, 200.0).unwrap();
        let (x, y) = stage.get_xy_position_um().unwrap();
        // round-trip through steps — may have tiny float error
        assert!((x - 100.0).abs() < 0.1);
        assert!((y - 200.0).abs() < 0.1);
        assert!(!log.lock().unwrap().is_empty());
    }
}
