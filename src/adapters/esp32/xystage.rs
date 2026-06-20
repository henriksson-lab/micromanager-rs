//! ESP32 XY Stage — dual-axis stage using `X,<um>` and `Y,<um>`.

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::types::{DeviceType, PropertyValue};

pub type StageWriter = std::sync::Arc<dyn Fn(&str) -> MmResult<()> + Send + Sync>;
pub type XYRangeReader = std::sync::Arc<dyn Fn() -> MmResult<(f64, f64)> + Send + Sync>;

const PROP_X_MIN_UM: &str = "X Stage Min Posn(um)";
const PROP_X_MAX_UM: &str = "X Stage Max Posn(um)";
const PROP_Y_MIN_UM: &str = "Y Stage Min Posn(um)";
const PROP_Y_MAX_UM: &str = "Y Stage Max Posn(um)";

pub struct Esp32XYStage {
    props: PropertyMap,
    initialized: bool,
    pos_x_um: f64,
    pos_y_um: f64,
    step_size_x_um: f64,
    step_size_y_um: f64,
    x_min_um: f64,
    x_max_um: f64,
    y_min_um: f64,
    y_max_um: f64,
    writer: Option<StageWriter>,
    range_reader: Option<XYRangeReader>,
}

impl Esp32XYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property(PROP_X_MIN_UM, PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property(PROP_X_MAX_UM, PropertyValue::Float(200.0), false)
            .unwrap();
        props
            .define_property(PROP_Y_MIN_UM, PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property(PROP_Y_MAX_UM, PropertyValue::Float(200.0), false)
            .unwrap();
        Self {
            props,
            initialized: false,
            pos_x_um: 0.0,
            pos_y_um: 0.0,
            step_size_x_um: 0.1,
            step_size_y_um: 0.1,
            x_min_um: 0.0,
            x_max_um: 200.0,
            y_min_um: 0.0,
            y_max_um: 200.0,
            writer: None,
            range_reader: None,
        }
    }

    pub fn with_writer(mut self, writer: StageWriter) -> Self {
        self.writer = Some(writer);
        self
    }

    pub fn with_range_reader(mut self, range_reader: XYRangeReader) -> Self {
        self.range_reader = Some(range_reader);
        self
    }

    fn send(&self, cmd: &str) -> MmResult<()> {
        let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
        writer(cmd)
    }
}

