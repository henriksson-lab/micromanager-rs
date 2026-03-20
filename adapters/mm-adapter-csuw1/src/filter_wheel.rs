/// Yokogawa CSU-W1 filter wheel and dichroic selector.
///
/// Protocol (TX/RX `\r`):
///   `FW_POS,<wheel>,<pos>\r`   → `A`           set filter wheel position (1-based)
///   `FW_POS, <wheel>, ?\r`     → `<pos>\rA`    query position (1-based in response)
///   `DMM_POS,1,<pos>\r`        → `A`           set dichroic position (1-based)
///   `DMM_POS,1, ?\r`           → `<pos>\rA`    query dichroic position
///
/// Positions: 1-based in serial commands, 0-based internally.
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, StateDevice};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

pub struct CsuFilterWheel {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    wheel: u8,
    num_positions: u64,
    position: u64,
    labels: Vec<String>,
}

impl CsuFilterWheel {
    pub fn new(wheel: u8, num_positions: u64) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Wheel", PropertyValue::Integer(wheel as i64), false).unwrap();
        let labels = (0..num_positions).map(|i| format!("Filter-{}", i + 1)).collect();
        Self { props, transport: None, initialized: false, wheel, num_positions, position: 0, labels }
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
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    fn parse_pos_response(resp: &str) -> u64 {
        // Response: "<pos>\rA" or "<pos>A" — take first token
        resp.split(|c: char| c.is_whitespace() || c == '\r' || c == '\n' || c == 'A')
            .find(|s| !s.is_empty())
            .and_then(|s| s.parse::<u64>().ok())
            .map(|p| p.saturating_sub(1)) // 1-based → 0-based
            .unwrap_or(0)
    }
}

impl Default for CsuFilterWheel { fn default() -> Self { Self::new(1, 6) } }

impl Device for CsuFilterWheel {
    fn name(&self) -> &str { "CsuFilterWheel" }
    fn description(&self) -> &str { "Yokogawa CSU-W1 Filter Wheel" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let q = format!("FW_POS, {}, ?", self.wheel);
        let resp = self.cmd(&q)?;
        self.position = Self::parse_pos_response(&resp);
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

impl StateDevice for CsuFilterWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!("Position {} out of range", pos)));
        }
        let cmd = format!("FW_POS,{},{}", self.wheel, pos + 1); // 1-based
        let resp = self.cmd(&cmd)?;
        if resp.contains('N') {
            return Err(MmError::LocallyDefined(format!("CSU FW NAK: {}", resp)));
        }
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

/// CSU-W1 dichroic mirror selector.
pub struct CsuDichroic {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    num_positions: u64,
    position: u64,
    labels: Vec<String>,
}

impl CsuDichroic {
    pub fn new(num_positions: u64) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        let labels = (0..num_positions).map(|i| format!("Dichroic-{}", i + 1)).collect();
        Self { props, transport: None, initialized: false, num_positions, position: 0, labels }
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
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }
}

impl Default for CsuDichroic { fn default() -> Self { Self::new(3) } }

impl Device for CsuDichroic {
    fn name(&self) -> &str { "CsuDichroic" }
    fn description(&self) -> &str { "Yokogawa CSU-W1 Dichroic Selector" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let resp = self.cmd("DMM_POS,1, ?")?;
        self.position = resp.split(|c: char| c.is_whitespace() || c == '\r' || c == '\n' || c == 'A')
            .find(|s| !s.is_empty())
            .and_then(|s| s.parse::<u64>().ok())
            .map(|p| p.saturating_sub(1))
            .unwrap_or(0);
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

impl StateDevice for CsuDichroic {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!("Position {} out of range", pos)));
        }
        let cmd = format!("DMM_POS,1,{}", pos + 1);
        let resp = self.cmd(&cmd)?;
        if resp.contains('N') {
            return Err(MmError::LocallyDefined(format!("CSU dichroic NAK: {}", resp)));
        }
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
    fn filter_wheel_init() {
        let t = MockTransport::new().expect("FW_POS, 1, ?\r", "2\rA");
        let mut w = CsuFilterWheel::new(1, 6).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert_eq!(w.get_position().unwrap(), 1); // '2' → 0-based 1
    }

    #[test]
    fn filter_wheel_set() {
        let t = MockTransport::new()
            .expect("FW_POS, 1, ?\r", "1\rA")
            .expect("FW_POS,1,3\r", "A");
        let mut w = CsuFilterWheel::new(1, 6).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position(2).unwrap(); // 0-based 2 → sends 3
        assert_eq!(w.get_position().unwrap(), 2);
    }

    #[test]
    fn dichroic_init_and_set() {
        let t = MockTransport::new()
            .expect("DMM_POS,1, ?\r", "1\rA")
            .expect("DMM_POS,1,2\r", "A");
        let mut d = CsuDichroic::new(3).with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 0);
        d.set_position(1).unwrap();
        assert_eq!(d.get_position().unwrap(), 1);
    }

    #[test]
    fn no_transport_error() {
        assert!(CsuFilterWheel::new(1, 6).initialize().is_err());
        assert!(CsuDichroic::new(3).initialize().is_err());
    }
}
