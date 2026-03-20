/// ASI FW-1000 filter wheel controller.
///
/// Protocol (ASCII, terminated with space or `\r`):
///   `VN\r`        → version string (≥3 chars)
///   `NF\r`        → number of filter positions (6 or 8)
///   `FW\r`        → current wheel number (0 or 1)
///   `FW <n>\r`    → select wheel n
///   `MP\r`        → current filter position (0-indexed digit)
///   `MP <n>\r`    → move to filter position n (0-indexed)
///   `?\r`         → busy status ('3' = busy, other = idle)
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, StateDevice};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

pub struct AsiFW1000 {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position: u64,
    num_positions: u64,
    labels: Vec<String>,
    gate_open: bool,
}

impl AsiFW1000 {
    pub fn new() -> Self {
        let num_positions: u64 = 8;
        let labels: Vec<String> = (0..num_positions).map(|i| format!("Filter-{}", i)).collect();
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("State", PropertyValue::Integer(0), false).unwrap();
        props.define_property("Label", PropertyValue::String("Filter-0".into()), false).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            position: 0,
            num_positions,
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

impl Default for AsiFW1000 {
    fn default() -> Self { Self::new() }
}

impl Device for AsiFW1000 {
    fn name(&self) -> &str { "ASI-FW1000" }
    fn description(&self) -> &str { "ASI FW-1000 filter wheel" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        let ver = self.cmd("VN")?;
        if ver.len() < 2 {
            return Err(MmError::LocallyDefined("No version response".into()));
        }

        let nf = self.cmd("NF")?;
        let n: u64 = nf.trim().parse().unwrap_or(8);
        self.num_positions = n;
        // Resize labels if needed
        self.labels = (0..n).map(|i| format!("Filter-{}", i)).collect();

        let pos_str = self.cmd("MP")?;
        self.position = pos_str.trim().parse().unwrap_or(0);

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

impl StateDevice for AsiFW1000 {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized {
            self.cmd(&format!("MP {}", pos))?;
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
        if pos >= self.num_positions {
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
    use mm_device::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("VN", "FW-1000 v2.1")
            .expect("NF", "8")
            .expect("MP", "0")
    }

    #[test]
    fn initialize() {
        let mut fw = AsiFW1000::new().with_transport(Box::new(make_transport()));
        fw.initialize().unwrap();
        assert_eq!(fw.get_position().unwrap(), 0);
        assert_eq!(fw.get_number_of_positions(), 8);
    }

    #[test]
    fn set_position() {
        let t = make_transport().expect("MP 5", "5");
        let mut fw = AsiFW1000::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_position(5).unwrap();
        assert_eq!(fw.get_position().unwrap(), 5);
    }

    #[test]
    fn out_of_range_rejected() {
        let mut fw = AsiFW1000::new().with_transport(Box::new(make_transport()));
        fw.initialize().unwrap();
        assert!(fw.set_position(8).is_err());
    }

    #[test]
    fn label_navigation() {
        let t = make_transport().any("3");
        let mut fw = AsiFW1000::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_position_label(3, "DAPI").unwrap();
        fw.set_position_by_label("DAPI").unwrap();
        assert_eq!(fw.get_position().unwrap(), 3);
    }

    #[test]
    fn no_transport_error() {
        assert!(AsiFW1000::new().initialize().is_err());
    }
}
