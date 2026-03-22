/// Vincent Uniblitz shutter controller (D1/D3).
///
/// Binary protocol — single byte commands, NO response from device.
///
/// Command byte = base + offset:
///   Address 'x' (broadcast) → base = 64 (0x40)
///   Address 0–7             → base = 128 + address * 16
///
/// D1 offsets (single shutter A or dual A+B):
///   +0 = Open A,  +1 = Close A
///   +4 = Open B,  +5 = Close B
///
/// D3 offsets (3-channel):
///   +0 = Open ch1,  +1 = Close ch1
///   +2 = Open ch2,  +3 = Close ch2
///   +4 = Open ch3,  +5 = Close ch3
///   +6 = Open all,  +7 = Close all
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

/// Which Vincent controller variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VincentModel { D1, D3 }

pub struct VincentShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    model: VincentModel,
    address: u8,   // 0–7 or 0xFF for broadcast ('x')
    channel: u8,   // 0 = A (D1) or channel index (D3)
    is_open: bool,
}

impl VincentShutter {
    pub fn new(model: VincentModel) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();

        Self { props, transport: None, initialized: false, model, address: 0xFF, channel: 0, is_open: false }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where F: FnOnce(&mut dyn Transport) -> MmResult<R> {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn base_byte(&self) -> u8 {
        if self.address == 0xFF { 64 } else { 128 + self.address * 16 }
    }

    fn open_offset(&self) -> u8 {
        match self.model {
            VincentModel::D1 => if self.channel == 0 { 0 } else { 4 },
            VincentModel::D3 => self.channel * 2,
        }
    }

    fn close_offset(&self) -> u8 { self.open_offset() + 1 }
}

impl Default for VincentShutter { fn default() -> Self { Self::new(VincentModel::D1) } }

impl Device for VincentShutter {
    fn name(&self) -> &str { "VincentShutter" }
    fn description(&self) -> &str { "Vincent Uniblitz shutter controller" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Close shutter on init; device gives no response
        let cmd = self.base_byte() + self.close_offset();
        self.call_transport(|t| t.send_bytes(&[cmd]))?;
        self.is_open = false;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let cmd = self.base_byte() + self.close_offset();
            let _ = self.call_transport(|t| t.send_bytes(&[cmd]));
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

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

impl Shutter for VincentShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let offset = if open { self.open_offset() } else { self.close_offset() };
        let cmd = self.base_byte() + offset;
        self.call_transport(|t| t.send_bytes(&[cmd]))?;
        self.is_open = open;
        Ok(())
    }
    fn get_open(&self) -> MmResult<bool> { Ok(self.is_open) }
    fn fire(&mut self, _dt: f64) -> MmResult<()> { self.set_open(true) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn d1_broadcast_commands() {
        // broadcast base=64; close=64+1=65, open=64+0=64
        let t = MockTransport::new(); // no responses expected
        let mut s = VincentShutter::new(VincentModel::D1).with_transport(Box::new(t));
        s.initialize().unwrap(); // sends close byte 65
        assert!(!s.get_open().unwrap());
        assert_eq!(s.base_byte(), 64);
        assert_eq!(s.open_offset(), 0);
        assert_eq!(s.close_offset(), 1);
    }

    #[test]
    fn d1_open_close() {
        let t = MockTransport::new();
        let mut s = VincentShutter::new(VincentModel::D1).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
        // Verify bytes sent: [65 (init close), 64 (open), 65 (close)]
        // (can't inspect bytes from outside test, but they're in received_bytes)
    }

    #[test]
    fn d3_channel_offsets() {
        let mut s = VincentShutter::new(VincentModel::D3);
        s.channel = 2; // channel 3
        assert_eq!(s.open_offset(), 4);
        assert_eq!(s.close_offset(), 5);
    }

    #[test]
    fn addressed_base_byte() {
        let mut s = VincentShutter::new(VincentModel::D1);
        s.address = 2;
        assert_eq!(s.base_byte(), 128 + 2 * 16); // = 160
    }

    #[test]
    fn no_transport_error() { assert!(VincentShutter::new(VincentModel::D1).initialize().is_err()); }
}
