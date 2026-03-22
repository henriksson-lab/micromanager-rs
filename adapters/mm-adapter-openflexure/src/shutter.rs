//! OpenFlexure LED Shutter — controls LED illumination via Sangaboard.
//! The Sangaboard does not have a built-in LED command in all firmware versions;
//! we model this as a simple boolean shutter with a no-op transport for now,
//! suitable for adapters that combine external LED control through the hub.

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::types::{DeviceType, PropertyValue};

use crate::xystage::Commander;

pub struct OfShutter {
    props: PropertyMap,
    initialized: bool,
    open: bool,
    commander: Option<Commander>,
}

impl OfShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("OnOff", PropertyValue::String("Off".into()), false).unwrap();
        props.set_allowed_values("OnOff", &["On", "Off"]).unwrap();
        Self { props, initialized: false, open: false, commander: None }
    }

    pub fn with_commander(mut self, c: Commander) -> Self {
        self.commander = Some(c);
        self
    }

    #[allow(dead_code)]
    fn send(&self, cmd: &str) -> MmResult<String> {
        let c = self.commander.as_ref().ok_or(MmError::NotConnected)?;
        c(cmd)
    }
}

impl Default for OfShutter {
    fn default() -> Self { Self::new() }
}

impl Device for OfShutter {
    fn name(&self) -> &str { "OFShutter" }
    fn description(&self) -> &str { "OpenFlexure LED shutter" }

    fn initialize(&mut self) -> MmResult<()> {
        // Commander is optional: if not connected to hub, still initialize (no hardware LED)
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized { let _ = self.set_open(false); self.initialized = false; }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "OnOff" && self.initialized {
            let _ = self.set_open(val.as_str() == "On");
        }
        self.props.set(name, val)
    }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Shutter }
    fn busy(&self) -> bool { false }
}

impl Shutter for OfShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        // Send LED command if commander is present
        if let Some(c) = &self.commander {
            let cmd = if open { "led on" } else { "led off" };
            let _ = c(cmd); // tolerate error (not all Sangaboard firmware has LED command)
        }
        self.open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.open) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> { self.set_open(true) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_close_no_transport() {
        let mut s = OfShutter::new();
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }
}
