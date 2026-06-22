/// CoolLED pE-4000 4-channel LED illuminator.
///
/// Protocol (same as pE-300 but 4 channels A–D):
///   `XMODEL\r`         → must contain "pE-4000"
///   `XVER\r`           → version string
///   `CSS?\r`           → "CSS<A6><B6><C6><D6>" each 6 chars `[S/X][N/F][000-100]`
///   `CSN\r`/`CSF\r`   → global on/off
///   `C<ch>I<N>\r`     → set channel intensity (ch = A-D, N = 0-100)
///   `C<ch>S\r`         → select channel
///   `C<ch>X\r`         → deselect channel
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

const CHANNELS: [char; 4] = ['A', 'B', 'C', 'D'];
const WAVELENGTH_LABELS: [[i64; 4]; 4] = [
    [365, 385, 405, 435],
    [460, 470, 490, 500],
    [525, 550, 580, 595],
    [635, 660, 740, 770],
];

#[derive(Debug, Clone, Copy)]
struct Channel {
    id: char,
    intensity: u8,
    selected: bool,
    wavelength: i64,
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
            let idx = (ch as u8 - b'A') as usize;
            let key_wave = format!("Channel{}", ch);
            props
                .define_property(
                    &key_wave,
                    PropertyValue::Integer(WAVELENGTH_LABELS[idx][0]),
                    false,
                )
                .unwrap();
            let allowed: Vec<String> = WAVELENGTH_LABELS[idx]
                .iter()
                .map(|w| w.to_string())
                .collect();
            let allowed_refs: Vec<&str> = allowed.iter().map(String::as_str).collect();
            props.set_allowed_values(&key_wave, &allowed_refs).unwrap();
        }
        props
            .define_property("Global State", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .set_allowed_values("Global State", &["0", "1"])
            .unwrap();
        props
            .define_property("Lock Pod", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("Lock Pod", &["0", "1"]).unwrap();
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
                    wavelength: 365,
                },
                Channel {
                    id: 'B',
                    intensity: 0,
                    selected: false,
                    wavelength: 460,
                },
                Channel {
                    id: 'C',
                    intensity: 0,
                    selected: false,
                    wavelength: 525,
                },
                Channel {
                    id: 'D',
                    intensity: 0,
                    selected: false,
                    wavelength: 635,
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

    fn send_cmd(&mut self, command: &str) -> MmResult<()> {
        let command = format!("{}\r", command);
        self.call_transport(|t| t.send(&command))
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

    fn refresh_css(&mut self) -> MmResult<()> {
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
        Ok(())
    }

    fn refresh_lams(&mut self) -> MmResult<()> {
        let wavelengths = self.call_transport(|t| {
            let mut wavelengths = Vec::new();
            t.send("LAMS\r")?;
            for i in 0..CHANNELS.len() {
                let response = t.receive_line()?;
                if !response.starts_with("LAM") {
                    continue;
                }
                let wavelength = response
                    .get(6..)
                    .unwrap_or("")
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| MmError::InvalidPropertyValue)?;
                wavelengths.push((i, wavelength));
            }
            Ok(wavelengths)
        })?;

        for (i, wavelength) in wavelengths {
            self.channels[i].wavelength = wavelength;
            self.props
                .entry_mut(&format!("Channel{}", CHANNELS[i]))
                .map(|e| e.value = PropertyValue::Integer(wavelength));
        }
        Ok(())
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
        self.cmd("PORT:P=ON")?;
        self.refresh_css()?;
        self.refresh_lams()?;
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
        if name == "Port" && self.initialized {
            return Err(MmError::CanNotSetProperty);
        }

        for ch in CHANNELS {
            let key_int = format!("Intensity{}", ch);
            if name == key_int {
                let raw = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0..=100).contains(&raw) {
                    return Err(MmError::InvalidPropertyValue);
                }
                let v = raw as u8;
                if self.initialized {
                    self.cmd(&format!("C{}I{}", ch, v))?;
                }
                let idx = (ch as u8 - b'A') as usize;
                self.channels[idx].intensity = v;
                return self.props.set(name, PropertyValue::Integer(v as i64));
            }
            let key_sel = format!("Selection{}", ch);
            if name == key_sel {
                let raw = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if raw != 0 && raw != 1 {
                    return Err(MmError::InvalidPropertyValue);
                }
                let selected = raw == 1;
                if self.initialized {
                    let cmd = if selected {
                        format!("C{}S", ch)
                    } else {
                        format!("C{}X", ch)
                    };
                    self.send_cmd(&cmd)?;
                }
                let idx = (ch as u8 - b'A') as usize;
                self.channels[idx].selected = selected;
                return self
                    .props
                    .set(name, PropertyValue::Integer(if selected { 1 } else { 0 }));
            }
            let key_wave = format!("Channel{}", ch);
            if name == key_wave {
                let wavelength = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                let idx = (ch as u8 - b'A') as usize;
                if !WAVELENGTH_LABELS[idx].contains(&wavelength) {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized {
                    self.cmd(&format!("LOAD:{}", wavelength))?;
                }
                self.channels[idx].wavelength = wavelength;
                return self.props.set(name, PropertyValue::Integer(wavelength));
            }
        }
        if name == "Global State" {
            let raw = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            if raw != 0 && raw != 1 {
                return Err(MmError::InvalidPropertyValue);
            }
            let open = raw == 1;
            if self.initialized {
                self.set_open(open)?;
            } else {
                self.global_on = open;
            }
            return self
                .props
                .set(name, PropertyValue::Integer(if open { 1 } else { 0 }));
        }
        if name == "Lock Pod" {
            let locked = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            if locked != 0 && locked != 1 {
                return Err(MmError::InvalidPropertyValue);
            }
            if self.initialized {
                self.cmd(if locked == 1 {
                    "PORT:P=OFF"
                } else {
                    "PORT:P=ON"
                })?;
            }
            return self.props.set(name, PropertyValue::Integer(locked));
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
            .expect("PORT:P=ON\r", "OK")
            // 4 channels: A selected/on/75, B-D not selected/off/0
            .expect("CSS?\r", "CSSASN075BXF000CXF000DXF000")
            .expect("LAMS\r", "LAM:A 405")
            .expect("LAMS\r", "LAM:B 470")
            .expect("LAMS\r", "LAM:C 550")
            .expect("LAMS\r", "LAM:D 660")
    }

    #[test]
    fn initialize() {
        let mut dev = CoolLedPE4000::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(dev.channels[0].selected);
        assert_eq!(dev.channels[0].intensity, 75);
        assert_eq!(dev.channels[0].wavelength, 405);
        assert_eq!(dev.channels[1].wavelength, 470);
        assert_eq!(dev.channels[2].wavelength, 550);
        assert_eq!(dev.channels[3].wavelength, 660);
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
        let t = make_transport().expect("CDI50\r", "OK");
        let mut dev = CoolLedPE4000::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("IntensityD", PropertyValue::Integer(50))
            .unwrap();
        assert_eq!(dev.channels[3].intensity, 50);
    }

    #[test]
    fn selection_write_is_send_only_like_upstream() {
        let t = make_transport();
        let mut dev = CoolLedPE4000::new().with_transport(Box::new(t));
        dev.initialize().unwrap();

        dev.set_property("SelectionA", PropertyValue::Integer(0))
            .unwrap();

        assert!(!dev.channels[0].selected);
        assert_eq!(
            dev.get_property("SelectionA").unwrap(),
            PropertyValue::Integer(0)
        );
    }

    #[test]
    fn channel_wavelength_sends_load() {
        let t = make_transport().expect("LOAD:470\r", "OK");
        let mut dev = CoolLedPE4000::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("ChannelB", PropertyValue::Integer(470))
            .unwrap();
        assert_eq!(
            dev.get_property("ChannelB").unwrap(),
            PropertyValue::Integer(470)
        );
    }

    #[test]
    fn lock_pod_uses_upstream_port_commands() {
        let t = make_transport().expect("PORT:P=OFF\r", "OK");
        let mut dev = CoolLedPE4000::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Lock Pod", PropertyValue::Integer(1))
            .unwrap();
        assert_eq!(
            dev.get_property("Lock Pod").unwrap(),
            PropertyValue::Integer(1)
        );
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

    #[test]
    fn rejects_out_of_range_values_before_writing() {
        let mut dev = CoolLedPE4000::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();

        assert_eq!(
            dev.set_property("IntensityA", PropertyValue::Integer(300)),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            dev.set_property("SelectionA", PropertyValue::Integer(2)),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            dev.set_property("Global State", PropertyValue::Integer(2)),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(dev.channels[0].intensity, 75);
        assert!(dev.channels[0].selected);
        assert!(dev.global_on);
    }

    #[test]
    fn rejects_initialized_port_changes() {
        let mut dev = CoolLedPE4000::new().with_transport(Box::new(make_transport()));
        dev.set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        dev.initialize().unwrap();

        assert_eq!(
            dev.set_property("Port", PropertyValue::String("COM2".into())),
            Err(MmError::CanNotSetProperty)
        );
        assert_eq!(
            dev.get_property("Port").unwrap(),
            PropertyValue::String("COM1".into())
        );
    }
}
