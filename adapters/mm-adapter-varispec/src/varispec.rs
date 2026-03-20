/// Cambridge Research & Instrumentation VariSpec liquid-crystal tunable filter.
///
/// Protocol (TX `\r`, echo-back, RX until `\r\n`):
///   `B0\r`           → init: go to band-pass mode
///   `G0\r`           → init: transmit mode
///   `I1\r`           → init: enable
///   `E1\r`           → init: enable output
///   `V?\r`           → "V <rev> <min_wl> <max_wl> <serial>"
///   `W?\r`           → "W <nm.nnn>"   (current wavelength)
///   `W <nm.nnn>\r`   → set wavelength
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, StateDevice};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

pub struct VarispecLCTF {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    wavelength_nm: f64,
    min_nm: f64,
    max_nm: f64,
}

impl VarispecLCTF {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Wavelength_nm", PropertyValue::Float(550.0), false).unwrap();
        props.define_property("MinWavelength_nm", PropertyValue::Float(400.0), true).unwrap();
        props.define_property("MaxWavelength_nm", PropertyValue::Float(720.0), true).unwrap();
        props.define_property("FirmwareVersion", PropertyValue::String(String::new()), true).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            wavelength_nm: 550.0,
            min_nm: 400.0,
            max_nm: 720.0,
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
        let c = format!("{}\r", command);
        self.call_transport(|t| { let r = t.send_recv(&c)?; Ok(r.trim().to_string()) })
    }

    /// Parse "V <rev> <min_wl> <max_wl> <serial>" → (rev, min, max)
    fn parse_version(resp: &str) -> Option<(String, f64, f64)> {
        let parts: Vec<&str> = resp.trim().split_whitespace().collect();
        if parts.len() >= 4 && parts[0] == "V" {
            let min: f64 = parts[2].parse().ok()?;
            let max: f64 = parts[3].parse().ok()?;
            Some((parts[1].to_string(), min, max))
        } else {
            None
        }
    }

    /// Parse "W <nm>" → nm
    fn parse_wavelength(resp: &str) -> Option<f64> {
        let parts: Vec<&str> = resp.trim().split_whitespace().collect();
        if parts.len() >= 2 && parts[0] == "W" {
            parts[1].parse().ok()
        } else {
            None
        }
    }
}

impl Default for VarispecLCTF { fn default() -> Self { Self::new() } }

impl Device for VarispecLCTF {
    fn name(&self) -> &str { "VarispecLCTF" }
    fn description(&self) -> &str { "CRI VariSpec liquid-crystal tunable filter" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let _ = self.cmd("B0");
        let _ = self.cmd("G0");
        let _ = self.cmd("I1");
        let _ = self.cmd("E1");
        if let Ok(r) = self.cmd("V?") {
            if let Some((rev, min, max)) = Self::parse_version(&r) {
                self.min_nm = min;
                self.max_nm = max;
                self.props.entry_mut("FirmwareVersion").map(|e| e.value = PropertyValue::String(rev));
                self.props.entry_mut("MinWavelength_nm").map(|e| e.value = PropertyValue::Float(min));
                self.props.entry_mut("MaxWavelength_nm").map(|e| e.value = PropertyValue::Float(max));
            }
        }
        if let Ok(r) = self.cmd("W?") {
            if let Some(wl) = Self::parse_wavelength(&r) {
                self.wavelength_nm = wl;
                self.props.entry_mut("Wavelength_nm").map(|e| e.value = PropertyValue::Float(wl));
            }
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if name == "Wavelength_nm" { return Ok(PropertyValue::Float(self.wavelength_nm)); }
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Wavelength_nm" {
            let nm = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            if self.initialized {
                self.cmd(&format!("W {:.3}", nm))?;
            }
            self.wavelength_nm = nm;
            self.props.entry_mut("Wavelength_nm").map(|e| e.value = PropertyValue::Float(nm));
            return Ok(());
        }
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

impl StateDevice for VarispecLCTF {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        self.set_property("Wavelength_nm", PropertyValue::Float(pos as f64))
    }
    fn get_position(&self) -> MmResult<u64> { Ok(self.wavelength_nm as u64) }
    fn get_number_of_positions(&self) -> u64 {
        (self.max_nm - self.min_nm) as u64 + 1
    }
    fn get_position_label(&self, pos: u64) -> MmResult<String> { Ok(format!("{} nm", pos)) }
    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let nm: f64 = label.trim_end_matches(" nm").parse().map_err(|_| MmError::UnknownLabel(label.to_string()))?;
        self.set_property("Wavelength_nm", PropertyValue::Float(nm))
    }
    fn set_position_label(&mut self, _pos: u64, _label: &str) -> MmResult<()> { Ok(()) }
    fn set_gate_open(&mut self, _open: bool) -> MmResult<()> { Ok(()) }
    fn get_gate_open(&self) -> MmResult<bool> { Ok(true) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any("OK")                          // B0
            .any("OK")                          // G0
            .any("OK")                          // I1
            .any("OK")                          // E1
            .any("V 1.2 400.0 720.0 SN12345")   // V?
            .any("W 550.000")                   // W?
    }

    #[test]
    fn initialize() {
        let mut dev = VarispecLCTF::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert_eq!(dev.wavelength_nm, 550.0);
        assert_eq!(dev.min_nm, 400.0);
        assert_eq!(dev.max_nm, 720.0);
    }

    #[test]
    fn set_wavelength() {
        let t = make_transport().any("OK");
        let mut dev = VarispecLCTF::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Wavelength_nm", PropertyValue::Float(600.0)).unwrap();
        assert_eq!(dev.wavelength_nm, 600.0);
    }

    #[test]
    fn parse_version_ok() {
        let (rev, min, max) = VarispecLCTF::parse_version("V 1.2 400.0 720.0 SN12345").unwrap();
        assert_eq!(rev, "1.2");
        assert_eq!(min, 400.0);
        assert_eq!(max, 720.0);
    }

    #[test]
    fn parse_wavelength_ok() {
        assert_eq!(VarispecLCTF::parse_wavelength("W 550.000"), Some(550.0));
    }

    #[test]
    fn no_transport_error() { assert!(VarispecLCTF::new().initialize().is_err()); }
}
