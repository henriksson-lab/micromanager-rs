//! Arduino32Shutter — controls bit 0 of the 8-bit digital output as a shutter.

use parking_lot::Mutex;
use std::sync::Arc;

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::types::{DeviceType, PropertyValue};

use crate::hub::HubState;

pub type SwitchWriter = Arc<dyn Fn(u8) -> MmResult<()> + Send + Sync>;

pub struct Arduino32Shutter {
    props: PropertyMap,
    initialized: bool,
    shared: Option<Arc<Mutex<HubState>>>,
    writer: Option<SwitchWriter>,
}

impl Arduino32Shutter {
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
        if open {
            state.switch_state |= 1;
        } else {
            state.switch_state &= !1;
        }
        let new_state = state.switch_state;
        drop(state);
        writer(new_state)
    }
}

impl Default for Arduino32Shutter {
    fn default() -> Self { Self::new() }
}

impl Device for Arduino32Shutter {
    fn name(&self) -> &str { "Arduino32-Shutter" }
    fn description(&self) -> &str { "Arduino32 shutter (digital out LSB)" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.shared.is_none() { return Err(MmError::CommHubMissing); }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.write_state(false);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "OnOff" && self.initialized {
            self.write_state(val.as_str() == "On")?;
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

impl Shutter for Arduino32Shutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        if !self.initialized { return Err(MmError::NotConnected); }
        self.write_state(open)?;
        let val = PropertyValue::String(if open { "On" } else { "Off" }.into());
        let _ = self.props.set("OnOff", val);
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        let shared = self.shared.as_ref().ok_or(MmError::NotConnected)?;
        Ok(shared.lock().switch_state & 1 != 0)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        self.set_open(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_shutter() -> Arduino32Shutter {
        let shared = Arc::new(Mutex::new(HubState::default()));
        let shared2 = shared.clone();
        let writer: SwitchWriter = Arc::new(move |state| {
            shared2.lock().switch_state = state;
            Ok(())
        });
        Arduino32Shutter::new().connect(shared, writer)
    }

    #[test]
    fn open_close() {
        let mut s = make_shutter();
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert_eq!(s.get_open().unwrap(), true);
        s.set_open(false).unwrap();
        assert_eq!(s.get_open().unwrap(), false);
    }
}
