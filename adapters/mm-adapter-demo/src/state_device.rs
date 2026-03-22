use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::types::{DeviceType, PropertyValue};

const NUM_POSITIONS: u64 = 6;

/// Demo filter wheel / state device.
pub struct DemoStateDevice {
    props: PropertyMap,
    initialized: bool,
    position: u64,
    labels: Vec<String>,
    gate_open: bool,
}

impl DemoStateDevice {
    pub fn new() -> Self {
        let labels: Vec<String> = (0..NUM_POSITIONS).map(|i| format!("State-{}", i)).collect();
        let mut props = PropertyMap::new();
        props.define_property("State", PropertyValue::Integer(0), false).unwrap();
        props.define_property("Label", PropertyValue::String(labels[0].clone()), false).unwrap();
        Self {
            props,
            initialized: false,
            position: 0,
            labels,
            gate_open: true,
        }
    }
}

impl Default for DemoStateDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for DemoStateDevice {
    fn name(&self) -> &str { "DWheel" }
    fn description(&self) -> &str { "Demo filter wheel" }

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
            "State" => Ok(PropertyValue::Integer(self.position as i64)),
            "Label" => Ok(PropertyValue::String(self.labels[self.position as usize].clone())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u64;
                if pos >= NUM_POSITIONS {
                    return Err(MmError::UnknownPosition);
                }
                self.position = pos;
                Ok(())
            }
            "Label" => {
                let label = val.as_str().to_string();
                let pos = self.labels.iter().position(|l| l == &label)
                    .ok_or(MmError::UnknownLabel(label))? as u64;
                self.position = pos;
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

    fn device_type(&self) -> DeviceType { DeviceType::State }
    fn busy(&self) -> bool { false }
}

impl StateDevice for DemoStateDevice {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= NUM_POSITIONS {
            return Err(MmError::UnknownPosition);
        }
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        Ok(self.position)
    }

    fn get_number_of_positions(&self) -> u64 {
        NUM_POSITIONS
    }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels.get(pos as usize)
            .cloned()
            .ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self.labels.iter().position(|l| l == label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64;
        self.position = pos;
        Ok(())
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        if pos >= NUM_POSITIONS {
            return Err(MmError::UnknownPosition);
        }
        self.labels[pos as usize] = label.to_string();
        Ok(())
    }

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(self.gate_open)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions() {
        let mut w = DemoStateDevice::new();
        w.initialize().unwrap();
        assert_eq!(w.get_position().unwrap(), 0);
        w.set_position(3).unwrap();
        assert_eq!(w.get_position().unwrap(), 3);
        assert!(w.set_position(10).is_err());
    }

    #[test]
    fn label_navigation() {
        let mut w = DemoStateDevice::new();
        w.initialize().unwrap();
        w.set_position_label(2, "DAPI").unwrap();
        w.set_position_by_label("DAPI").unwrap();
        assert_eq!(w.get_position().unwrap(), 2);
    }
}
