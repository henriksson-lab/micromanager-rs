//! Arduino32Shutter — controls bit 0 of the 8-bit digital output as a shutter.

use parking_lot::Mutex;
use std::sync::Arc;

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::types::{DeviceType, PropertyValue};

use super::hub::HubState;

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

        let new_state = {
            let state = shared.lock();
            if open {
                state.switch_state & 63
            } else {
                0
            }
        };
        writer(new_state)?;
        shared.lock().shutter_open = open;
        Ok(())
    }
}

impl Default for Arduino32Shutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Arduino32Shutter {
    fn name(&self) -> &str {
        "Arduino32-Shutter"
    }
    fn description(&self) -> &str {
        "Arduino32 shutter (digital out LSB)"
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
        match name {
            "OnOff" => {
                let open = self
                    .shared
                    .as_ref()
                    .map(|s| s.lock().shutter_open)
                    .unwrap_or(false);
                Ok(PropertyValue::Integer(if open { 1 } else { 0 }))
            }
            _ => self.props.get(name).cloned(),
        }
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

impl Shutter for Arduino32Shutter {
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

    fn make_shutter() -> Arduino32Shutter {
        let shared = Arc::new(Mutex::new(HubState::default()));
        let shared2 = shared.clone();
        let writer: SwitchWriter = Arc::new(move |state| {
            shared2.lock().switch_state = state;
            Ok(())
        });
        Arduino32Shutter::new().connect(shared, writer)
    }

    fn make_shutter_with_writer(writer: SwitchWriter) -> (Arduino32Shutter, Arc<Mutex<HubState>>) {
        let shared = Arc::new(Mutex::new(HubState::default()));
        (
            Arduino32Shutter::new().connect(shared.clone(), writer),
            shared,
        )
    }

    #[test]
    fn open_close() {
        let mut s = make_shutter();
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert_eq!(s.get_open().unwrap(), true);
        assert_eq!(s.get_property("OnOff").unwrap(), PropertyValue::Integer(1));
        s.set_open(false).unwrap();
        assert_eq!(s.get_open().unwrap(), false);
        assert_eq!(s.get_property("OnOff").unwrap(), PropertyValue::Integer(0));
    }

    #[test]
    fn onoff_property_uses_upstream_integer_values() {
        let mut s = make_shutter();
        s.initialize().unwrap();
        s.set_property("OnOff", PropertyValue::Integer(1)).unwrap();
        assert_eq!(s.get_open().unwrap(), true);
        assert!(s.set_property("OnOff", PropertyValue::Integer(2)).is_err());
        assert_eq!(s.get_open().unwrap(), true);
        assert!(s
            .set_property("OnOff", PropertyValue::String("On".into()))
            .is_err());
    }

    #[test]
    fn fire_is_unsupported_like_upstream() {
        let mut s = make_shutter();
        s.initialize().unwrap();
        assert_eq!(s.fire(1.0).unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn shutdown_does_not_change_output_like_upstream() {
        let mut s = make_shutter();
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        s.shutdown().unwrap();
        assert_eq!(s.get_open().unwrap(), true);
    }

    #[test]
    fn onoff_property_reads_shared_state_like_upstream() {
        let writer: SwitchWriter = Arc::new(|_| Ok(()));
        let (s, shared) = make_shutter_with_writer(writer);
        shared.lock().shutter_open = true;

        assert_eq!(s.get_property("OnOff").unwrap(), PropertyValue::Integer(1));
    }

    #[test]
    fn failed_write_does_not_update_shutter_state() {
        let writer: SwitchWriter = Arc::new(|_| Err(MmError::SerialInvalidResponse));
        let (mut s, shared) = make_shutter_with_writer(writer);
        s.initialize().unwrap();

        assert_eq!(
            s.set_property("OnOff", PropertyValue::Integer(1))
                .unwrap_err(),
            MmError::SerialInvalidResponse
        );
        assert_eq!(shared.lock().shutter_open, false);
    }
}
