/// ASI FW1000 Filter Wheel.
///
/// Protocol (TX `\n\r`, RX echo of command + data):
///   Responses echo the command; data follows after the echo.
///   `VN \n\r`         → "VN <version>\n\r"  firmware version
///   `VB 6\n\r`        → echo              set verbose=6 (disables prompts)
///   `FW<n>\n\r`       → echo              select wheel 0 or 1
///   `NF\n\r`          → "NF <N>\n\r"      number of filter positions
///   `MP\n\r`          → "MP <pos>\n\r"    current position (0-based)
///   `MP <pos>\n\r`    → echo              set position (0-based)
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct AsiFw1000FilterWheel {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    wheel: u8,
    num_positions: u64,
    position: u64,
    labels: Vec<String>,
}

impl AsiFw1000FilterWheel {
    pub fn new(wheel: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Wheel", PropertyValue::Integer(wheel as i64), false).unwrap();
        props.define_property("FirmwareVersion", PropertyValue::String(String::new()), true).unwrap();
        let num = 6u64;
        let labels: Vec<String> = (0..num).map(|i| format!("Filter-{}", i + 1)).collect();
        Self {
            props,
            transport: None,
            initialized: false,
            wheel,
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

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let full = format!("{}\n\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    /// Parse the data field from an echo response "CMD <data>".
    fn parse_last_word(resp: &str) -> &str {
        resp.split_whitespace().last().unwrap_or("")
    }
}

impl Default for AsiFw1000FilterWheel { fn default() -> Self { Self::new(0) } }

impl Device for AsiFw1000FilterWheel {
    fn name(&self) -> &str { "AsiFw1000FilterWheel" }
    fn description(&self) -> &str { "ASI FW1000 Filter Wheel" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Get firmware version
        let ver_resp = self.cmd("VN ")?;
        let ver = Self::parse_last_word(&ver_resp).to_string();
        self.props.entry_mut("FirmwareVersion").map(|e| e.value = PropertyValue::String(ver));
        // Set verbose to suppress prompts
        self.cmd("VB 6")?;
        // Select wheel
        self.cmd(&format!("FW{}", self.wheel))?;
        // Query number of positions
        let nf_resp = self.cmd("NF")?;
        let n: u64 = Self::parse_last_word(&nf_resp).parse().unwrap_or(6);
        self.num_positions = n;
        self.labels = (0..n).map(|i| format!("Filter-{}", i + 1)).collect();
        // Query current position
        let mp_resp = self.cmd("MP")?;
        self.position = Self::parse_last_word(&mp_resp).parse().unwrap_or(0);
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

impl StateDevice for AsiFw1000FilterWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!("Position {} out of range", pos)));
        }
        self.cmd(&format!("MP {}", pos))?;
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
    use micromanager::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect("VN \n\r", "VN 1.23")   // firmware version
            .expect("VB 6\n\r", "VB 6")     // set verbose
            .expect("FW0\n\r", "FW0")       // select wheel 0
            .expect("NF\n\r", "NF 6")       // 6 positions
            .expect("MP\n\r", "MP 0")       // current position 0
    }

    #[test]
    fn initialize() {
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(make_init_transport()));
        w.initialize().unwrap();
        assert_eq!(w.get_number_of_positions(), 6);
        assert_eq!(w.get_position().unwrap(), 0);
    }

    #[test]
    fn set_position() {
        let t = make_init_transport().expect("MP 3\n\r", "MP 3");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position(3).unwrap();
        assert_eq!(w.get_position().unwrap(), 3);
    }

    #[test]
    fn label_roundtrip() {
        let t = make_init_transport().expect("MP 2\n\r", "MP 2");
        let mut w = AsiFw1000FilterWheel::new(0).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position_label(2, "DAPI").unwrap();
        assert_eq!(w.get_position_label(2).unwrap(), "DAPI");
        w.set_position_by_label("DAPI").unwrap();
        assert_eq!(w.get_position().unwrap(), 2);
    }

    #[test]
    fn no_transport_error() { assert!(AsiFw1000FilterWheel::new(0).initialize().is_err()); }
}
