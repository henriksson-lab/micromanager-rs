/// Hamilton MVP (Modular Valve Positioner).
///
/// Protocol (TX `\r`, RX echo + 0x06 ACK):
///   Address char (default 'a') prepended to every command.
///   Device echoes the command back, then sends 0x06 (ACK) on success,
///   0x15 (NAK) on failure. Data queries append data after the ACK.
///
///   `<a>LXR\r`         → echo + ACK        initialize/reset
///   `<a>LQT\r`         → echo + ACK + '2'–'7'  valve type digit
///   `<a>LQP\r`         → echo + ACK + N    current position (1-based ASCII digit)
///   `<a>LP0<N>R\r`     → echo + ACK        set position (0=CW, N=1-based)
///   `<a>F\r`           → echo + ACK + 'Y'/'N'  movement finished?
///
/// Valve type → number of positions:
///   '2'=8, '3'=6, '4'=3, '5'=2, '6'=2, '7'=4
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, StateDevice};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

const ACK: char = '\x06';

pub struct HamiltonMvpValve {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    address: char,
    num_positions: u64,
    position: u64,
    labels: Vec<String>,
}

impl HamiltonMvpValve {
    pub fn new(address: char) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Address", PropertyValue::String(address.to_string()), false).unwrap();
        props.define_property("ValveType", PropertyValue::String(String::new()), true).unwrap();
        let num = 6u64;
        let labels: Vec<String> = (1..=num).map(|i| format!("Position-{}", i)).collect();
        Self {
            props,
            transport: None,
            initialized: false,
            address,
            num_positions: num,
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

    /// Send command (prepend address + `\r`), return trimmed response.
    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let full = format!("{}{}\r", self.address, command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.to_string())
        })
    }

    /// Send command, strip echo, verify ACK (0x06), return any data after ACK.
    fn cmd_ack(&mut self, command: &str) -> MmResult<String> {
        let resp = self.cmd(command)?;
        let echo = format!("{}{}", self.address, command);
        let after = resp.strip_prefix(&echo).unwrap_or(&resp);
        if let Some(ack_pos) = after.find(ACK) {
            Ok(after[ack_pos + 1..].trim().to_string())
        } else {
            Err(MmError::LocallyDefined(format!("Hamilton NAK: {:?}", resp)))
        }
    }

    fn valve_type_to_positions(c: char) -> u64 {
        match c {
            '2' => 8,
            '3' => 6,
            '4' => 3,
            '5' | '6' => 2,
            '7' => 4,
            _ => 6,
        }
    }
}

impl Default for HamiltonMvpValve { fn default() -> Self { Self::new('a') } }

impl Device for HamiltonMvpValve {
    fn name(&self) -> &str { "HamiltonMvpValve" }
    fn description(&self) -> &str { "Hamilton MVP Modular Valve Positioner" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Reset/initialize
        self.cmd_ack("LXR")?;
        // Query valve type to determine number of positions
        let type_data = self.cmd_ack("LQT")?;
        let type_char = type_data.chars().next().unwrap_or('3');
        self.num_positions = Self::valve_type_to_positions(type_char);
        self.props.entry_mut("ValveType").map(|e| e.value = PropertyValue::String(type_char.to_string()));
        self.labels = (1..=self.num_positions).map(|i| format!("Position-{}", i)).collect();
        // Query current position
        let pos_data = self.cmd_ack("LQP")?;
        let pos1: u64 = pos_data.trim().parse().unwrap_or(1);
        self.position = pos1.saturating_sub(1); // 1-based → 0-based
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

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
    fn device_type(&self) -> DeviceType { DeviceType::State }
    fn busy(&self) -> bool { false }
}

impl StateDevice for HamiltonMvpValve {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!("Position {} out of range", pos)));
        }
        // Device uses 1-based positions; 0=CW rotation
        let cmd = format!("LP0{}R", pos + 1);
        self.cmd_ack(&cmd)?;
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

    fn make_init_transport() -> MockTransport {
        // LXR → ACK; LQT → ACK + '3' (6-pos valve); LQP → ACK + '1' (pos 1)
        MockTransport::new()
            .expect("aLXR\r", "\x061")         // LXR init echo+ACK (data ignored)
            .expect("aLQT\r", "\x063")         // type '3' = 6 positions
            .expect("aLQP\r", "\x061")         // position 1
    }

    #[test]
    fn initialize() {
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(make_init_transport()));
        v.initialize().unwrap();
        assert_eq!(v.get_number_of_positions(), 6);
        assert_eq!(v.get_position().unwrap(), 0); // pos 1 → 0-indexed 0
    }

    #[test]
    fn set_position() {
        let t = make_init_transport()
            .expect("aLP02R\r", "\x06"); // set position 2 (1-based)
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        v.initialize().unwrap();
        v.set_position(1).unwrap(); // 0-based → device sends 2
        assert_eq!(v.get_position().unwrap(), 1);
    }

    #[test]
    fn nak_fails() {
        let t = make_init_transport()
            .expect("aLP02R\r", "\x15"); // NAK
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        v.initialize().unwrap();
        assert!(v.set_position(1).is_err());
    }

    #[test]
    fn label_roundtrip() {
        let t = make_init_transport()
            .expect("aLP02R\r", "\x06");
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        v.initialize().unwrap();
        v.set_position_label(1, "Buffer").unwrap();
        assert_eq!(v.get_position_label(1).unwrap(), "Buffer");
        v.set_position_by_label("Buffer").unwrap();
        assert_eq!(v.get_position().unwrap(), 1);
    }

    #[test]
    fn valve_type_8_port() {
        let t = MockTransport::new()
            .expect("aLXR\r", "\x06")
            .expect("aLQT\r", "\x062") // type '2' = 8 positions
            .expect("aLQP\r", "\x061");
        let mut v = HamiltonMvpValve::new('a').with_transport(Box::new(t));
        v.initialize().unwrap();
        assert_eq!(v.get_number_of_positions(), 8);
    }

    #[test]
    fn no_transport_error() { assert!(HamiltonMvpValve::new('a').initialize().is_err()); }
}
