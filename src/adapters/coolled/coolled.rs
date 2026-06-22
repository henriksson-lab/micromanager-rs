/// CoolLED pE-300 3-channel LED illuminator.
///
/// Protocol:
///   `XMODEL\r`        → model string (must contain "pE-300")
///   `XVER\r`          → version info
///   `CSS?\r`          → channel status: `CSS<A><B><C>` each 6 chars `[S/X][N/F][000-100]`
///   `CSN\r`/`CSF\r`   → global on/off
///   `CAI<N>\r`        → set channel A intensity (0-100)
///   `CBI<N>\r`        → set channel B intensity
///   `CCI<N>\r`        → set channel C intensity
///   `CAS\r`/`CAX\r`  → select/deselect channel A
///   `CBS\r`/`CBX\r`  → select/deselect channel B
///   `CCS\r`/`CCX\r`  → select/deselect channel C
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::time::Instant;

/// One channel: A, B, or C.
#[derive(Debug, Clone, Copy)]
struct Channel {
    id: char,
    intensity: u8, // 0-100
    selected: bool,
}

pub struct CoolLedPE300 {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    global_on: bool,
    channels: [Channel; 3],
    delay_ms: f64,
    changed_time: Instant,
}

impl CoolLedPE300 {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Version", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Delay_ms", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("Delay_ms", 0.0, f64::MAX)
            .unwrap();
        for ch in ['A', 'B', 'C'] {
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
            ],
            delay_ms: 0.0,
            changed_time: Instant::now(),
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
        let cmd = format!("{}\r", command);
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }

    fn send_cmd(&mut self, command: &str) -> MmResult<()> {
        let cmd = format!("{}\r", command);
        self.call_transport(|t| t.send(&cmd))
    }

    /// Parse `CSS` response: `CSS<ch><S/X><N/F><000-100>...`.
    fn parse_css(resp: &str) -> [(bool, bool, u8); 3] {
        let body = resp.trim().strip_prefix("CSS").unwrap_or(resp.trim());
        let mut result = [(false, false, 0u8); 3];
        for (i, chunk) in body.as_bytes().chunks(6).take(3).enumerate() {
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

    fn read_version_description(&mut self) -> MmResult<()> {
        let (mainboard, pod) = self.call_transport(|t| {
            t.send("XVER\r")?;
            let mainboard = t.receive_line()?.trim().to_string();
            let _hardware = t.receive_line()?;
            let _data = t.receive_line()?;
            let pod = t.receive_line()?.trim().to_string();
            Ok((mainboard, pod))
        })?;

        let mainboard_version = mainboard.get(8..).unwrap_or(&mainboard);
        let pod_version = pod.get(8..).unwrap_or(&pod);
        let description = format!(
            "CoolLED pE300. Mainboard: v{} Pod: v{}",
            mainboard_version, pod_version
        );
        self.props
            .define_property("Description", PropertyValue::String(description), true)?;
        self.props
            .entry_mut("Version")
            .map(|e| e.value = PropertyValue::String(mainboard));
        Ok(())
    }
}

impl Default for CoolLedPE300 {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CoolLedPE300 {
    fn name(&self) -> &str {
        "pE300"
    }
    fn description(&self) -> &str {
        "pE300 LED illuminator"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        let model = self.cmd("XMODEL")?;
        if !model.contains("pE-300") {
            return Err(MmError::LocallyDefined(format!(
                "Unexpected device model: {}",
                model
            )));
        }

        self.read_version_description()?;

        self.cmd("PORT:P=ON")?;
        self.refresh_css()?;

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

        // Intensity{A/B/C}
        for ch in ['A', 'B', 'C'] {
            let key = format!("Intensity{}", ch);
            if name == key {
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
        if name == "Delay_ms" {
            let delay = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            self.props.set(name, PropertyValue::Float(delay))?;
            self.delay_ms = delay;
            return Ok(());
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
        self.changed_time.elapsed().as_secs_f64() * 1000.0 < self.delay_ms
    }
}

impl Shutter for CoolLedPE300 {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open { "CSN" } else { "CSF" };
        self.cmd(cmd)?;
        self.global_on = open;
        self.changed_time = Instant::now();
        self.props
            .entry_mut("Global State")
            .map(|e| e.value = PropertyValue::Integer(if open { 1 } else { 0 }));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.global_on)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("XMODEL\r", "pE-300 v1.0")
            .expect("XVER\r", "MainBd: 2.3")
            .expect("XVER\r", "Hardware: 1.0")
            .expect("XVER\r", "Data: 1.1")
            .expect("XVER\r", "PodVer: 4.5")
            .expect("PORT:P=ON\r", "OK")
            // CSS: A=selected/on/50%, B=not selected/off/0%, C=not selected/off/0%
            .expect("CSS?\r", "CSSASN050BXF000CXF000")
    }

    #[test]
    fn initialize() {
        let mut dev = CoolLedPE300::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(dev.channels[0].selected);
        assert_eq!(dev.channels[0].intensity, 50);
        assert!(!dev.channels[1].selected);
        assert_eq!(
            dev.get_property("Description").unwrap(),
            PropertyValue::String("CoolLED pE300. Mainboard: v2.3 Pod: v4.5".into())
        );
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
        let t = make_transport().expect("CAI75\r", "OK");
        let mut dev = CoolLedPE300::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("IntensityA", PropertyValue::Integer(75))
            .unwrap();
        assert_eq!(dev.channels[0].intensity, 75);
    }

    #[test]
    fn selection_write_is_send_only_like_upstream() {
        let t = make_transport();
        let mut dev = CoolLedPE300::new().with_transport(Box::new(t));
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
    fn lock_pod_uses_upstream_port_commands() {
        let t = make_transport().expect("PORT:P=OFF\r", "OK");
        let mut dev = CoolLedPE300::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Lock Pod", PropertyValue::Integer(1))
            .unwrap();
        assert_eq!(
            dev.get_property("Lock Pod").unwrap(),
            PropertyValue::Integer(1)
        );
    }

    #[test]
    fn delay_controls_busy_after_state_change() {
        let t = make_transport().expect("CSF\r", "OK");
        let mut dev = CoolLedPE300::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Delay_ms", PropertyValue::Float(1000.0))
            .unwrap();
        dev.set_open(false).unwrap();
        assert!(dev.busy());
    }

    #[test]
    fn rejects_negative_delay() {
        let mut dev = CoolLedPE300::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();

        assert_eq!(
            dev.set_property("Delay_ms", PropertyValue::Float(-1.0)),
            Err(MmError::InvalidPropertyValue)
        );
    }

    #[test]
    fn shutdown_does_not_force_global_off() {
        let mut dev = CoolLedPE300::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        dev.shutdown().unwrap();
        assert!(dev.get_open().unwrap());
    }

    #[test]
    fn fire_is_unsupported() {
        let mut dev = CoolLedPE300::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert_eq!(dev.fire(1.0).unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn parse_css_values() {
        let states = CoolLedPE300::parse_css("CSSASN050BXF000CXF000");
        assert_eq!(states[0], (true, true, 50));
        assert_eq!(states[1], (false, false, 0));
        assert_eq!(states[2], (false, false, 0));
    }

    #[test]
    fn no_transport_error() {
        assert!(CoolLedPE300::new().initialize().is_err());
    }

    #[test]
    fn rejects_out_of_range_values_before_writing() {
        let mut dev = CoolLedPE300::new().with_transport(Box::new(make_transport()));
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
        assert_eq!(dev.channels[0].intensity, 50);
        assert!(dev.channels[0].selected);
        assert!(dev.global_on);
    }

    #[test]
    fn rejects_initialized_port_changes() {
        let mut dev = CoolLedPE300::new().with_transport(Box::new(make_transport()));
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
