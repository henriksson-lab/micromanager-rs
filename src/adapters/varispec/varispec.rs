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
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;
use std::time::{Duration, Instant};

pub struct VarispecLCTF {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    wavelength_nm: f64,
    min_nm: f64,
    max_nm: f64,
    delay_ms: f64,
    changed_time: Option<Instant>,
    last_response: String,
    wavelength_sequence: Vec<f64>,
}

enum VarispecStatus {
    Ready,
    NeedsExercise,
    NeedsInitialization,
}

impl VarispecLCTF {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Baud Rate", PropertyValue::String("9600".into()), false)
            .unwrap();
        props
            .set_allowed_values("Baud Rate", &["115200", "9600"])
            .unwrap();
        props
            .define_property("Wavelength", PropertyValue::Float(546.0), false)
            .unwrap();
        props
            .define_property("Wavelength_nm", PropertyValue::Float(546.0), false)
            .unwrap();
        props
            .define_property("MinWavelength_nm", PropertyValue::Float(400.0), true)
            .unwrap();
        props
            .define_property("MaxWavelength_nm", PropertyValue::Float(720.0), true)
            .unwrap();
        props
            .define_property(
                "FirmwareVersion",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Version Number",
                PropertyValue::String("Version Number Not Found".into()),
                true,
            )
            .unwrap();
        props
            .define_property("Device Delay (ms.)", PropertyValue::Float(200.0), false)
            .unwrap();
        props
            .set_property_limits("Device Delay (ms.)", 0.0, 200.0)
            .unwrap();
        props
            .define_property(
                "String send to VarispecLCTF",
                PropertyValue::String(String::new()),
                false,
            )
            .unwrap();
        props
            .define_property(
                "String from VarispecLCTF",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            wavelength_nm: 546.0,
            min_nm: 400.0,
            max_nm: 720.0,
            delay_ms: 200.0,
            changed_time: None,
            last_response: String::new(),
            wavelength_sequence: Vec::new(),
        }
    }

    pub fn with_transport(self, t: Box<dyn Transport>) -> Self {
        {
            *self.transport.borrow_mut() = Some(t);
        }
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        let mut transport = self.transport.borrow_mut();
        match transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn send_cmd(&self, command: &str) -> MmResult<()> {
        self.call_transport(|t| {
            t.send(&format!("{}\r", command))?;
            let echo = t.receive_line()?.trim().to_string();
            if echo == command {
                Ok(())
            } else {
                Err(MmError::SerialInvalidResponse)
            }
        })
    }

    fn query_cmd(&self, command: &str) -> MmResult<String> {
        self.send_cmd(command)?;
        self.call_transport(|t| Ok(t.receive_line()?.trim().to_string()))
    }

    fn status_bytes(&self, command: u8) -> MmResult<Vec<u8>> {
        self.call_transport(|t| {
            t.send_bytes(&[command])?;
            let bytes = t.receive_bytes(2)?;
            if bytes.len() == 2 && bytes[0] == command {
                Ok(bytes)
            } else {
                Err(MmError::SerialInvalidResponse)
            }
        })
    }

    fn read_status(&self) -> MmResult<VarispecStatus> {
        let bytes = self.status_bytes(b'@')?;
        let status = bytes[1];
        if status & 0x20 != 0 {
            self.query_cmd("R?")?;
            self.send_cmd("R1")?;
            return Err(MmError::Err);
        }
        if status & 0x02 == 0 || status & 0x01 == 0 {
            if status & 0x02 == 0 {
                return Ok(VarispecStatus::NeedsExercise);
            }
            return Ok(VarispecStatus::NeedsInitialization);
        }
        Ok(VarispecStatus::Ready)
    }

    fn get_status(&self) -> MmResult<()> {
        match self.read_status()? {
            VarispecStatus::Ready => Ok(()),
            VarispecStatus::NeedsExercise | VarispecStatus::NeedsInitialization => {
                Err(MmError::Err)
            }
        }
    }

    fn reports_busy(&self) -> MmResult<bool> {
        let bytes = self.status_bytes(b'!')?;
        match bytes[1] {
            b'<' => Ok(true),
            b'>' => Ok(false),
            _ => Ok(false),
        }
    }

    fn wait_until_ready(&self) -> MmResult<()> {
        for _ in 0..100 {
            if !self.reports_busy()? {
                return Ok(());
            }
        }
        Err(MmError::SerialTimeout)
    }

    /// Parse "V <rev> <min_wl> <max_wl> <serial>" -> (rev, min, max)
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

    /// Parse "W <nm>" -> nm
    fn parse_wavelength(resp: &str) -> Option<f64> {
        let parts: Vec<&str> = resp.trim().split_whitespace().collect();
        if parts.len() >= 2 && parts[0] == "W" {
            parts[1].parse().ok()
        } else {
            None
        }
    }

    pub fn clear_wavelength_sequence(&mut self) -> MmResult<()> {
        self.wavelength_sequence.clear();
        Ok(())
    }

    pub fn add_to_wavelength_sequence(&mut self, nm: f64) -> MmResult<()> {
        if self.wavelength_sequence.len() >= 128 {
            return Err(MmError::SequenceTooLarge);
        }
        if nm < self.min_nm || nm > self.max_nm {
            return Err(MmError::InvalidPropertyValue);
        }
        self.wavelength_sequence.push(nm);
        Ok(())
    }

    pub fn load_wavelength_sequence(&mut self) -> MmResult<()> {
        if !self.initialized {
            return Err(MmError::NotConnected);
        }
        self.send_cmd("C1")?;
        let sequence = self.wavelength_sequence.clone();
        for (index, nm) in sequence.into_iter().enumerate() {
            self.send_cmd(&format!("D{:.3},{}", nm, index))?;
        }
        self.get_status()
    }

    pub fn start_wavelength_sequence(&mut self) -> MmResult<()> {
        if !self.initialized {
            return Err(MmError::NotConnected);
        }
        self.send_cmd("M0")?;
        self.send_cmd("G1")?;
        self.send_cmd("P0")?;
        Ok(())
    }

    pub fn stop_wavelength_sequence(&mut self) -> MmResult<()> {
        if !self.initialized {
            return Err(MmError::NotConnected);
        }
        self.send_cmd("G0")?;
        self.send_cmd("P0")?;
        Ok(())
    }
}

