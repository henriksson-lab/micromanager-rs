//! UC2Shutter — Shutter device using laser JSON command.
//! Open = LASERval 255, Closed = LASERval 0.

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::types::{DeviceType, PropertyValue};

use crate::xystage::JsonWriter;

pub struct Uc2Shutter {
    props: PropertyMap,
    initialized: bool,
    open: bool,
    writer: Option<JsonWriter>,
}

impl Uc2Shutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("OnOff", PropertyValue::String("Off".into()), false).unwrap();
        props.set_allowed_values("OnOff", &["On", "Off"]).unwrap();
        Self { props, initialized: false, open: false, writer: None }
    }

    pub fn with_writer(mut self, writer: JsonWriter) -> Self {
        self.writer = Some(writer);
        self
    }

    fn send_state(&self, open: bool) -> MmResult<()> {
        let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
        let val = if open { 255 } else { 0 };
        let cmd = format!(r#"{{"task":"/laser_act","LASERid":1,"LASERval":{}}}"#, val);
        writer(&cmd)?;
        Ok(())
    }
}

impl Default for Uc2Shutter {
    fn default() -> Self { Self::new() }
}

impl Device for Uc2Shutter {
    fn name(&self) -> &str { "UC2Shutter" }
    fn description(&self) -> &str { "LED/Laser Shutter for openUC2" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.writer.is_none() { return Err(MmError::CommHubMissing); }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized { let _ = self.send_state(false); self.initialized = false; }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "OnOff" && self.initialized { self.send_state(val.as_str() == "On")?; }
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

impl Shutter for Uc2Shutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        if !self.initialized { return Err(MmError::NotConnected); }
        self.send_state(open)?;
        self.open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.open) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> { self.set_open(true) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn make_shutter() -> Uc2Shutter {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let writer: JsonWriter = Arc::new(move |cmd| {
            log.lock().unwrap().push(cmd.to_string());
            Ok("ok".to_string())
        });
        Uc2Shutter::new().with_writer(writer)
    }

    #[test]
    fn open_close() {
        let mut s = make_shutter();
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }
}
