/// CoolLED pE-4000 4-channel LED illuminator.
///
/// Protocol (same as pE-300 but 4 channels A–D):
///   `XMODEL\r`         → must contain "pE-4000"
///   `XVER\r`           → version string
///   `CSS?\r`           → "CSS<A6><B6><C6><D6>" each 6 chars `[S/X][N/F][000-100]`
///   `CSN\r`/`CSF\r`   → global on/off
///   `C<ch>I<NNN>\r`   → set channel intensity (ch = A–D, NNN = 000-100)
///   `C<ch>S\r`         → select channel
///   `C<ch>X\r`         → deselect channel
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

const CHANNELS: [char; 4] = ['A', 'B', 'C', 'D'];

#[derive(Debug, Clone, Copy)]
struct Channel {
    id: char,
    intensity: u8,
    selected: bool,
}

pub struct CoolLedPE4000 {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    global_on: bool,
    channels: [Channel; 4],
}

impl CoolLedPE4000 {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Version", PropertyValue::String(String::new()), true)
            .unwrap();
        for ch in CHANNELS {
            let key_int = format!("Intensity{}", ch);
            let key_sel = format!("Selection{}", ch);
            props
                .define_property(&key_int, PropertyValue::Integer(0), false)
                .unwrap();
            props.set_property_limits(&key_int, 0.0, 100.0).unwrap();
            props
                .define_property(&key_sel, PropertyValue::Integer(0), false)
                .unwrap();
            props.set_allowed_values(&key_sel, &["0", "1"]).unwrap();
        }
        props
            .define_property("Global State", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .set_allowed_values("Global State", &["0", "1"])
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            global_on: false,
            channels: [
                Channel {
                    id: 'A',
                    intensity: 0,
                    selected: false,
                },
                Channel {
                    id: 'B',
                    intensity: 0,
                    selected: false,
                },
                Channel {
                    id: 'C',
                    intensity: 0,
                    selected: false,
                },
                Channel {
                    id: 'D',
                    intensity: 0,
                    selected: false,
                },
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
        let command = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&command)?;
            Ok(r.trim().to_string())
        })
    }

    /// Parse CSS response: `CSS<ch><S/X><N/F><000-100>...`.
    fn parse_css(resp: &str) -> [(bool, bool, u8); 4] {
        let body = resp.trim().strip_prefix("CSS").unwrap_or(resp.trim());
        let mut result = [(false, false, 0u8); 4];
        for (i, chunk) in body.as_bytes().chunks(6).take(4).enumerate() {
            if chunk.len() >= 6 {
                let selected = chunk[1] == b'S';
                let on = chunk[2] == b'N';
                let int_str = std::str::from_utf8(&chunk[3..6]).unwrap_or("0");
                let intensity = int_str.parse::<u8>().unwrap_or(0);
                result[i] = (selected, on, intensity);
            }
        }
        result
    }
}

