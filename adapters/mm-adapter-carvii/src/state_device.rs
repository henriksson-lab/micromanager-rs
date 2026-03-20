/// CARVII state devices (filter wheels, sliders, motors).
///
/// Protocol (TX `\r`):
///   `<CMD><POS>\r`  → echo               set position (POS = ASCII '1'..'N', 1-based)
///   `r<CMD>\r`      → "r<CMD><POS>\r"    query position (response[2] = ASCII position char)
///
/// Used for: ExFilter (A), EmFilter (B), Dichroic (C), DiskSlider (D),
///           SpinMotor (N), PrismSlider (P), TouchScreen (M).
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, StateDevice};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

pub struct CarviiStateDevice {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    cmd_char: char,
    num_positions: u64,
    position: u64,
    labels: Vec<String>,
}

impl CarviiStateDevice {
    pub fn new(cmd_char: char, num_positions: u64) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        let labels = (0..num_positions).map(|i| format!("Position-{}", i + 1)).collect();
        Self {
            props,
            transport: None,
            initialized: false,
            cmd_char,
            num_positions,
            position: 0,
            labels,
        }
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
        self.call_transport(|t| { let r = t.send_recv(&full)?; Ok(r.trim().to_string()) })
    }
}

impl Device for CarviiStateDevice {
    fn name(&self) -> &str { "CarviiStateDevice" }
    fn description(&self) -> &str { "CARVII State Device" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let query = format!("r{}", self.cmd_char);
        let resp = self.cmd(&query)?;
        // response[2] = ASCII '1'-'N' (1-based) → 0-based
        let pos_byte = resp.as_bytes().get(2).copied().unwrap_or(b'1');
        self.position = (pos_byte.saturating_sub(b'1')) as u64;
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

impl StateDevice for CarviiStateDevice {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!("Position {} out of range", pos)));
        }
        // 1-based ASCII position character
        let pos_char = (b'1' + pos as u8) as char;
        let cmd = format!("{}{}", self.cmd_char, pos_char);
        self.cmd(&cmd)?;
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> { Ok(self.position) }
    fn get_number_of_positions(&self) -> u64 { self.num_positions }

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
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!("Position {} out of range", pos)));
        }
        self.labels[pos as usize] = label.to_string();
        Ok(())
    }

    fn set_gate_open(&mut self, _open: bool) -> MmResult<()> { Ok(()) }
    fn get_gate_open(&self) -> MmResult<bool> { Ok(true) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    #[test]
    fn initialize_ex_filter() {
        // ExFilter, cmd='A', 6 positions; query rA returns position 1 (0-based 0)
        let t = MockTransport::new().expect("rA\r", "rA1");
        let mut d = CarviiStateDevice::new('A', 6).with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 0);
    }

    #[test]
    fn set_position() {
        let t = MockTransport::new()
            .expect("rA\r", "rA1")
            .expect("A3\r", "A3"); // set position 2 (0-based) → '3' (1-based ASCII)
        let mut d = CarviiStateDevice::new('A', 6).with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position(2).unwrap(); // 0-based 2 → 1-based 3 → char '3'
        assert_eq!(d.get_position().unwrap(), 2);
    }

    #[test]
    fn disk_slider() {
        let t = MockTransport::new()
            .expect("rD\r", "rD2")
            .expect("D1\r", "D1");
        let mut d = CarviiStateDevice::new('D', 2).with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 1); // '2' - '1' = 1
        d.set_position(0).unwrap();
        assert_eq!(d.get_position().unwrap(), 0);
    }

    #[test]
    fn label_roundtrip() {
        let t = MockTransport::new()
            .expect("rA\r", "rA1")
            .expect("A2\r", "A2");
        let mut d = CarviiStateDevice::new('A', 6).with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position_label(1, "FITC").unwrap();
        assert_eq!(d.get_position_label(1).unwrap(), "FITC");
        d.set_position_by_label("FITC").unwrap();
        assert_eq!(d.get_position().unwrap(), 1);
    }

    #[test]
    fn no_transport_error() { assert!(CarviiStateDevice::new('A', 6).initialize().is_err()); }
}
