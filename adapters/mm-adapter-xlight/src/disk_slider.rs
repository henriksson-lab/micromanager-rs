/// CrestOptics X-Light spinning disk slider.
///
/// 3 positions, 0-based on wire:
///   0 = disk out of light path
///   1 = disk position 70 µm
///   2 = disk position 40 µm
///
/// Query:  `rD\r` → echoes `rDN` (N=0..2)
/// Set:    `DN\r` → echoes `DN`
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const LABELS: [&str; 3] = ["Disk Out", "Disk Pos 70um", "Disk Pos 40um"];

pub struct XLightDiskSlider {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position: u64,
    labels: Vec<String>,
    gate_open: bool,
}

impl XLightDiskSlider {
    pub fn new() -> Self {
        let labels = LABELS.iter().map(|s| s.to_string()).collect();
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, transport: None, initialized: false, position: 0, labels, gate_open: true }
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
        let full = format!("{}\r", command);
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }
}

impl Default for XLightDiskSlider { fn default() -> Self { Self::new() } }

impl Device for XLightDiskSlider {
    fn name(&self) -> &str { "XLight-DiskSlider" }
    fn description(&self) -> &str { "CrestOptics X-Light spinning disk slider" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let resp = self.cmd("rD")?;
        // 0-based, no offset
        let digit = resp.chars().last()
            .and_then(|c| c.to_digit(10))
            .unwrap_or(0) as u64;
        self.position = digit;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::Integer(self.position as i64)),
            "Label" => Ok(PropertyValue::String(
                self.labels.get(self.position as usize).cloned().unwrap_or_default()
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u64;
                self.set_position(pos)
            }
            "Label" => {
                let label = val.as_str().to_string();
                self.set_position_by_label(&label)
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::State }
    fn busy(&self) -> bool { false }
}

impl StateDevice for XLightDiskSlider {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= 3 { return Err(MmError::UnknownPosition); }
        if self.initialized {
            self.cmd(&format!("D{}", pos))?;
        }
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> { Ok(self.position) }
    fn get_number_of_positions(&self) -> u64 { 3 }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels.get(pos as usize).cloned().ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self.labels.iter().position(|l| l == label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64;
        self.set_position(pos)
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        if pos >= 3 { return Err(MmError::UnknownPosition); }
        self.labels[pos as usize] = label.to_string();
        Ok(())
    }

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> { self.gate_open = open; Ok(()) }
    fn get_gate_open(&self) -> MmResult<bool> { Ok(self.gate_open) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize_reads_position() {
        let t = MockTransport::new().expect("rD\r", "rD0");
        let mut d = XLightDiskSlider::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 0);
    }

    #[test]
    fn set_slider_position() {
        let t = MockTransport::new()
            .expect("rD\r", "rD0")
            .expect("D2\r", "D2");
        let mut d = XLightDiskSlider::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position(2).unwrap();
        assert_eq!(d.get_position().unwrap(), 2);
    }

    #[test]
    fn label_navigation() {
        let t = MockTransport::new()
            .expect("rD\r", "rD0")
            .expect("D1\r", "D1");
        let mut d = XLightDiskSlider::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position_by_label("Disk Pos 70um").unwrap();
        assert_eq!(d.get_position().unwrap(), 1);
    }
}
