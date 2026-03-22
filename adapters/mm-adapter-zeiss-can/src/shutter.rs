/// Zeiss CAN-bus shutter (reflected light / fluorescence shutter).
///
/// Protocol (TX `\r`, RX `\r`):
///   `HPRs0\r`  → `PH\r`  (close shutter)
///   `HPRs1\r`  → `PH\r`  (open shutter)
///   `HPRp\r`   → `PH{0|1}\r`  (query shutter state)
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::types::{DeviceType, PropertyValue};

use crate::hub::ZeissHub;

pub struct ZeissShutter {
    props: PropertyMap,
    hub: ZeissHub,
    initialized: bool,
    open: bool,
}

impl ZeissShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, hub: ZeissHub::new(), initialized: false, open: false }
    }

    pub fn new_with_hub(hub: ZeissHub) -> Self {
        let mut s = Self::new();
        s.hub = hub;
        s
    }

    fn send(&mut self, cmd: &str) -> MmResult<String> {
        self.hub.send(cmd)
    }
}

impl Default for ZeissShutter { fn default() -> Self { Self::new() } }

impl Device for ZeissShutter {
    fn name(&self) -> &str { "ZeissShutter" }
    fn description(&self) -> &str { "Zeiss CAN-bus reflected light shutter" }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.hub.is_connected() { return Err(MmError::NotConnected); }
        let resp = self.send("HPRp")?;
        let state = resp.strip_prefix("PH").unwrap_or("0").trim().to_string();
        self.open = state == "1";
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> { self.props.set(name, val) }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Shutter }
    fn busy(&self) -> bool { false }
}

impl Shutter for ZeissShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let resp = self.send(if open { "HPRs1" } else { "HPRs0" })?;
        if resp.starts_with("PH") {
            self.open = open;
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("Zeiss shutter error: '{}'", resp)))
        }
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.open) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::NotSupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn shutter_with(t: MockTransport) -> ZeissShutter {
        let hub = ZeissHub::new().with_transport(Box::new(t));
        ZeissShutter::new_with_hub(hub)
    }

    #[test]
    fn initialize_reads_state() {
        let t = MockTransport::new().any("PH0");
        let mut s = shutter_with(t);
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn open_close() {
        let t = MockTransport::new().any("PH0").any("PH").any("PH");
        let mut s = shutter_with(t);
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }
}