impl Default for Esp32XYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Esp32XYStage {
    fn name(&self) -> &str {
        "XYStage"
    }
    fn description(&self) -> &str {
        "ESP32 XY stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.writer.is_none() {
            return Err(MmError::CommHubMissing);
        }
        if let Some(read_range) = &self.range_reader {
            let (travel_x, travel_y) = read_range()?;
            self.x_max_um = travel_x;
            self.y_max_um = travel_y;
            self.step_size_x_um = travel_x / 65535.0;
            self.step_size_y_um = travel_y / 65535.0;
            self.props
                .set(PROP_X_MAX_UM, PropertyValue::Float(travel_x))?;
            self.props
                .set(PROP_Y_MAX_UM, PropertyValue::Float(travel_y))?;
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            PROP_X_MIN_UM => Ok(PropertyValue::Float(self.x_min_um)),
            PROP_X_MAX_UM => Ok(PropertyValue::Float(self.x_max_um)),
            PROP_Y_MIN_UM => Ok(PropertyValue::Float(self.y_min_um)),
            PROP_Y_MAX_UM => Ok(PropertyValue::Float(self.y_max_um)),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            PROP_X_MIN_UM => {
                let limit = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.x_min_um = limit;
                self.props.set(name, PropertyValue::Float(limit))
            }
            PROP_X_MAX_UM => {
                let limit = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.x_max_um = limit;
                self.props.set(name, PropertyValue::Float(limit))
            }
            PROP_Y_MIN_UM => {
                let limit = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.y_min_um = limit;
                self.props.set(name, PropertyValue::Float(limit))
            }
            PROP_Y_MAX_UM => {
                let limit = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.y_max_um = limit;
                self.props.set(name, PropertyValue::Float(limit))
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
        DeviceType::XYStage
    }
    fn busy(&self) -> bool {
        false
    }
}

impl XYStage for Esp32XYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        if !self.initialized {
            return Err(MmError::NotConnected);
        }
        let x = x.clamp(self.x_min_um, self.x_max_um);
        let y = y.clamp(self.y_min_um, self.y_max_um);
        if self.pos_x_um != x {
            self.send(&format!("X,{:.3}", x))?;
        }
        if self.pos_y_um != y {
            self.send(&format!("Y,{:.3}", y))?;
        }
        self.pos_x_um = x;
        self.pos_y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        Ok((self.pos_x_um, self.pos_y_um))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        self.set_xy_position_um(self.pos_x_um + dx, self.pos_y_um + dy)
    }

    fn home(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn stop(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((self.x_min_um, self.x_max_um, self.y_min_um, self.y_max_um))
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        (self.step_size_x_um, self.step_size_y_um)
    }

    fn set_origin(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn make_xystage() -> (Esp32XYStage, Arc<Mutex<Vec<String>>>) {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        let writer: StageWriter = Arc::new(move |cmd| {
            log2.lock().unwrap().push(cmd.to_string());
            Ok(())
        });
        (Esp32XYStage::new().with_writer(writer), log)
    }

    #[test]
    fn move_xy() {
        let (mut stage, log) = make_xystage();
        stage.initialize().unwrap();
        stage.set_xy_position_um(100.0, 200.0).unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (100.0, 200.0));
        let cmds = log.lock().unwrap();
        assert_eq!(&*cmds, &["X,100.000", "Y,200.000"]);
    }

    #[test]
    fn limits() {
        let (stage, _) = make_xystage();
        let (xlo, xhi, ylo, yhi) = stage.get_limits_um().unwrap();
        assert_eq!((xlo, xhi, ylo, yhi), (0.0, 200.0, 0.0, 200.0));
        assert!(stage.has_property(PROP_X_MIN_UM));
        assert!(stage.has_property(PROP_X_MAX_UM));
        assert!(stage.has_property(PROP_Y_MIN_UM));
        assert!(stage.has_property(PROP_Y_MAX_UM));
        assert!(!stage.has_property("XMinUm"));
        assert!(!stage.has_property("StepSizeUm"));
    }

    #[test]
    fn limit_properties_update_clamping_limits() {
        let (mut stage, log) = make_xystage();
        stage.initialize().unwrap();
        stage
            .set_property(PROP_X_MAX_UM, PropertyValue::Float(50.0))
            .unwrap();
        stage
            .set_property(PROP_Y_MAX_UM, PropertyValue::Float(60.0))
            .unwrap();

        stage.set_xy_position_um(70.0, 80.0).unwrap();

        assert_eq!(stage.get_xy_position_um().unwrap(), (50.0, 60.0));
        assert_eq!(&*log.lock().unwrap(), &["X,50.000", "Y,60.000"]);
    }

    #[test]
    fn initialize_updates_limits_and_step_size_from_hub_axis_info_like_upstream() {
        let (mut stage, log) = make_xystage();
        let range_reader: XYRangeReader = Arc::new(|| Ok((300.0, 400.0)));
        stage = stage.with_range_reader(range_reader);

        stage.initialize().unwrap();

        assert_eq!(stage.get_limits_um().unwrap(), (0.0, 300.0, 0.0, 400.0));
        let (x_step, y_step) = stage.get_step_size_um();
        assert!((x_step - (300.0 / 65535.0)).abs() < f64::EPSILON);
        assert!((y_step - (400.0 / 65535.0)).abs() < f64::EPSILON);

        stage.set_xy_position_um(350.0, 450.0).unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (300.0, 400.0));
        assert_eq!(&*log.lock().unwrap(), &["X,300.000", "Y,400.000"]);
    }

    #[test]
    fn clamps_and_reports_unsupported_actions_like_upstream() {
        let (mut stage, log) = make_xystage();
        stage.initialize().unwrap();
        stage.set_xy_position_um(-1.0, 201.0).unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (0.0, 200.0));
        assert_eq!(&*log.lock().unwrap(), &["Y,200.000"]);
        assert_eq!(stage.home().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(stage.stop().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(stage.set_origin().unwrap_err(), MmError::UnsupportedCommand);
    }
}
