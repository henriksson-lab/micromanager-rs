/// ArduinoShutter — gates the digital output pattern as a shutter.
///
/// In the original adapter, opening restores the switch state and closing
/// writes zero while preserving the selected switch state.
/// This implementation owns a reference to the hub's shared state so it can
/// compose the full switch_state before sending to the hub.
use parking_lot::Mutex;
use std::sync::Arc;

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::types::{DeviceType, PropertyValue};

use super::hub::HubState;

/// A write callback: the shutter calls this to push a new switch state.
pub type SwitchWriter = Arc<dyn Fn(u16) -> MmResult<()> + Send + Sync>;

pub struct ArduinoShutter {
    props: PropertyMap,
    initialized: bool,
    shared: Option<Arc<Mutex<HubState>>>,
    writer: Option<SwitchWriter>,
}

impl ArduinoShutter {
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

    /// Connect to hub's shared state and write callback.
    pub fn connect(mut self, shared: Arc<Mutex<HubState>>, writer: SwitchWriter) -> Self {
        self.shared = Some(shared);
        self.writer = Some(writer);
        self
    }

    fn write_state(&self, open: bool) -> MmResult<()> {
        let shared = self.shared.as_ref().ok_or(MmError::NotConnected)?;
        let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;

        let mut state = shared.lock();
        let output_state = if open { state.switch_state } else { 0 };
        state.shutter_state = open;
        drop(state);
        writer(output_state)
    }
}

fn on_off_value(open: bool) -> PropertyValue {
    PropertyValue::Integer(if open { 1 } else { 0 })
}

impl Default for ArduinoShutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ArduinoShutter {
    fn name(&self) -> &str {
        "Arduino-Shutter"
    }
    fn description(&self) -> &str {
        "Arduino shutter (digital out LSB)"
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
                    .map(|s| s.lock().shutter_state)
                    .unwrap_or(false);
                Ok(on_off_value(open))
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "OnOff" {
            let open = val.as_i64().ok_or(MmError::InvalidPropertyValue)? > 0;
            if self.initialized {
                self.write_state(open)?;
            } else if let Some(shared) = &self.shared {
                shared.lock().shutter_state = open;
            }
            self.props.set(name, on_off_value(open))?;
            return Ok(());
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

impl Shutter for ArduinoShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        if !self.initialized {
            return Err(MmError::NotConnected);
        }
        self.write_state(open)?;
        self.props.set("OnOff", on_off_value(open))?;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        let shared = self.shared.as_ref().ok_or(MmError::NotConnected)?;
        Ok(shared.lock().shutter_state)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_shutter() -> (
        ArduinoShutter,
        Arc<Mutex<HubState>>,
        Arc<std::sync::Mutex<Vec<u16>>>,
    ) {
        let shared = Arc::new(Mutex::new(HubState::default()));
        let writes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let writes2 = writes.clone();
        let writer: SwitchWriter = Arc::new(move |state| {
            writes2.lock().unwrap().push(state);
            Ok(())
        });
        (
            ArduinoShutter::new().connect(shared.clone(), writer),
            shared,
            writes,
        )
    }

    #[test]
    fn open_restores_switch_state_and_close_writes_zero() {
        let (mut shutter, shared, writes) = make_shutter();
        shared.lock().switch_state = 42;
        shutter.initialize().unwrap();

        shutter.set_open(true).unwrap();
        shutter.set_open(false).unwrap();

        assert_eq!(&*writes.lock().unwrap(), &[42, 0]);
        assert_eq!(shared.lock().switch_state, 42);
        assert!(!shared.lock().shutter_state);
    }

    #[test]
    fn fire_is_unsupported() {
        let (mut shutter, _, _) = make_shutter();
        shutter.initialize().unwrap();
        assert_eq!(shutter.fire(1.0).unwrap_err(), MmError::UnsupportedCommand);
    }
}