impl Default for VarispecLCTF {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for VarispecLCTF {
    fn name(&self) -> &str {
        "VarispecLCTF"
    }
    fn description(&self) -> &str {
        "CRI VariSpec liquid-crystal tunable filter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        self.call_transport(|t| t.purge())?;
        self.send_cmd("B0")?;
        self.send_cmd("G0")?;
        loop {
            match self.read_status()? {
                VarispecStatus::Ready => break,
                VarispecStatus::NeedsInitialization => {
                    self.send_cmd("I1")?;
                    self.wait_until_ready()?;
                }
                VarispecStatus::NeedsExercise => {
                    self.send_cmd("E1")?;
                    self.wait_until_ready()?;
                }
            }
        }
        let r = self.query_cmd("V?")?;
        if let Some((rev, min, max)) = Self::parse_version(&r) {
            self.min_nm = min;
            self.max_nm = max;
            self.props
                .entry_mut("FirmwareVersion")
                .map(|e| e.value = PropertyValue::String(rev.clone()));
            self.props
                .entry_mut("Version Number")
                .map(|e| e.value = PropertyValue::String(r.clone()));
            self.props
                .entry_mut("MinWavelength_nm")
                .map(|e| e.value = PropertyValue::Float(min));
            self.props
                .entry_mut("MaxWavelength_nm")
                .map(|e| e.value = PropertyValue::Float(max));
        } else {
            return Err(MmError::SerialInvalidResponse);
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if (name == "Wavelength" || name == "Wavelength_nm") && self.initialized {
            let r = self.query_cmd("W?")?;
            let wl = Self::parse_wavelength(&r).ok_or(MmError::SerialInvalidResponse)?;
            self.get_status()?;
            return Ok(PropertyValue::Float(wl));
        }
        if name == "Version Number" && self.initialized {
            return Ok(PropertyValue::String(self.query_cmd("V?")?));
        }
        if name == "String from VarispecLCTF" {
            return Ok(PropertyValue::String(self.last_response.clone()));
        }
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Wavelength" || name == "Wavelength_nm" {
            let nm = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            if nm < self.min_nm || nm > self.max_nm {
                return Err(MmError::InvalidPropertyValue);
            }
            if self.initialized {
                self.send_cmd(&format!("W {:.3}", nm))?;
                self.changed_time = Some(Instant::now());
            }
            self.wavelength_nm = nm;
            self.props
                .entry_mut("Wavelength")
                .map(|e| e.value = PropertyValue::Float(nm));
            self.props
                .entry_mut("Wavelength_nm")
                .map(|e| e.value = PropertyValue::Float(nm));
            return Ok(());
        }
        if name == "Device Delay (ms.)" {
            let delay = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            if !(0.0..=200.0).contains(&delay) {
                return Err(MmError::InvalidPropertyValue);
            }
            self.delay_ms = delay;
            self.props.set(name, PropertyValue::Float(delay))?;
            return Ok(());
        }
        if name == "String send to VarispecLCTF" {
            let command = val.as_str().to_string();
            let response = self.query_cmd(&command)?;
            self.last_response = response.clone();
            self.get_status()?;
            self.props.set(name, PropertyValue::String(command))?;
            self.props
                .entry_mut("String from VarispecLCTF")
                .map(|e| e.value = PropertyValue::String(response));
            return Ok(());
        }
        if self.initialized && (name == "Port" || name == "Baud Rate") {
            return Err(MmError::InvalidInputParam);
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
        DeviceType::Generic
    }
    fn busy(&self) -> bool {
        self.changed_time
            .map(|changed| changed.elapsed() < Duration::from_secs_f64(self.delay_ms / 1000.0))
            .unwrap_or(false)
    }
}

impl StateDevice for VarispecLCTF {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        self.set_property("Wavelength_nm", PropertyValue::Float(pos as f64))
    }
    fn get_position(&self) -> MmResult<u64> {
        Ok(self.wavelength_nm as u64)
    }
    fn get_number_of_positions(&self) -> u64 {
        (self.max_nm - self.min_nm) as u64 + 1
    }
    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        Ok(format!("{} nm", pos))
    }
    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let nm: f64 = label
            .trim_end_matches(" nm")
            .parse()
            .map_err(|_| MmError::UnknownLabel(label.to_string()))?;
        self.set_property("Wavelength_nm", PropertyValue::Float(nm))
    }
    fn set_position_label(&mut self, _pos: u64, _label: &str) -> MmResult<()> {
        Ok(())
    }
    fn set_gate_open(&mut self, _open: bool) -> MmResult<()> {
        Ok(())
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(true)
    }
}

