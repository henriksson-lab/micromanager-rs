/// Cambridge Research VariLC liquid crystal controller adapter.
///
/// ASCII serial protocol, `\r` terminated.
///   Set standard mode:    `"B0\r"` → echo
///   Get version/range:    `"V?\r"` → `"0 <minwl> <maxwl> <serial>\r"`
///   Set wavelength:       `"W <wl>\r"` → echo
///   Set retardance:       `"L <lc-a> <lc-b> ...\r"` → echo
///   Get retardance list:  `"L?\r"` → current retardance values
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;
use std::time::{Duration, Instant};

const MAX_LCS: usize = 4;
const DEFAULT_NUM_PALETTE_ELEMENTS: usize = 5;

pub struct VariLC {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    num_total_lcs: usize,
    num_active_lcs: usize,
    num_palette_elements: usize,
    wavelength: f64,
    retardance: [f64; MAX_LCS],
    min_wavelength: f64,
    max_wavelength: f64,
    serial_number: String,
    palette_elements: Vec<String>,
    delay_ms: f64,
    changed_time: Option<Instant>,
    last_response: String,
}

impl VariLC {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Baud", PropertyValue::String("9600".into()), false)
            .unwrap();
        props
            .set_allowed_values("Baud", &["115200", "9600"])
            .unwrap();
        props
            .define_property("Baud Rate", PropertyValue::String("9600".into()), false)
            .unwrap();
        props
            .set_allowed_values("Baud Rate", &["115200", "9600"])
            .unwrap();
        props
            .define_property("NumActiveLCs", PropertyValue::Integer(2), false)
            .unwrap();
        props
            .define_property("Number of Active LCs", PropertyValue::Integer(2), false)
            .unwrap();
        props
            .define_property("Total Number of LCs", PropertyValue::Integer(2), false)
            .unwrap();
        props
            .define_property(
                "Total Number of Palette Elements",
                PropertyValue::Integer(DEFAULT_NUM_PALETTE_ELEMENTS as i64),
                false,
            )
            .unwrap();
        props
            .define_property("Active LCs", PropertyValue::Integer(2), true)
            .unwrap();
        props
            .define_property("Total LCs", PropertyValue::Integer(2), true)
            .unwrap();
        props
            .define_property(
                "Version Number",
                PropertyValue::String("Version Number Not Found".into()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Mode; 1=Brief; 0=Standard",
                PropertyValue::String(" 0".into()),
                true,
            )
            .unwrap();
        props
            .define_property("Wavelength", PropertyValue::Float(546.0), false)
            .unwrap();
        props
            .set_property_limits("Wavelength", 400.0, 800.0)
            .unwrap();
        for i in 0..2usize {
            let name = format!("Retardance LC-{}", (b'A' + i as u8) as char);
            props
                .define_property(&name, PropertyValue::Float(0.5), false)
                .unwrap();
            props.set_property_limits(&name, 0.0001, 3.0).unwrap();
            let abs_name = format!("Retardance LC-{} [in nm.]", (b'A' + i as u8) as char);
            props
                .define_property(&abs_name, PropertyValue::Float(273.0), true)
                .unwrap();
        }
        for i in 0..DEFAULT_NUM_PALETTE_ELEMENTS {
            props
                .define_property(
                    Self::palette_property_name(i),
                    PropertyValue::String(String::new()),
                    false,
                )
                .unwrap();
        }
        props
            .define_property("Device Delay (ms.)", PropertyValue::Float(200.0), false)
            .unwrap();
        props
            .set_property_limits("Device Delay (ms.)", 0.0, 200.0)
            .unwrap();
        props
            .define_property(
                "String send to VariLC",
                PropertyValue::String(String::new()),
                false,
            )
            .unwrap();
        props
            .define_property(
                "String from VariLC",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            num_total_lcs: 2,
            num_active_lcs: 2,
            num_palette_elements: DEFAULT_NUM_PALETTE_ELEMENTS,
            wavelength: 546.0,
            retardance: [0.5; MAX_LCS],
            min_wavelength: 400.0,
            max_wavelength: 800.0,
            serial_number: String::new(),
            palette_elements: vec![String::new(); DEFAULT_NUM_PALETTE_ELEMENTS],
            delay_ms: 200.0,
            changed_time: None,
            last_response: String::new(),
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

    fn set_mode_standard(&mut self) -> MmResult<()> {
        self.send_cmd("B0")?;
        Ok(())
    }

    fn query_version(&mut self) -> MmResult<(f64, f64, String)> {
        let resp = self.query_cmd("V?")?;
        // Response: "[V] <revision> <minwl> <maxwl> <serial>"
        let parts: Vec<&str> = resp.split_whitespace().collect();
        let offset = if parts.first() == Some(&"V") { 1 } else { 0 };
        if parts.len() < offset + 4 {
            return Err(MmError::SerialInvalidResponse);
        }
        let min_wl: f64 = parts[offset + 1]
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        let max_wl: f64 = parts[offset + 2]
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        let serial = parts[offset + 3].to_string();
        Ok((min_wl, max_wl, serial))
    }

    fn set_wavelength_cmd(&mut self, wl: f64) -> MmResult<()> {
        self.send_cmd(&format!("W {}", wl))?;
        Ok(())
    }

    fn set_retardance_cmd(&mut self, lc_index: usize, value: f64) -> MmResult<()> {
        let mut values = self.retardance;
        values[lc_index] = value;
        let mut cmd = String::from("L");
        for value in values.iter().take(self.num_total_lcs) {
            cmd.push_str(&format!(" {}", value));
        }
        self.send_cmd(&cmd)?;
        Ok(())
    }

    fn parse_numbers(resp: &str) -> Vec<f64> {
        let mut out = Vec::new();
        for part in resp.split_whitespace() {
            if let Ok(value) = part.parse() {
                out.push(value);
            }
        }
        out
    }

    fn palette_property_name(index: usize) -> String {
        format!("Pal. elem. {:02}; enter 0 to define; 1 to activate", index)
    }

    fn set_integer_aliases(&mut self, names: &[&str], value: i64) {
        for name in names {
            if let Some(entry) = self.props.entry_mut(name) {
                entry.value = PropertyValue::Integer(value);
            }
        }
    }

    fn set_baud_aliases(&mut self, value: String) -> MmResult<()> {
        self.props
            .set("Baud", PropertyValue::String(value.clone()))?;
        self.props.set("Baud Rate", PropertyValue::String(value))?;
        Ok(())
    }

    fn ensure_lc_properties(&mut self, count: usize) -> MmResult<()> {
        for i in 0..count.min(MAX_LCS) {
            let name = format!("Retardance LC-{}", (b'A' + i as u8) as char);
            if !self.props.has_property(&name) {
                self.props
                    .define_property(&name, PropertyValue::Float(0.5), false)?;
                self.props.set_property_limits(&name, 0.0001, 3.0)?;
            }
            let abs_name = format!("Retardance LC-{} [in nm.]", (b'A' + i as u8) as char);
            if !self.props.has_property(&abs_name) {
                self.props
                    .define_property(&abs_name, PropertyValue::Float(273.0), true)?;
            }
        }
        Ok(())
    }

    fn get_palette_element(&self, index: usize) -> MmResult<PropertyValue> {
        if self.initialized {
            let ans = self.query_cmd("D?")?;
            let numbers = Self::parse_numbers(&ans);
            let elem_count = numbers.first().copied().unwrap_or(0.0) as usize;
            if elem_count == 0 {
                return Ok(PropertyValue::String(String::new()));
            }
            for row in 0..elem_count {
                let ans = self.call_transport(|t| Ok(t.receive_line()?.trim().to_string()))?;
                if row == index {
                    let values = Self::parse_numbers(&ans);
                    if values.is_empty() {
                        return Err(MmError::SerialInvalidResponse);
                    }
                    let mut formatted = format!("  {}", values[0]);
                    for value in values.iter().skip(1).take(self.num_active_lcs) {
                        formatted.push_str(&format!("  {}", value));
                    }
                    return Ok(PropertyValue::String(formatted));
                }
            }
        }
        Ok(PropertyValue::String(
            self.palette_elements
                .get(index)
                .cloned()
                .unwrap_or_default(),
        ))
    }
}

impl Default for VariLC {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for VariLC {
    fn name(&self) -> &str {
        "VariLC"
    }
    fn description(&self) -> &str {
        "Cambridge Research VariLC liquid crystal controller"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        self.call_transport(|t| t.purge())?;
        self.set_mode_standard()?;
        let (min_wl, max_wl, serial) = self.query_version()?;
        self.min_wavelength = min_wl;
        self.max_wavelength = max_wl;
        self.serial_number = serial.clone();
        self.set_integer_aliases(
            &["NumActiveLCs", "Number of Active LCs", "Active LCs"],
            self.num_active_lcs as i64,
        );
        self.set_integer_aliases(
            &["Total Number of LCs", "Total LCs"],
            self.num_total_lcs as i64,
        );
        self.set_integer_aliases(
            &["Total Number of Palette Elements"],
            self.num_palette_elements as i64,
        );
        self.props
            .entry_mut("Version Number")
            .map(|e| e.value = PropertyValue::String(serial));
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if name == "Wavelength" && self.initialized {
            let resp = self.query_cmd("W?")?;
            let wl = Self::parse_numbers(&resp)
                .first()
                .copied()
                .ok_or(MmError::SerialInvalidResponse)?;
            return Ok(PropertyValue::Float(wl));
        }
        if name == "SerialNumber" || name == "Version Number" {
            if self.initialized {
                return Ok(PropertyValue::String(self.query_cmd("V?")?));
            }
            return Ok(PropertyValue::String(self.serial_number.clone()));
        }
        if name == "String from VariLC" {
            return Ok(PropertyValue::String(self.last_response.clone()));
        }
        for i in 0..self.num_palette_elements {
            if name == Self::palette_property_name(i) {
                return self.get_palette_element(i);
            }
        }
        for i in 0..self.num_active_lcs {
            let pname = format!("Retardance LC-{}", (b'A' + i as u8) as char);
            if name == pname {
                if self.initialized {
                    let resp = self.query_cmd("L?")?;
                    let values = Self::parse_numbers(&resp);
                    return values
                        .get(i)
                        .copied()
                        .map(PropertyValue::Float)
                        .ok_or(MmError::SerialInvalidResponse);
                }
                return Ok(PropertyValue::Float(self.retardance[i]));
            }
            let abs_name = format!("Retardance LC-{} [in nm.]", (b'A' + i as u8) as char);
            if name == abs_name {
                return Ok(PropertyValue::Float(self.retardance[i] * self.wavelength));
            }
        }
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Wavelength" {
            let wl = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            if wl < self.min_wavelength || wl > self.max_wavelength {
                return Err(MmError::InvalidPropertyValue);
            }
            if self.initialized {
                self.set_wavelength_cmd(wl)?;
                self.changed_time = Some(Instant::now());
            }
            self.wavelength = wl;
            return Ok(());
        }
        if name == "NumActiveLCs" || name == "Number of Active LCs" {
            if self.initialized {
                return Err(MmError::InvalidInputParam);
            }
            let count = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            if count < 1 || count as usize > self.num_total_lcs || count as usize > MAX_LCS {
                return Err(MmError::InvalidPropertyValue);
            }
            self.num_active_lcs = count as usize;
            self.ensure_lc_properties(self.num_active_lcs)?;
            self.set_integer_aliases(
                &["NumActiveLCs", "Number of Active LCs", "Active LCs"],
                count,
            );
            return Ok(());
        }
        if name == "Total Number of LCs" {
            if self.initialized {
                return Err(MmError::InvalidInputParam);
            }
            let count = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            if count < 1 || count as usize > MAX_LCS {
                return Err(MmError::InvalidPropertyValue);
            }
            self.num_total_lcs = count as usize;
            if self.num_active_lcs > self.num_total_lcs {
                self.num_active_lcs = self.num_total_lcs;
            }
            self.ensure_lc_properties(self.num_total_lcs)?;
            self.set_integer_aliases(&["Total Number of LCs", "Total LCs"], count);
            self.set_integer_aliases(
                &["NumActiveLCs", "Number of Active LCs", "Active LCs"],
                self.num_active_lcs as i64,
            );
            return Ok(());
        }
        if name == "Total Number of Palette Elements" {
            if self.initialized {
                return Err(MmError::InvalidInputParam);
            }
            let count = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            if count < 1 || count as usize > DEFAULT_NUM_PALETTE_ELEMENTS {
                return Err(MmError::InvalidPropertyValue);
            }
            self.num_palette_elements = count as usize;
            self.palette_elements
                .resize(self.num_palette_elements, String::new());
            self.set_integer_aliases(&["Total Number of Palette Elements"], count);
            return Ok(());
        }
        if name == "Baud" || name == "Baud Rate" {
            if self.initialized {
                return Err(MmError::InvalidInputParam);
            }
            return self.set_baud_aliases(val.as_str().to_string());
        }
        for i in 0..self.num_palette_elements {
            if name == Self::palette_property_name(i) {
                let action = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    match action {
                        0 => self.send_cmd(&format!("D {}", i))?,
                        1 => self.send_cmd(&format!("P {}", i))?,
                        _ => return Err(MmError::InvalidPropertyValue),
                    }
                    self.changed_time = Some(Instant::now());
                } else if action != 0 && action != 1 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.palette_elements[i] = val.to_string();
                self.props
                    .set(name, PropertyValue::String(val.to_string()))?;
                return Ok(());
            }
        }
        for i in 0..self.num_active_lcs {
            let pname = format!("Retardance LC-{}", (b'A' + i as u8) as char);
            if name == pname {
                let r = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.set_retardance_cmd(i, r)?;
                    self.changed_time = Some(Instant::now());
                }
                self.retardance[i] = r;
                return Ok(());
            }
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
        if name == "String send to VariLC" {
            let command = val.as_str().to_string();
            let response = self.query_cmd(&command)?;
            self.last_response = response.clone();
            self.props.set(name, PropertyValue::String(command))?;
            self.props
                .entry_mut("String from VariLC")
                .map(|e| e.value = PropertyValue::String(response));
            return Ok(());
        }
        if self.initialized && (name == "Port" || name == "Baud" || name == "Baud Rate") {
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

impl Generic for VariLC {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_initialized_varilc() -> VariLC {
        let t = MockTransport::new()
            .expect("B0\r", "B0")
            .expect("V?\r", "V?")
            .any("V 0 400 800 SN12345");
        VariLC::new().with_transport(Box::new(t))
    }

    #[test]
    fn initialize() {
        let mut v = make_initialized_varilc();
        v.initialize().unwrap();
        assert_eq!(v.min_wavelength, 400.0);
        assert_eq!(v.max_wavelength, 800.0);
        assert!(v.serial_number.contains("SN12345"));
    }

    #[test]
    fn set_wavelength() {
        let t = MockTransport::new()
            .expect("B0\r", "B0")
            .expect("V?\r", "V?")
            .any("V 0 400 800 SN12345")
            .expect("W 633\r", "W 633");
        let mut v = VariLC::new().with_transport(Box::new(t));
        v.initialize().unwrap();
        v.set_property("Wavelength", PropertyValue::Float(633.0))
            .unwrap();
        assert_eq!(v.wavelength, 633.0);
    }

    #[test]
    fn set_retardance_lc_a() {
        let t = MockTransport::new()
            .expect("B0\r", "B0")
            .expect("V?\r", "V?")
            .any("V 0 400 800 SN12345")
            .expect("L 1.2 0.5\r", "L 1.2 0.5");
        let mut v = VariLC::new().with_transport(Box::new(t));
        v.initialize().unwrap();
        v.set_property("Retardance LC-A", PropertyValue::Float(1.2))
            .unwrap();
        assert!((v.retardance[0] - 1.2).abs() < 1e-6);
    }

    #[test]
    fn wavelength_out_of_range() {
        let mut v = make_initialized_varilc();
        v.initialize().unwrap();
        assert!(v
            .set_property("Wavelength", PropertyValue::Float(1200.0))
            .is_err());
    }

    #[test]
    fn no_transport_error() {
        let mut v = VariLC::new();
        assert!(v.initialize().is_err());
    }

    #[test]
    fn get_wavelength_performs_live_before_get_read() {
        let t = MockTransport::new()
            .expect("B0\r", "B0")
            .expect("V?\r", "V?")
            .any("V 0 400 800 SN12345")
            .expect("W?\r", "W?")
            .any("W 589");
        let mut v = VariLC::new().with_transport(Box::new(t));
        v.initialize().unwrap();
        assert_eq!(
            v.get_property("Wavelength").unwrap(),
            PropertyValue::Float(589.0)
        );
    }

    #[test]
    fn get_retardance_performs_live_before_get_read() {
        let t = MockTransport::new()
            .expect("B0\r", "B0")
            .expect("V?\r", "V?")
            .any("V 0 400 800 SN12345")
            .expect("L?\r", "L?")
            .any("L 0.75 0.25");
        let mut v = VariLC::new().with_transport(Box::new(t));
        v.initialize().unwrap();
        assert_eq!(
            v.get_property("Retardance LC-A").unwrap(),
            PropertyValue::Float(0.75)
        );
    }

    #[test]
    fn rejects_bad_echo_on_wavelength_set() {
        let t = MockTransport::new()
            .expect("B0\r", "B0")
            .expect("V?\r", "V?")
            .any("V 0 400 800 SN12345")
            .expect("W 633\r", "W 632");
        let mut v = VariLC::new().with_transport(Box::new(t));
        v.initialize().unwrap();
        assert_eq!(
            v.set_property("Wavelength", PropertyValue::Float(633.0)),
            Err(MmError::SerialInvalidResponse)
        );
    }

    #[test]
    fn upstream_preinit_aliases_update_active_total_lc_and_baud() {
        let mut v = VariLC::new();
        v.set_property("Total Number of LCs", PropertyValue::Integer(4))
            .unwrap();
        v.set_property("Number of Active LCs", PropertyValue::Integer(3))
            .unwrap();
        v.set_property("Baud Rate", PropertyValue::String("115200".into()))
            .unwrap();
        assert_eq!(
            v.get_property("Total LCs").unwrap(),
            PropertyValue::Integer(4)
        );
        assert_eq!(
            v.get_property("Active LCs").unwrap(),
            PropertyValue::Integer(3)
        );
        assert_eq!(
            v.get_property("Baud").unwrap(),
            PropertyValue::String("115200".into())
        );
    }

    #[test]
    fn set_retardance_sends_total_lc_width_like_upstream() {
        let t = MockTransport::new()
            .expect("B0\r", "B0")
            .expect("V?\r", "V?")
            .any("V 0 400 800 SN12345")
            .expect("L 0.5 0.5 1.25 0.5\r", "L 0.5 0.5 1.25 0.5");
        let mut v = VariLC::new().with_transport(Box::new(t));
        v.set_property("Total Number of LCs", PropertyValue::Integer(4))
            .unwrap();
        v.set_property("Number of Active LCs", PropertyValue::Integer(3))
            .unwrap();
        v.initialize().unwrap();
        v.set_property("Retardance LC-C", PropertyValue::Float(1.25))
            .unwrap();
        assert!((v.retardance[2] - 1.25).abs() < 1e-6);
    }

    #[test]
    fn palette_properties_define_activate_and_read_rows() {
        let name = VariLC::palette_property_name(1);
        let t = MockTransport::new()
            .expect("B0\r", "B0")
            .expect("V?\r", "V?")
            .any("V 0 400 800 SN12345")
            .expect("D 1\r", "D 1")
            .expect("P 1\r", "P 1")
            .expect("D?\r", "D?")
            .any("D 2")
            .any("546 0.5 0.5")
            .any("589 0.75 0.25");
        let mut v = VariLC::new().with_transport(Box::new(t));
        v.initialize().unwrap();
        v.set_property(&name, PropertyValue::Integer(0)).unwrap();
        v.set_property(&name, PropertyValue::Integer(1)).unwrap();
        assert_eq!(
            v.get_property(&name).unwrap(),
            PropertyValue::String("  589  0.75  0.25".into())
        );
    }
}
