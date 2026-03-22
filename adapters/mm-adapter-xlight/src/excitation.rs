/// CrestOptics X-Light excitation filter wheel.
///
/// 8 positions, 1-based on wire.
/// Older firmware uses command letter 'E'; newer firmware uses 'A'.
/// During initialize we first try `rE`, if the echo doesn't start with "rE"
/// we fall back to `rA` and set `use_new_cmd = true`.
/// Query:  `rE\r` or `rA\r` → echoes `rEN` or `rAN`
/// Set:    `EN\r` or `AN\r` → echoes same
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct XLightExcitation {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position: u64,
    num_positions: u64,
    labels: Vec<String>,
    gate_open: bool,
    use_new_cmd: bool, // true → use 'A', false → use 'E'
}

impl XLightExcitation {
    pub fn new() -> Self {
        let num_positions: u64 = 8;
        let labels = (0..num_positions).map(|i| format!("Excitation-{}", i)).collect();
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            props, transport: None, initialized: false,
            position: 0, num_positions, labels, gate_open: true,
            use_new_cmd: false,
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
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }

    fn cmd_letter(&self) -> char { if self.use_new_cmd { 'A' } else { 'E' } }
}

impl Default for XLightExcitation { fn default() -> Self { Self::new() } }

impl Device for XLightExcitation {
    fn name(&self) -> &str { "XLight-Excitation" }
    fn description(&self) -> &str { "CrestOptics X-Light excitation filter wheel" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }

        // Try old-style 'E' command first
        let resp = self.cmd("rE")?;
        let (pos, new_cmd) = if resp.starts_with("rE") {
            let digit = resp.chars().last()
                .and_then(|c| c.to_digit(10))
                .unwrap_or(1) as u64;
            (digit.saturating_sub(1), false)
        } else {
            // Try new-style 'A' command
            let resp2 = self.cmd("rA")?;
            let digit = resp2.chars().last()
                .and_then(|c| c.to_digit(10))
                .unwrap_or(1) as u64;
            (digit.saturating_sub(1), true)
        };

        self.position = pos;
        self.use_new_cmd = new_cmd;
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

impl StateDevice for XLightExcitation {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions { return Err(MmError::UnknownPosition); }
        if self.initialized {
            let letter = self.cmd_letter();
            self.cmd(&format!("{}{}", letter, pos + 1))?;
        }
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> { Ok(self.position) }
    fn get_number_of_positions(&self) -> u64 { self.num_positions }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels.get(pos as usize).cloned().ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self.labels.iter().position(|l| l == label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64;
        self.set_position(pos)
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        if pos >= self.num_positions { return Err(MmError::UnknownPosition); }
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
    fn initialize_old_firmware() {
        let t = MockTransport::new().expect("rE\r", "rE4");
        let mut d = XLightExcitation::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 3);
        assert!(!d.use_new_cmd);
    }

    #[test]
    fn set_position_old_cmd() {
        let t = MockTransport::new()
            .expect("rE\r", "rE1")
            .expect("E3\r", "E3");
        let mut d = XLightExcitation::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position(2).unwrap();
        assert_eq!(d.get_position().unwrap(), 2);
    }

    #[test]
    fn set_position_new_cmd() {
        // rE returns something not starting with "rE" → falls back to rA
        let t = MockTransport::new()
            .expect("rE\r", "ERR")
            .expect("rA\r", "rA2")
            .expect("A5\r", "A5");
        let mut d = XLightExcitation::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert!(d.use_new_cmd);
        d.set_position(4).unwrap();
        assert_eq!(d.get_position().unwrap(), 4);
    }
}