impl Generic for VarispecLCTF {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("B0\r", "B0")
            .expect("G0\r", "G0")
            .expect_binary(&[b'@', 0x03])
            .expect("V?\r", "V?")
            .any("V 1.2 400.0 720.0 SN12345")
    }

    #[test]
    fn initialize() {
        let mut dev = VarispecLCTF::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert_eq!(dev.wavelength_nm, 546.0);
        assert_eq!(dev.min_nm, 400.0);
        assert_eq!(dev.max_nm, 720.0);
    }

    #[test]
    fn constructor_uses_upstream_cached_wavelength_default() {
        let dev = VarispecLCTF::new();
        assert_eq!(
            dev.get_property("Wavelength").unwrap(),
            PropertyValue::Float(546.0)
        );
        assert_eq!(
            dev.get_property("Wavelength_nm").unwrap(),
            PropertyValue::Float(546.0)
        );
    }

    #[test]
    fn baud_rate_property_matches_upstream_surface() {
        let dev = VarispecLCTF::new();
        assert!(dev.has_property("Baud Rate"));
        assert!(!dev.has_property("Baud"));
    }

    #[test]
    fn set_wavelength() {
        let t = make_transport().expect("W 600.000\r", "W 600.000");
        let mut dev = VarispecLCTF::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Wavelength", PropertyValue::Float(600.0))
            .unwrap();
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
    fn no_transport_error() {
        assert!(VarispecLCTF::new().initialize().is_err());
    }

    #[test]
    fn get_wavelength_performs_live_before_get_read_and_status_check() {
        let t = make_transport()
            .expect("W?\r", "W?")
            .any("W 610.000")
            .expect_binary(&[b'@', 0x03]);
        let mut dev = VarispecLCTF::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert_eq!(
            dev.get_property("Wavelength").unwrap(),
            PropertyValue::Float(610.0)
        );
    }

    #[test]
    fn rejects_bad_echo_on_set_wavelength() {
        let t = make_transport().expect("W 600.000\r", "W 599.000");
        let mut dev = VarispecLCTF::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert_eq!(
            dev.set_property("Wavelength", PropertyValue::Float(600.0)),
            Err(MmError::SerialInvalidResponse)
        );
    }

    #[test]
    fn wavelength_sequence_loads_palette_like_upstream() {
        let t = make_transport()
            .expect("C1\r", "C1")
            .expect("D500.000,0\r", "D500.000,0")
            .expect("D600.000,1\r", "D600.000,1")
            .expect_binary(&[b'@', 0x03]);
        let mut dev = VarispecLCTF::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.add_to_wavelength_sequence(500.0).unwrap();
        dev.add_to_wavelength_sequence(600.0).unwrap();
        dev.load_wavelength_sequence().unwrap();
    }

    #[test]
    fn wavelength_sequence_start_stop_matches_upstream_ttl_commands() {
        let t = make_transport()
            .expect("M0\r", "M0")
            .expect("G1\r", "G1")
            .expect("P0\r", "P0")
            .expect("G0\r", "G0")
            .expect("P0\r", "P0");
        let mut dev = VarispecLCTF::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.start_wavelength_sequence().unwrap();
        dev.stop_wavelength_sequence().unwrap();
    }

    #[test]
    fn initialize_runs_only_needed_status_actions_in_upstream_order() {
        let t = MockTransport::new()
            .expect("B0\r", "B0")
            .expect("G0\r", "G0")
            .expect_binary(&[b'@', 0x02])
            .expect("I1\r", "I1")
            .expect_binary(&[b'!', b'>'])
            .expect_binary(&[b'@', 0x01])
            .expect("E1\r", "E1")
            .expect_binary(&[b'!', b'>'])
            .expect_binary(&[b'@', 0x03])
            .expect("V?\r", "V?")
            .any("V 1.2 400.0 720.0 SN12345");
        let mut dev = VarispecLCTF::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
    }

    #[test]
    fn malformed_busy_response_is_treated_as_not_busy_like_upstream() {
        let t = MockTransport::new()
            .expect("B0\r", "B0")
            .expect("G0\r", "G0")
            .expect_binary(&[b'@', 0x02])
            .expect("I1\r", "I1")
            .expect_binary(&[b'!', b'?'])
            .expect_binary(&[b'@', 0x03])
            .expect("V?\r", "V?")
            .any("V 1.2 400.0 720.0 SN12345");
        let mut dev = VarispecLCTF::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
    }

    #[test]
    fn status_error_propagates_error_query_failure() {
        let t = make_transport()
            .expect("W?\r", "W?")
            .any("W 610.000")
            .expect_binary(&[b'@', 0x23])
            .expect("R?\r", "bad");
        let mut dev = VarispecLCTF::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert_eq!(
            dev.get_property("Wavelength"),
            Err(MmError::SerialInvalidResponse)
        );
    }
}
