use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::types::{DeviceType, PropertyValue};

const LOWER_LIMIT_UM: f64 = 0.0;
const UPPER_LIMIT_UM: f64 = 20_000.0;
const STEP_SIZE_UM: f64 = 0.015;
const MIN_VELOCITY: f64 = 0.1;

/// Demo XY stage.
pub struct DemoXYStage {
    props: PropertyMap,
    initialized: bool,
    x_um: f64,
    y_um: f64,
    velocity: f64,
}

impl DemoXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Velocity", PropertyValue::Float(10.0), false)
            .unwrap();
        Self {
            props,
            initialized: false,
            x_um: 0.0,
            y_um: 0.0,
            velocity: 10.0,
        }
    }

    fn set_xy_position_checked(&mut self, x: f64, y: f64) -> MmResult<()> {
        if !(LOWER_LIMIT_UM..=UPPER_LIMIT_UM).contains(&x)
            || !(LOWER_LIMIT_UM..=UPPER_LIMIT_UM).contains(&y)
        {
            return Err(MmError::UnknownPosition);
        }
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }
}

impl Default for DemoXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for DemoXYStage {
    fn name(&self) -> &str {
        "DXYStage"
    }
    fn description(&self) -> &str {
        "Demo XY stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Velocity" => Ok(PropertyValue::Float(self.velocity)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Velocity" => {
                let velocity = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.velocity = velocity.max(MIN_VELOCITY);
                self.props.set(name, PropertyValue::Float(self.velocity))
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

impl XYStage for DemoXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        self.set_xy_position_checked(x, y)
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        Ok((self.x_um, self.y_um))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        self.set_xy_position_checked(self.x_um + dx, self.y_um + dy)
    }

    fn home(&mut self) -> MmResult<()> {
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((
            LOWER_LIMIT_UM,
            UPPER_LIMIT_UM,
            LOWER_LIMIT_UM,
            UPPER_LIMIT_UM,
        ))
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        (STEP_SIZE_UM, STEP_SIZE_UM)
    }

    fn set_origin(&mut self) -> MmResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_xy() {
        let mut stage = DemoXYStage::new();
        stage.initialize().unwrap();
        stage.set_xy_position_um(100.0, 200.0).unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (100.0, 200.0));
        stage.set_relative_xy_position_um(-10.0, 20.0).unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (90.0, 220.0));
    }

    #[test]
    fn upstream_velocity_property_limits_and_step_size() {
        let mut stage = DemoXYStage::new();
        assert!(stage.has_property("Velocity"));
        assert!(!stage.has_property("X_um"));
        assert!(!stage.has_property("Y_um"));
        assert_eq!(
            stage.get_limits_um().unwrap(),
            (0.0, 20_000.0, 0.0, 20_000.0)
        );
        assert_eq!(stage.get_step_size_um(), (0.015, 0.015));

        stage
            .set_property("Velocity", PropertyValue::Float(-1.0))
            .unwrap();
        assert_eq!(
            stage.get_property("Velocity").unwrap(),
            PropertyValue::Float(0.1)
        );
    }

    #[test]
    fn upstream_home_and_origin_are_noops() {
        let mut stage = DemoXYStage::new();
        stage.set_xy_position_um(100.0, 200.0).unwrap();
        stage.home().unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (100.0, 200.0));
        stage.set_origin().unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (100.0, 200.0));
        assert_eq!(
            stage.set_relative_xy_position_um(-101.0, 0.0).unwrap_err(),
            MmError::UnknownPosition
        );
    }
}
