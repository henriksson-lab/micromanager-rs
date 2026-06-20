use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::types::{DeviceType, PropertyValue};
use std::time::Instant;

/// Demo shutter.
pub struct DemoShutter {
    props: PropertyMap,
    initialized: bool,
    open: bool,
    delay_ms: f64,
    changed_time: Instant,
}

impl DemoShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("State", &["0", "1"]).unwrap();
        props
            .define_property("Delay_ms", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("Delay_ms", 0.0, f64::MAX)
            .unwrap();
        Self {
            props,
            initialized: false,
            open: false,
            delay_ms: 0.0,
            changed_time: Instant::now(),
        }
    }

    fn set_open_state(&mut self, open: bool) {
        self.open = open;
        self.changed_time = Instant::now();
    }
}

impl Default for DemoShutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for DemoShutter {
    fn name(&self) -> &str {
        "DShutter"
    }
    fn description(&self) -> &str {
        "Demo shutter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        self.changed_time = Instant::now();
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.set_open_state(false);
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::Integer(if self.open { 1 } else { 0 })),
            "Delay_ms" => Ok(PropertyValue::Float(self.delay_ms)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                match state {
                    0 => self.set_open_state(false),
                    1 => self.set_open_state(true),
                    _ => return Err(MmError::InvalidPropertyValue),
                }
                Ok(())
            }
            "Delay_ms" => {
                let delay = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(delay))?;
                self.delay_ms = delay;
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

    fn device_type(&self) -> DeviceType {
        DeviceType::Shutter
    }
    fn busy(&self) -> bool {
        self.changed_time.elapsed().as_secs_f64() * 1000.0 < self.delay_ms
    }
}

impl Shutter for DemoShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.set_open_state(open);
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

    #[test]
    fn open_close() {
        let mut s = DemoShutter::new();
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
        assert_eq!(s.get_property("State").unwrap(), PropertyValue::Integer(0));
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        assert_eq!(s.get_property("State").unwrap(), PropertyValue::Integer(1));
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn state_property_matches_demo_adapter_integer_encoding() {
        let mut s = DemoShutter::new();
        s.set_property("State", PropertyValue::Integer(1)).unwrap();
        assert!(s.get_open().unwrap());
        s.set_property("State", PropertyValue::String("0".into()))
            .unwrap();
        assert!(!s.get_open().unwrap());
        assert!(
            s.set_property("State", PropertyValue::String("Open".into()))
                .is_err()
        );
    }

    #[test]
    fn fire_is_unsupported_like_upstream() {
        let mut s = DemoShutter::new();
        assert_eq!(s.fire(1.0).unwrap_err(), MmError::UnsupportedCommand);
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn busy_tracks_delay_after_state_changes() {
        let mut s = DemoShutter::new();
        s.set_property("Delay_ms", PropertyValue::Float(5.0))
            .unwrap();

        s.set_open(true).unwrap();
        assert!(s.busy());
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(!s.busy());

        s.set_property("State", PropertyValue::Integer(0)).unwrap();
        assert!(s.busy());
    }
}
