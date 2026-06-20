//! OpenFlexure LED Shutter — controls LED illumination via Sangaboard.

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::types::{DeviceType, PropertyValue};

use super::xystage::Commander;

pub struct OfShutter {
    props: PropertyMap,
    initialized: bool,
    open: bool,
    brightness: f64,
    commander: Option<Commander>,
}

impl OfShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("State", &["0", "1"]).unwrap();
        props
            .define_property("LED Brightness", PropertyValue::Float(1.0), false)
            .unwrap();
        props
            .set_property_limits("LED Brightness", 0.0, 1.0)
            .unwrap();
        Self {
            props,
            initialized: false,
            open: false,
            brightness: 1.0,
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

    fn set_brightness(&mut self) -> MmResult<()> {
        self.send(&format!("led_cc {}", self.brightness))?;
        Ok(())
    }
}

impl Default for OfShutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for OfShutter {
    fn name(&self) -> &str {
        "OFShutter"
    }
    fn description(&self) -> &str {
        "OpenFlexure LED shutter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.commander.is_none() {
            return Err(MmError::CommHubMissing);
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.set_open(false);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "State" && self.initialized {
            let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)? != 0;
            self.set_open(state)?;
        }
        if name == "LED Brightness" {
            self.brightness = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            if self.open && self.initialized {
                self.set_brightness()?;
            }
        }
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
        DeviceType::Shutter
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Shutter for OfShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        if let Some(c) = &self.commander {
            let cmd = if open {
                format!("led_cc {}", self.brightness)
            } else {
                "led_cc 0".to_string()
            };
            c(&cmd)?;
        }
        self.open = open;
        let _ = self
            .props
            .set("State", PropertyValue::Integer(if open { 1 } else { 0 }));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn open_close_uses_led_cc() {
        let commander: Commander = Arc::new(|cmd| match cmd {
            "led_cc 1" | "led_cc 0" => Ok("ok".to_string()),
            other => Err(MmError::LocallyDefined(format!(
                "unexpected command {other}"
            ))),
        });
        let mut s = OfShutter::new().with_commander(commander);
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn fire_is_unsupported() {
        let mut s = OfShutter::new();
        assert_eq!(s.fire(1.0).unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn initialize_requires_hub_commander() {
        let mut s = OfShutter::new();
        assert_eq!(s.initialize().unwrap_err(), MmError::CommHubMissing);
    }
}
