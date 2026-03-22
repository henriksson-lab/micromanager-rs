/// Thorlabs Elliptec slider (ELL6 / ELL9) — 2-position state device.
///
/// Protocol (TX/RX `\r`):
///   `<ch>in\r`    → device info (same as stage)
///   `<ch>gp\r`    → `<ch>PO<8-hex>`   get position (0x00000000 or non-zero)
///   `<ch>mofb\r`  → `<ch>PO...`       move forward  (to position 1)
///   `<ch>mobk\r`  → `<ch>PO...`       move backward (to position 0)
///
/// For ELL6/ELL9, there are only 2 positions (forward/backward).
/// Position 0 = home/backward, position 1 = forward.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct ElliptecSlider {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    channel: char,
    position: u64,
    labels: Vec<String>,
}

impl ElliptecSlider {
    pub fn new(channel: char) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Channel", PropertyValue::String(channel.to_string()), false).unwrap();
        let labels = vec!["Position-0".to_string(), "Position-1".to_string()];
        Self { props, transport: None, initialized: false, channel, position: 0, labels }
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

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let full = format!("{}{}\r", self.channel, command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    fn parse_position(resp: &str, channel: char) -> u64 {
        let prefix = format!("{}PO", channel);
        let hex = resp.strip_prefix(&prefix).unwrap_or(resp);
        let pulses = u32::from_str_radix(hex.trim(), 16).unwrap_or(0) as i32;
        if pulses == 0 { 0 } else { 1 }
    }
}

impl Default for ElliptecSlider { fn default() -> Self { Self::new('0') } }

impl Device for ElliptecSlider {
    fn name(&self) -> &str { "ElliptecSlider" }
    fn description(&self) -> &str { "Thorlabs Elliptec Slider (ELL6/ELL9)" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let gp = self.cmd("gp")?;
        self.position = Self::parse_position(&gp, self.channel);
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
    fn device_type(&self) -> DeviceType { DeviceType::State }
    fn busy(&self) -> bool { false }
}

impl StateDevice for ElliptecSlider {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos > 1 {
            return Err(MmError::LocallyDefined(format!("Position {} out of range (0-1)", pos)));
        }
        self.cmd(if pos == 0 { "mobk" } else { "mofb" })?;
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> { Ok(self.position) }
    fn get_number_of_positions(&self) -> u64 { 2 }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels.get(pos as usize).cloned()
            .ok_or_else(|| MmError::LocallyDefined(format!("Position {} out of range", pos)))
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self.labels.iter().position(|l| l == label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64;
        self.set_position(pos)
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        if pos > 1 { return Err(MmError::LocallyDefined(format!("Position {} out of range", pos))); }
        self.labels[pos as usize] = label.to_string();
        Ok(())
    }

    fn set_gate_open(&mut self, _open: bool) -> MmResult<()> { Ok(()) }
    fn get_gate_open(&self) -> MmResult<bool> { Ok(true) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize_at_zero() {
        let t = MockTransport::new().expect("0gp\r", "0PO00000000");
        let mut s = ElliptecSlider::new('0').with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_position().unwrap(), 0);
    }

    #[test]
    fn move_forward_backward() {
        let t = MockTransport::new()
            .expect("0gp\r", "0PO00000000")
            .expect("0mofb\r", "0PO00002710")
            .expect("0mobk\r", "0PO00000000");
        let mut s = ElliptecSlider::new('0').with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position(1).unwrap();
        assert_eq!(s.get_position().unwrap(), 1);
        s.set_position(0).unwrap();
        assert_eq!(s.get_position().unwrap(), 0);
    }

    #[test]
    fn no_transport_error() { assert!(ElliptecSlider::new('0').initialize().is_err()); }
}
