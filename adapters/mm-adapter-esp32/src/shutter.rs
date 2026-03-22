//! ESP32Shutter — Shutter device backed by ESP32 Hub.

use parking_lot::Mutex;
use std::sync::Arc;

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::types::{DeviceType, PropertyValue};

use crate::hub::HubState;

pub type SwitchWriter = Arc<dyn Fn(u8) -> MmResult<()> + Send + Sync>;

pub struct Esp32Shutter {
    props: PropertyMap,
    initialized: bool,
    shared: Option<Arc<Mutex<HubState>>>,
    writer: Option<SwitchWriter>,
}

impl Esp32Shutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("OnOff", PropertyValue::String("Off".into()), false).unwrap();
        props.set_allowed_values("OnOff", &["On", "Off"]).unwrap();
        Self { props, initialized: false, shared: None, writer: None }
    }

    pub fn connect(mut self, shared: Arc<Mutex<HubState>>, writer: SwitchWriter) -> Self {
        self.shared = Some(shared);
        self.writer = Some(writer);
        self
    }

    fn write_state(&self, open: bool) -> MmResult<()> {
        let shared = self.shared.as_ref().ok_or(MmError::NotConnected)?;
        let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
        let mut state = shared.lock();
        if open { state.switch_state |= 0x80; } else { state.switch_state &= !0x80; }
        let s = state.switch_state;
        drop(state);
        writer(s)
    }
}

impl Default for Esp32Shutter {
    fn default() -> Self { Self::new() }
}

impl Device for Esp32Shutter {
    fn name(&self) -> &str { "ESP32-Shutter" }
    fn description(&self) -> &str { "ESP32 shutter" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.shared.is_none() { return Err(MmError::CommHubMissing); }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized { let _ = self.write_state(false); self.initialized = false; }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "OnOff" && self.initialized { self.write_state(val.as_str() == "On")?; }
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

impl Shutter for Esp32Shutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        if !self.initialized { return Err(MmError::NotConnected); }
        self.write_state(open)?;
        let val = PropertyValue::String(if open { "On" } else { "Off" }.into());
        let _ = self.props.set("OnOff", val);
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        let shared = self.shared.as_ref().ok_or(MmError::NotConnected)?;
        Ok(shared.lock().shutter_open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> { self.set_open(true) }
}
