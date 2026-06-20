use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::types::{DeviceType, FocusDirection, PropertyValue};

const LOWER_LIMIT_UM: f64 = -300.0;
const UPPER_LIMIT_UM: f64 = 300.0;

/// Demo Z stage.
pub struct DemoStage {
    props: PropertyMap,
    initialized: bool,
    position_um: f64,
    sequenceable: bool,
}

impl DemoStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Position", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("Position", LOWER_LIMIT_UM, UPPER_LIMIT_UM)
            .unwrap();
        props
            .define_property("UseSequences", PropertyValue::String("No".into()), false)
            .unwrap();
        props
            .set_allowed_values("UseSequences", &["No", "Yes"])
            .unwrap();
        Self {
            props,
            initialized: false,
            position_um: 0.0,
            sequenceable: false,
        }
    }

    fn set_position_checked(&mut self, pos: f64) -> MmResult<()> {
        if !(LOWER_LIMIT_UM..=UPPER_LIMIT_UM).contains(&pos) {
            return Err(MmError::UnknownPosition);
        }
        self.position_um = pos;
        Ok(())
    }
}

impl Default for DemoStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for DemoStage {
    fn name(&self) -> &str {
        "DStage"
    }
    fn description(&self) -> &str {
        "Demo Z stage"
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
            "Position" => Ok(PropertyValue::Float(self.position_um)),
            "UseSequences" => Ok(PropertyValue::String(
                if self.sequenceable { "Yes" } else { "No" }.into(),
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Position" => {
                let pos = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_position_checked(pos)
            }
            "UseSequences" => {
                match val.as_str() {
                    "Yes" => self.sequenceable = true,
                    "No" => self.sequenceable = false,
                    _ => return Err(MmError::InvalidPropertyValue),
                }
                self.props.set(name, val)
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
        DeviceType::Stage
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Stage for DemoStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        self.set_position_checked(pos)
    }

    fn get_position_um(&self) -> MmResult<f64> {
        Ok(self.position_um)
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        self.set_position_checked(self.position_um + d)
    }

    fn home(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn stop(&mut self) -> MmResult<()> {
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((LOWER_LIMIT_UM, UPPER_LIMIT_UM))
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

    #[test]
    fn move_stage() {
        let mut stage = DemoStage::new();
        stage.initialize().unwrap();
        stage.set_position_um(100.0).unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 100.0);
        stage.set_relative_position_um(50.0).unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 150.0);
        assert_eq!(stage.home().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(stage.get_position_um().unwrap(), 150.0);
    }

    #[test]
    fn upstream_position_property_and_limits() {
        let mut stage = DemoStage::new();
        assert!(stage.has_property("Position"));
        assert!(!stage.has_property("Position_um"));
        assert_eq!(stage.get_limits().unwrap(), (-300.0, 300.0));

        stage
            .set_property("Position", PropertyValue::Float(300.0))
            .unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 300.0);
        assert_eq!(
            stage
                .set_property("Position", PropertyValue::Float(300.1))
                .unwrap_err(),
            MmError::UnknownPosition
        );
        assert_eq!(stage.get_position_um().unwrap(), 300.0);
    }

    #[test]
    fn upstream_sequence_toggle_property() {
        let mut stage = DemoStage::new();
        assert_eq!(
            stage.get_property("UseSequences").unwrap(),
            PropertyValue::String("No".into())
        );
        stage
            .set_property("UseSequences", PropertyValue::String("Yes".into()))
            .unwrap();
        assert_eq!(
            stage.get_property("UseSequences").unwrap(),
            PropertyValue::String("Yes".into())
        );
        assert!(stage
            .set_property("UseSequences", PropertyValue::String("Maybe".into()))
            .is_err());
    }

    #[test]
    fn upstream_home_unsupported_and_focus_direction_unknown() {
        let mut stage = DemoStage::new();
        stage.set_position_um(25.0).unwrap();
        assert_eq!(stage.home().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(stage.get_position_um().unwrap(), 25.0);
        assert_eq!(stage.get_focus_direction(), FocusDirection::Unknown);
    }
}
