//! ESP32 Z Stage — single-axis stage using `Z,<um>`.

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::types::{DeviceType, FocusDirection, PropertyValue};

pub type StageWriter = std::sync::Arc<dyn Fn(&str) -> MmResult<()> + Send + Sync>;
pub type RangeReader = std::sync::Arc<dyn Fn() -> MmResult<f64> + Send + Sync>;

pub struct Esp32ZStage {
    props: PropertyMap,
    initialized: bool,
    pos_um: f64,
    min_um: f64,
    max_um: f64,
    writer: Option<StageWriter>,
    range_reader: Option<RangeReader>,
}

impl Esp32ZStage {
    pub fn new() -> Self {
        Self {
            props: PropertyMap::new(),
            initialized: false,
            pos_um: 0.0,
            min_um: 0.0,
            max_um: 100.0,
            writer: None,
            range_reader: None,
        }
    }

    pub fn with_writer(mut self, writer: StageWriter) -> Self {
        self.writer = Some(writer);
        self
    }

    pub fn with_range_reader(mut self, range_reader: RangeReader) -> Self {
        self.range_reader = Some(range_reader);
        self
    }

    fn send_move(&self, pos: f64) -> MmResult<()> {
        let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
        writer(&format!("Z,{:.3}", pos))
    }
}

impl Default for Esp32ZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Esp32ZStage {
    fn name(&self) -> &str {
        "ZStage"
    }
    fn description(&self) -> &str {
        "ESP32 Z stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.writer.is_none() {
            return Err(MmError::CommHubMissing);
        }
        if let Some(read_range) = &self.range_reader {
            self.max_um = read_range()?;
        }
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
        false
    }
}

impl Stage for Esp32ZStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        if !self.initialized {
            return Err(MmError::NotConnected);
        }
        let pos = pos.clamp(self.min_um, self.max_um);
        self.send_move(pos)?;
        self.pos_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        Ok(self.pos_um)
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let new_pos = self.pos_um + d;
        self.set_position_um(new_pos)
    }

    fn home(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn stop(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((self.min_um, self.max_um))
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

    fn make_stage() -> (Esp32ZStage, Arc<Mutex<Vec<String>>>) {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        let writer: StageWriter = Arc::new(move |cmd| {
            log2.lock().unwrap().push(cmd.to_string());
            Ok(())
        });
        (Esp32ZStage::new().with_writer(writer), log)
    }

    #[test]
    fn move_absolute() {
        let (mut stage, log) = make_stage();
        stage.initialize().unwrap();
        stage.set_position_um(10.0).unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 10.0);
        let cmds = log.lock().unwrap();
        assert_eq!(cmds[0], "Z,10.000");
    }

    #[test]
    fn limits() {
        let (stage, _) = make_stage();
        let (lo, hi) = stage.get_limits().unwrap();
        assert_eq!((lo, hi), (0.0, 100.0));
        assert!(!stage.has_property("ZLowUm"));
        assert!(!stage.has_property("ZHighUm"));
        assert!(!stage.has_property("StepSizeUm"));
    }

    #[test]
    fn clamps_to_upstream_limits() {
        let (mut stage, log) = make_stage();
        stage.initialize().unwrap();
        stage.set_position_um(101.0).unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 100.0);
        assert_eq!(log.lock().unwrap()[0], "Z,100.000");
    }

    #[test]
    fn initialize_updates_upper_limit_from_hub_axis_info_like_upstream() {
        let (mut stage, log) = make_stage();
        let range_reader: RangeReader = Arc::new(|| Ok(250.0));
        stage = stage.with_range_reader(range_reader);

        stage.initialize().unwrap();
        assert_eq!(stage.get_limits().unwrap(), (0.0, 250.0));

        stage.set_position_um(300.0).unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 250.0);
        assert_eq!(log.lock().unwrap()[0], "Z,250.000");
    }

    #[test]
    fn reports_unsupported_actions_like_upstream() {
        let (mut stage, log) = make_stage();
        stage.initialize().unwrap();
        assert_eq!(stage.home().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(stage.stop().unwrap_err(), MmError::UnsupportedCommand);
        assert!(log.lock().unwrap().is_empty());
    }
}
