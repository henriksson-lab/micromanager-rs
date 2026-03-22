/// CoolLED pE-300 3-channel LED illuminator.
///
/// Protocol:
///   `XMODEL\r`        → model string (must contain "pE-300")
///   `XVER\r`          → version info
///   `CSS?\r`          → channel status: `CSS<A><B><C>` each 6 chars `[S/X][N/F][000-100]`
///   `CSN\r`/`CSF\r`   → global on/off
///   `CAI<NNN>\r`      → set channel A intensity (000-100)
///   `CBI<NNN>\r`      → set channel B intensity
///   `CCI<NNN>\r`      → set channel C intensity
///   `CAS\r`/`CAX\r`  → select/deselect channel A
///   `CBS\r`/`CBX\r`  → select/deselect channel B
///   `CCS\r`/`CCX\r`  → select/deselect channel C
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

/// One channel: A, B, or C.
#[derive(Debug, Clone, Copy)]
struct Channel {
    id: char,
    intensity: u8,   // 0-100
    selected: bool,
}

pub struct CoolLedPE300 {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    global_on: bool,
    channels: [Channel; 3],
}

impl CoolLedPE300 {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Version", PropertyValue::String(String::new()), true).unwrap();
        for ch in ['A', 'B', 'C'] {
            let key_int = format!("Intensity{}", ch);
            let key_sel = format!("Select{}", ch);
            props.define_property(&key_int, PropertyValue::Integer(0), false).unwrap();
            props.set_property_limits(&key_int, 0.0, 100.0).unwrap();
            props.define_property(&key_sel, PropertyValue::String("Off".into()), false).unwrap();
            props.set_allowed_values(&key_sel, &["On", "Off"]).unwrap();
        }

        Self {
            props,
            transport: None,
            initialized: false,
            global_on: false,
            channels: [
                Channel { id: 'A', intensity: 0, selected: false },
                Channel { id: 'B', intensity: 0, selected: false },
                Channel { id: 'C', intensity: 0, selected: false },
            ],
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

    /// Parse `CSS` response: `CSS<A6><B6><C6>` where each is `[S/X][N/F][000-100]`.
    fn parse_css(resp: &str) -> [(bool, bool, u8); 3] {
        let body = resp.trim().strip_prefix("CSS").unwrap_or(resp.trim());
        let mut result = [(false, false, 0u8); 3];
        for (i, chunk) in body.as_bytes().chunks(6).take(3).enumerate() {
            if chunk.len() >= 6 {
                let selected = chunk[0] == b'S';
                let on       = chunk[1] == b'N';
                let int_str  = std::str::from_utf8(&chunk[2..5]).unwrap_or("0");
                let intensity = int_str.parse::<u8>().unwrap_or(0);
                result[i] = (selected, on, intensity);
            }
        }
        result
    }
}

impl Default for CoolLedPE300 {
    fn default() -> Self { Self::new() }
}

impl Device for CoolLedPE300 {
    fn name(&self) -> &str { "CoolLEDpE300" }
    fn description(&self) -> &str { "CoolLED pE-300 LED illuminator" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        let model = self.cmd("XMODEL")?;
        if !model.contains("pE-300") {
            return Err(MmError::LocallyDefined(
                format!("Unexpected device model: {}", model)
            ));
        }

        let ver = self.cmd("XVER")?;
        self.props.entry_mut("Version").map(|e| e.value = PropertyValue::String(ver));

        let css = self.cmd("CSS?")?;
        let states = Self::parse_css(&css);
        for (i, (sel, on, intensity)) in states.iter().enumerate() {
            self.channels[i].selected = *sel;
            self.channels[i].intensity = *intensity;
            let ch = self.channels[i].id;
            self.props.entry_mut(&format!("Intensity{}", ch))
                .map(|e| e.value = PropertyValue::Integer(*intensity as i64));
            self.props.entry_mut(&format!("Select{}", ch))
                .map(|e| e.value = PropertyValue::String(if *sel { "On".into() } else { "Off".into() }));
            if i == 0 { self.global_on = *on; }
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.cmd("CSF");
            self.global_on = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        // Intensity{A/B/C}
        for ch in ['A', 'B', 'C'] {
            let key = format!("Intensity{}", ch);
            if name == key {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u8;
                if self.initialized {
                    self.cmd(&format!("C{}I{:03}", ch, v))?;
                }
                let idx = (ch as u8 - b'A') as usize;
                self.channels[idx].intensity = v;
                return self.props.set(name, PropertyValue::Integer(v as i64));
            }
            let key_sel = format!("Select{}", ch);
            if name == key_sel {
                let s = val.as_str().to_string();
                if self.initialized {
                    let cmd = if s == "On" { format!("C{}S", ch) } else { format!("C{}X", ch) };
                    self.cmd(&cmd)?;
                }
                let idx = (ch as u8 - b'A') as usize;
                self.channels[idx].selected = s == "On";
                return self.props.set(name, PropertyValue::String(s));
            }
        }
        self.props.set(name, val)
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Shutter }
    fn busy(&self) -> bool { false }
}

impl Shutter for CoolLedPE300 {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open { "CSN" } else { "CSF" };
        self.cmd(cmd)?;
        self.global_on = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.global_on) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> { self.set_open(true) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("XMODEL", "pE-300 v1.0")
            .expect("XVER",   "HW:1.0 FW:2.3")
            // CSS: A=selected/on/50%, B=not selected/off/0%, C=not selected/off/0%
            .expect("CSS?", "CSSSN050XF000XF000")
    }

    #[test]
    fn initialize() {
        let mut dev = CoolLedPE300::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(dev.channels[0].selected);
        assert_eq!(dev.channels[0].intensity, 50);
        assert!(!dev.channels[1].selected);
    }

    #[test]
    fn global_on_off() {
        let t = make_transport().any("OK").any("OK");
        let mut dev = CoolLedPE300::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_intensity_a() {
        let t = make_transport().expect("CAI075", "OK");
        let mut dev = CoolLedPE300::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("IntensityA", PropertyValue::Integer(75)).unwrap();
        assert_eq!(dev.channels[0].intensity, 75);
    }

    #[test]
    fn parse_css_values() {
        let states = CoolLedPE300::parse_css("CSSSN050XF000XF000");
        assert_eq!(states[0], (true,  true,  50));
        assert_eq!(states[1], (false, false,  0));
        assert_eq!(states[2], (false, false,  0));
    }

    #[test]
    fn no_transport_error() {
        assert!(CoolLedPE300::new().initialize().is_err());
    }
}
