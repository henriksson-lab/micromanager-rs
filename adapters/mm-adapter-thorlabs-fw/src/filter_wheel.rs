/// Thorlabs motorized filter wheel.
///
/// Protocol (ASCII, `\r` terminated):
///   `sensors=0\r`  → disable sensor mode
///   `pos?\r`        → current position (1-indexed integer)
///   `pos=<N>\r`     → move to position N (1-indexed, 1–6)
///
/// Positions are 1-indexed in commands but 0-indexed in the StateDevice API.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const NUM_POSITIONS: u64 = 6;

pub struct ThorlabsFilterWheel {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position: u64,
    labels: Vec<String>,
    gate_open: bool,
}

impl ThorlabsFilterWheel {
    pub fn new() -> Self {
        let labels: Vec<String> = (1..=NUM_POSITIONS).map(|i| format!("Filter-{}", i)).collect();
        let mut props = PropertyMap::new();
        props.define_property("State", PropertyValue::Integer(0), false).unwrap();
        props.define_property("Label", PropertyValue::String("Filter-1".into()), false).unwrap();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            position: 0,
            labels,
            gate_open: true,
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

impl Default for ThorlabsFilterWheel {
    fn default() -> Self { Self::new() }
}

impl Device for ThorlabsFilterWheel {
    fn name(&self) -> &str { "ThorlabsFilterWheel" }
    fn description(&self) -> &str { "Thorlabs motorized filter wheel" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let _ = self.cmd("sensors=0");
        let pos_str = self.cmd("pos?")?;
        let pos_1indexed: u64 = pos_str.trim().parse()
            .map_err(|_| MmError::LocallyDefined(format!("Bad pos: {}", pos_str)))?;
        self.position = pos_1indexed.saturating_sub(1); // convert to 0-indexed
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

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

impl StateDevice for ThorlabsFilterWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= NUM_POSITIONS {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized {
            let resp = self.cmd(&format!("pos={}", pos + 1))?; // 1-indexed in command
            // Response echoes the new position
            let _ = resp;
        }
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> { Ok(self.position) }

    fn get_number_of_positions(&self) -> u64 { NUM_POSITIONS }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels.get(pos as usize).cloned().ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self.labels.iter().position(|l| l == label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64;
        self.set_position(pos)
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        if pos >= NUM_POSITIONS {
            return Err(MmError::UnknownPosition);
        }
        self.labels[pos as usize] = label.to_string();
        Ok(())
    }

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> { Ok(self.gate_open) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize_reads_position() {
        let t = MockTransport::new().any("OK").expect("pos?", "3");
        let mut fw = ThorlabsFilterWheel::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        assert_eq!(fw.get_position().unwrap(), 2); // 0-indexed: pos 3 → index 2
    }

    #[test]
    fn set_position() {
        let t = MockTransport::new()
            .any("OK").expect("pos?", "1")
            .expect("pos=4", "4");
        let mut fw = ThorlabsFilterWheel::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_position(3).unwrap(); // pos 3 (0-indexed) → command pos=4
        assert_eq!(fw.get_position().unwrap(), 3);
    }

    #[test]
    fn out_of_range_rejected() {
        let t = MockTransport::new().any("OK").any("1");
        let mut fw = ThorlabsFilterWheel::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        assert!(fw.set_position(6).is_err());
    }

    #[test]
    fn label_navigation() {
        let t = MockTransport::new()
            .any("OK").any("1")
            .any("2");
        let mut fw = ThorlabsFilterWheel::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_position_label(1, "FITC").unwrap();
        fw.set_position_by_label("FITC").unwrap();
        assert_eq!(fw.get_position().unwrap(), 1);
    }

    #[test]
    fn no_transport_error() {
        assert!(ThorlabsFilterWheel::new().initialize().is_err());
    }
}
