//! ESP32Shutter — Shutter device backed by ESP32 Hub.

use parking_lot::Mutex;
use std::sync::Arc;

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::types::{DeviceType, PropertyValue};

use super::hub::HubState;

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
        props
            .define_property("OnOff", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("OnOff", &["0", "1"]).unwrap();
        Self {
            props,
            initialized: false,
            shared: None,
            writer: None,
        }
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
        state.shutter_open = open;
        let s = if open { state.switch_state | 0x80 } else { 0 };
        drop(state);
        writer(s)
    }
}

impl Default for Esp32Shutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Esp32Shutter {
    fn name(&self) -> &str {
        "ESP32-Shutter"
    }
    fn description(&self) -> &str {
        "ESP32 shutter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.shared.is_none() {
            return Err(MmError::CommHubMissing);
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "OnOff" && self.initialized {
            let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            match state {
                0 | 1 => self.write_state(state > 0)?,
                _ => return Err(MmError::InvalidPropertyValue),
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

impl Shutter for Esp32Shutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        if !self.initialized {
            return Err(MmError::NotConnected);
        }
        self.write_state(open)?;
        let val = PropertyValue::Integer(if open { 1 } else { 0 });
        let _ = self.props.set("OnOff", val);
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        let shared = self.shared.as_ref().ok_or(MmError::NotConnected)?;
        Ok(shared.lock().shutter_open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_shutter() -> (Esp32Shutter, Arc<std::sync::Mutex<Vec<u8>>>) {
        let shared = Arc::new(Mutex::new(HubState::default()));
        shared.lock().switch_state = 7;
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log2 = log.clone();
        let writer: SwitchWriter = Arc::new(move |s| {
            log2.lock().unwrap().push(s);
            Ok(())
        });
        (Esp32Shutter::new().connect(shared, writer), log)
    }

    #[test]
    fn onoff_uses_integer_encoding_and_restores_switch_state() {
        let (mut s, log) = make_shutter();
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        s.set_open(false).unwrap();
        assert_eq!(&*log.lock().unwrap(), &[135, 0]);
        assert_eq!(s.get_property("OnOff").unwrap(), PropertyValue::Integer(0));
    }

    #[test]
    fn fire_and_invalid_onoff_are_unsupported() {
        let (mut s, _) = make_shutter();
        s.initialize().unwrap();
        assert_eq!(s.fire(1.0).unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(
            s.set_property("OnOff", PropertyValue::Integer(2))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn shutdown_does_not_close_output() {
        let (mut s, log) = make_shutter();
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        s.shutdown().unwrap();
        assert_eq!(&*log.lock().unwrap(), &[135]);
    }
}