impl Default for CoolLedPE4000 {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CoolLedPE4000 {
    fn name(&self) -> &str {
        "pE4000"
    }
    fn description(&self) -> &str {
        "pE4000 LED illuminator"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let model = self.cmd("XMODEL")?;
        if !model.contains("pE-4000") {
            return Err(MmError::LocallyDefined(format!(
                "Unexpected model: {}",
                model
            )));
        }
        let ver = self.cmd("XVER")?;
        self.props
            .entry_mut("Version")
            .map(|e| e.value = PropertyValue::String(ver));
        let css = self.cmd("CSS?")?;
        let states = Self::parse_css(&css);
        self.global_on = false;
        for (i, (sel, on, intensity)) in states.iter().enumerate() {
            self.channels[i].selected = *sel;
            self.channels[i].intensity = *intensity;
            let ch = self.channels[i].id;
            self.props
                .entry_mut(&format!("Intensity{}", ch))
                .map(|e| e.value = PropertyValue::Integer(*intensity as i64));
            self.props
                .entry_mut(&format!("Selection{}", ch))
                .map(|e| e.value = PropertyValue::Integer(if *sel { 1 } else { 0 }));
            self.global_on |= *on;
        }
        self.props
            .entry_mut("Global State")
            .map(|e| e.value = PropertyValue::Integer(if self.global_on { 1 } else { 0 }));
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        for ch in CHANNELS {
            let key_int = format!("Intensity{}", ch);
            if name == key_int {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u8;
                if self.initialized {
                    self.cmd(&format!("C{}I{:03}", ch, v))?;
                }
                let idx = (ch as u8 - b'A') as usize;
                self.channels[idx].intensity = v;
                return self.props.set(name, PropertyValue::Integer(v as i64));
            }
            let key_sel = format!("Selection{}", ch);
            if name == key_sel {
                let selected = val.as_i64().ok_or(MmError::InvalidPropertyValue)? != 0;
                if self.initialized {
                    let cmd = if selected {
                        format!("C{}S", ch)
                    } else {
                        format!("C{}X", ch)
                    };
                    self.cmd(&cmd)?;
                }
                let idx = (ch as u8 - b'A') as usize;
                self.channels[idx].selected = selected;
                return self
                    .props
                    .set(name, PropertyValue::Integer(if selected { 1 } else { 0 }));
            }
        }
        if name == "Global State" {
            let open = val.as_i64().ok_or(MmError::InvalidPropertyValue)? != 0;
            if self.initialized {
                self.set_open(open)?;
            } else {
                self.global_on = open;
            }
            return self
                .props
                .set(name, PropertyValue::Integer(if open { 1 } else { 0 }));
        }
        self.props.set(name, val)
    }

    fn property_names(&self) -> Vec<String> {
        self.props.property_names().to_vec()
    }
    fn has_property(&self, name: &str) -> bool {
        self.props.has_property(name)
    }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Shutter
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Shutter for CoolLedPE4000 {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.cmd(if open { "CSN" } else { "CSF" })?;
        self.global_on = open;
        self.props
            .entry_mut("Global State")
            .map(|e| e.value = PropertyValue::Integer(if open { 1 } else { 0 }));
        Ok(())
    }
    fn get_open(&self) -> MmResult<bool> {
        Ok(self.global_on)
    }
    fn fire(&mut self, _dt: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("XMODEL\r", "pE-4000 v1.0")
            .expect("XVER\r", "HW:2.0 FW:3.1")
            // 4 channels: A selected/on/75, B-D not selected/off/0
            .expect("CSS?\r", "CSSASN075BXF000CXF000DXF000")
    }

    #[test]
    fn initialize() {
        let mut dev = CoolLedPE4000::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(dev.channels[0].selected);
        assert_eq!(dev.channels[0].intensity, 75);
        assert!(!dev.channels[1].selected);
        assert!(!dev.channels[3].selected);
    }

    #[test]
    fn global_on_off() {
        let t = make_transport().any("OK").any("OK");
        let mut dev = CoolLedPE4000::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_intensity_d() {
        let t = make_transport().expect("CDI050\r", "OK");
        let mut dev = CoolLedPE4000::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("IntensityD", PropertyValue::Integer(50))
            .unwrap();
        assert_eq!(dev.channels[3].intensity, 50);
    }

    #[test]
    fn shutdown_does_not_force_global_off() {
        let mut dev = CoolLedPE4000::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        dev.shutdown().unwrap();
        assert!(dev.get_open().unwrap());
    }

    #[test]
    fn fire_is_unsupported() {
        let mut dev = CoolLedPE4000::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert_eq!(dev.fire(1.0).unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn parse_css_four_channels() {
        let states = CoolLedPE4000::parse_css("CSSASN075BXF000CXF000DXF000");
        assert_eq!(states[0], (true, true, 75));
        assert_eq!(states[1], (false, false, 0));
        assert_eq!(states[2], (false, false, 0));
        assert_eq!(states[3], (false, false, 0));
    }

    #[test]
    fn no_transport_error() {
        assert!(CoolLedPE4000::new().initialize().is_err());
    }
}
