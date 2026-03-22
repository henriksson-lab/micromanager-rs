/// ASI FW-1000 shutter control.
///
/// Commands:
///   `SO <n>\r`  → open shutter n (response echoes `1`)
///   `SC <n>\r`  → close shutter n (response echoes `0`)
///   `SQ <n>\r`  → query shutter state (response: last 2 chars, bit 0 = state)
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct AsiShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    shutter_id: u8,
    is_open: bool,
}

impl AsiShutter {
    pub fn new(shutter_id: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("ShutterId", PropertyValue::Integer(shutter_id as i64), true).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            shutter_id,
            is_open: false,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let cmd = command.to_string();
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }
}

impl Device for AsiShutter {
    fn name(&self) -> &str { "ASI-Shutter" }
    fn description(&self) -> &str { "ASI FW-1000 shutter" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Query initial state
        let resp = self.cmd(&format!("SQ {}", self.shutter_id))?;
        // Parse: last char is state bit
        let state_char = resp.chars().last().unwrap_or('0');
        self.is_open = state_char == '1';
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.cmd(&format!("SC {}", self.shutter_id));
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
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

impl Shutter for AsiShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open {
            format!("SO {}", self.shutter_id)
        } else {
            format!("SC {}", self.shutter_id)
        };
        self.cmd(&cmd)?;
        self.is_open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.is_open) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> { self.set_open(true) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize_reads_state() {
        let t = MockTransport::new().expect("SQ 1", "00"); // closed
        let mut s = AsiShutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn open_close() {
        let t = MockTransport::new()
            .expect("SQ 1", "00")
            .expect("SO 1", "1")
            .expect("SC 1", "0");
        let mut s = AsiShutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() {
        assert!(AsiShutter::new(1).initialize().is_err());
    }
}
