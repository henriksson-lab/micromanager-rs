use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Stage};
use mm_device::types::{DeviceType, FocusDirection, PropertyValue};

/// Demo Z stage.
pub struct DemoStage {
    props: PropertyMap,
    initialized: bool,
    position_um: f64,
}

impl DemoStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Position_um", PropertyValue::Float(0.0), false).unwrap();
        Self {
            props,
            initialized: false,
            position_um: 0.0,
        }
    }
}

impl Default for DemoStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for DemoStage {
    fn name(&self) -> &str { "DStage" }
    fn description(&self) -> &str { "Demo Z stage" }

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
            "Position_um" => Ok(PropertyValue::Float(self.position_um)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Position_um" => {
                self.position_um = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
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

    fn device_type(&self) -> DeviceType { DeviceType::Stage }
    fn busy(&self) -> bool { false }
}

impl Stage for DemoStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        self.position_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        Ok(self.position_um)
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        self.position_um += d;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        self.position_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((-1000.0, 1000.0))
    }

    fn get_focus_direction(&self) -> FocusDirection {
        FocusDirection::TowardSample
    }

    fn is_continuous_focus_drive(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_stage() {
        let mut stage = DemoStage::new();
        stage.initialize().unwrap();
        stage.set_position_um(100.0).unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 100.0);
        stage.set_relative_position_um(50.0).unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 150.0);
        stage.home().unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 0.0);
    }
}
