//! OpenFlexure XY Stage — relative moves via `mrx <steps>` and `mry <steps>`.
//! Position is maintained as a cached step count; poll with "p" for sync.

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, XYStage};
use micromanager::types::{DeviceType, PropertyValue};

pub type Commander = std::sync::Arc<dyn Fn(&str) -> MmResult<String> + Send + Sync>;

pub struct OfXYStage {
    props: PropertyMap,
    initialized: bool,
    steps_x: i64,
    steps_y: i64,
    step_size_um: f64,
    commander: Option<Commander>,
}

impl OfXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("StepSizeUm", PropertyValue::Float(0.07), false).unwrap();
        Self {
            props,
            initialized: false,
            steps_x: 0,
            steps_y: 0,
            step_size_um: 0.07,
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

    fn sync_state(&mut self) -> MmResult<()> {
        let resp = self.send("p")?;
        let mut parts = resp.split_whitespace();
        let x: i64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(self.steps_x);
        let y: i64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(self.steps_y);
        self.steps_x = x;
        self.steps_y = y;
        Ok(())
    }
}

impl Default for OfXYStage {
    fn default() -> Self { Self::new() }
}

impl Device for OfXYStage {
    fn name(&self) -> &str { "OFXYStage" }
    fn description(&self) -> &str { "OpenFlexure XY stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.commander.is_none() { return Err(MmError::CommHubMissing); }
        self.sync_state()?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized { let _ = self.send("release"); self.initialized = false; }
        Ok(())
    }

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

impl XYStage for OfXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        if !self.initialized { return Err(MmError::NotConnected); }
        let tx = (x / self.step_size_um).round() as i64;
        let ty = (y / self.step_size_um).round() as i64;
        let dx = tx - self.steps_x;
        let dy = ty - self.steps_y;
        if dx != 0 { self.send(&format!("mrx {}", dx))?; }
        if dy != 0 { self.send(&format!("mry {}", dy))?; }
        self.steps_x = tx;
        self.steps_y = ty;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        Ok((
            self.steps_x as f64 * self.step_size_um,
            self.steps_y as f64 * self.step_size_um,
        ))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        if !self.initialized { return Err(MmError::NotConnected); }
        let dx_steps = (dx / self.step_size_um).round() as i64;
        let dy_steps = (dy / self.step_size_um).round() as i64;
        if dx_steps != 0 { self.send(&format!("mrx {}", dx_steps))?; }
        if dy_steps != 0 { self.send(&format!("mry {}", dy_steps))?; }
        self.steps_x += dx_steps;
        self.steps_y += dy_steps;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        if !self.initialized { return Err(MmError::NotConnected); }
        self.set_xy_position_um(0.0, 0.0)
    }

    fn stop(&mut self) -> MmResult<()> {
        self.send("stop")?;
        let _ = self.sync_state();
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> { Ok((0.0, 0.0, 0.0, 0.0)) }

    fn get_step_size_um(&self) -> (f64, f64) { (self.step_size_um, self.step_size_um) }

    fn set_origin(&mut self) -> MmResult<()> {
        self.send("zero")?;
        self.steps_x = 0;
        self.steps_y = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn make_stage() -> OfXYStage {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let commander: Commander = Arc::new(move |cmd| {
            log.lock().unwrap().push(cmd.to_string());
            // Return "0 0 0" for "p" queries
            if cmd == "p" { Ok("0 0 0".to_string()) } else { Ok("ok".to_string()) }
        });
        OfXYStage::new().with_commander(commander)
    }

    #[test]
    fn relative_move() {
        let mut stage = make_stage();
        stage.initialize().unwrap();
        stage.set_relative_xy_position_um(70.0, 140.0).unwrap();
        let (x, y) = stage.get_xy_position_um().unwrap();
        assert!((x - 70.0).abs() < 0.1);
        assert!((y - 140.0).abs() < 0.1);
    }
}
