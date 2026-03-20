use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, XYStage};
use mm_device::types::{DeviceType, PropertyValue};

/// Demo XY stage.
pub struct DemoXYStage {
    props: PropertyMap,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl DemoXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("X_um", PropertyValue::Float(0.0), false).unwrap();
        props.define_property("Y_um", PropertyValue::Float(0.0), false).unwrap();
        Self {
            props,
            initialized: false,
            x_um: 0.0,
            y_um: 0.0,
        }
    }
}

impl Default for DemoXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for DemoXYStage {
    fn name(&self) -> &str { "DXYStage" }
    fn description(&self) -> &str { "Demo XY stage" }

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
            "X_um" => Ok(PropertyValue::Float(self.x_um)),
            "Y_um" => Ok(PropertyValue::Float(self.y_um)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "X_um" => {
                self.x_um = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                Ok(())
            }
            "Y_um" => {
                self.y_um = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                Ok(())
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

    fn device_type(&self) -> DeviceType { DeviceType::XYStage }
    fn busy(&self) -> bool { false }
}

impl XYStage for DemoXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        Ok((self.x_um, self.y_um))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((-50_000.0, 50_000.0, -50_000.0, 50_000.0))
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        (0.1, 0.1)
    }

    fn set_origin(&mut self) -> MmResult<()> {
        self.x_um = 0.0;
        self.y_um = 0.0;
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
}
