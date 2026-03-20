use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Shutter};
use mm_device::types::{DeviceType, PropertyValue};

/// Demo shutter.
pub struct DemoShutter {
    props: PropertyMap,
    initialized: bool,
    open: bool,
}

impl DemoShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("State", PropertyValue::String("Closed".into()), false).unwrap();
        props.set_allowed_values("State", &["Open", "Closed"]).unwrap();
        Self {
            props,
            initialized: false,
            open: false,
        }
    }
}

impl Default for DemoShutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for DemoShutter {
    fn name(&self) -> &str { "DShutter" }
    fn description(&self) -> &str { "Demo shutter" }

    fn initialize(&mut self) -> MmResult<()> {
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.open = false;
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::String(if self.open { "Open" } else { "Closed" }.into())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let s = val.as_str().to_string();
                match s.as_str() {
                    "Open" => self.open = true,
                    "Closed" => self.open = false,
                    _ => return Err(MmError::InvalidPropertyValue),
                }
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

    fn device_type(&self) -> DeviceType { DeviceType::Shutter }
    fn busy(&self) -> bool { false }
}

impl Shutter for DemoShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        // Open then close (instant simulation)
        self.open = true;
        self.open = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_close() {
        let mut s = DemoShutter::new();
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }
}
