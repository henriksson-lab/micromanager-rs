//! ESP32 XY Stage — dual-axis stage using `mrx <steps>` and `mry <steps>`.

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, XYStage};
use micromanager::types::{DeviceType, PropertyValue};

pub type StageWriter = std::sync::Arc<dyn Fn(&str) -> MmResult<()> + Send + Sync>;

pub struct Esp32XYStage {
    props: PropertyMap,
    initialized: bool,
    pos_x_um: f64,
    pos_y_um: f64,
    step_size_um: f64,
    x_min_um: f64,
    x_max_um: f64,
    y_min_um: f64,
    y_max_um: f64,
    writer: Option<StageWriter>,
}

impl Esp32XYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("XMinUm", PropertyValue::Float(-10000.0), false).unwrap();
        props.define_property("XMaxUm", PropertyValue::Float(10000.0), false).unwrap();
        props.define_property("YMinUm", PropertyValue::Float(-10000.0), false).unwrap();
        props.define_property("YMaxUm", PropertyValue::Float(10000.0), false).unwrap();
        props.define_property("StepSizeUm", PropertyValue::Float(0.1), false).unwrap();
        Self {
            props,
            initialized: false,
            pos_x_um: 0.0,
            pos_y_um: 0.0,
            step_size_um: 0.1,
            x_min_um: -10000.0,
            x_max_um: 10000.0,
            y_min_um: -10000.0,
            y_max_um: 10000.0,
            writer: None,
        }
    }

    pub fn with_writer(mut self, writer: StageWriter) -> Self {
        self.writer = Some(writer);
        self
    }

    fn send(&self, cmd: &str) -> MmResult<()> {
        let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
        writer(cmd)
    }
}

impl Default for Esp32XYStage {
    fn default() -> Self { Self::new() }
}

impl Device for Esp32XYStage {
    fn name(&self) -> &str { "XYStage" }
    fn description(&self) -> &str { "ESP32 XY stage" }

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

impl XYStage for Esp32XYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        if !self.initialized { return Err(MmError::NotConnected); }
        let dx_steps = ((x - self.pos_x_um) / self.step_size_um).round() as i64;
        let dy_steps = ((y - self.pos_y_um) / self.step_size_um).round() as i64;
        if dx_steps != 0 { self.send(&format!("mrx {}", dx_steps))?; }
        if dy_steps != 0 { self.send(&format!("mry {}", dy_steps))?; }
        self.pos_x_um = x;
        self.pos_y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> { Ok((self.pos_x_um, self.pos_y_um)) }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        self.set_xy_position_um(self.pos_x_um + dx, self.pos_y_um + dy)
    }

    fn home(&mut self) -> MmResult<()> { self.set_xy_position_um(0.0, 0.0) }

    fn stop(&mut self) -> MmResult<()> { self.send("stop") }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((self.x_min_um, self.x_max_um, self.y_min_um, self.y_max_um))
    }

    fn get_step_size_um(&self) -> (f64, f64) { (self.step_size_um, self.step_size_um) }

    fn set_origin(&mut self) -> MmResult<()> {
        self.pos_x_um = 0.0;
        self.pos_y_um = 0.0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn make_xystage() -> (Esp32XYStage, Arc<Mutex<Vec<String>>>) {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        let writer: StageWriter = Arc::new(move |cmd| { log2.lock().unwrap().push(cmd.to_string()); Ok(()) });
        (Esp32XYStage::new().with_writer(writer), log)
    }

    #[test]
    fn move_xy() {
        let (mut stage, log) = make_xystage();
        stage.initialize().unwrap();
        stage.set_xy_position_um(100.0, 200.0).unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (100.0, 200.0));
        let cmds = log.lock().unwrap();
        assert!(cmds.iter().any(|c| c.starts_with("mrx ")));
        assert!(cmds.iter().any(|c| c.starts_with("mry ")));
    }

    #[test]
    fn limits() {
        let (stage, _) = make_xystage();
        let (xlo, xhi, ylo, yhi) = stage.get_limits_um().unwrap();
        assert!(xlo < xhi);
        assert!(ylo < yhi);
    }
}
