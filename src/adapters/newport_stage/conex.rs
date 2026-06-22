/// Newport CONEX-CC single-axis motion controller.
///
/// Protocol (`\r\n` terminator, address prefix "1"):
///   `1VE\r\n`         → "1VE CONEX-CC ..." (firmware)
///   `1TP\r\n`         → "1TP<value>"  (current position, mm)
///   `1PA<+mm.6f>\r\n` → move to absolute position (mm)
///   `1PR<+mm.6f>\r\n` → relative move (mm)
///   `1OR\r\n`         → home (origin search)
///   `1ST\r\n`         → stop
///   `1TS\r\n`         → "1TS00000X" (last 2 hex chars = status code)
///                        0x1C = READY, 0x28 = HOMING, 0x1E = MOVING
///
/// Position unit: millimetres (× 1000 → µm for MicroManager).
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::RefCell;

pub struct NewportConex {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    name: &'static str,
    description: &'static str,
}

impl NewportConex {
    pub fn new() -> Self {
        Self::with_identity("NewportConex", "Newport CONEX-CC single-axis controller")
    }

    pub fn with_identity(name: &'static str, description: &'static str) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_property(
                "FirmwareVersion",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("SearchForHome", PropertyValue::String(String::new()), false)
            .unwrap();
        props
            .set_allowed_values("SearchForHome", &["", "Search for HOME position now"])
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            name,
            description,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        *self.transport.get_mut() = Some(t);
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

    fn cmd(&self, command: &str) -> MmResult<String> {
        let c = format!("{}\r\n", command);
        self.call_transport(|t| {
            let r = t.send_recv(&c)?;
            Ok(r.trim().to_string())
        })
    }

    fn send_cmd(&self, command: &str) -> MmResult<()> {
        let c = format!("{}\r\n", command);
        self.call_transport(|t| t.send(&c))
    }

    fn query_value_um(&self, command: &str, prefix_len: usize) -> MmResult<f64> {
        let resp = self.cmd(command)?;
        Self::parse_prefixed_um(&resp, prefix_len)
    }

    /// Parse "1TP<value>" → µm
    fn parse_position(resp: &str) -> MmResult<f64> {
        Self::parse_prefixed_um(resp, 3)
    }

    fn parse_prefixed_um(resp: &str, prefix_len: usize) -> MmResult<f64> {
        let s = resp.trim();
        if s.len() < prefix_len {
            return Err(MmError::LocallyDefined(format!(
                "Bad CONEX numeric response: {}",
                s
            )));
        }
        let val_str = &s[prefix_len..];
        val_str
            .parse::<f64>()
            .map(|mm| mm * 1000.0)
            .map_err(|_| MmError::LocallyDefined(format!("Cannot parse CONEX value: {}", s)))
    }

    fn moving_from_status(resp: &str) -> bool {
        let s = resp.trim();
        if s.len() < 9 {
            return false;
        }
        matches!(&s[7..9], "1E" | "28")
    }
}

impl Default for NewportConex {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for NewportConex {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        self.description
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        let ver = self.cmd("1VE")?;
        if !ver.contains("CONEX") {
            return Err(MmError::LocallyDefined(format!(
                "Not a CONEX device: {}",
                ver
            )));
        }
        let speed = self.query_value_um("1VA?", 3)?;
        let acceleration = self.query_value_um("1AC?", 4)?;
        let pos = self.query_value_um("1TP", 3)?;
        let negative_limit = self.query_value_um("1SL?", 3)?;
        let positive_limit = self.query_value_um("1SR?", 3)?;
        self.props
            .entry_mut("FirmwareVersion")
            .map(|e| e.value = PropertyValue::String(ver));
        self.props
            .define_property("Position", PropertyValue::Float(pos), false)
            .unwrap();
        self.props
            .define_property("NegativeLimit", PropertyValue::Float(negative_limit), false)
            .unwrap();
        self.props
            .define_property("PositiveLimit", PropertyValue::Float(positive_limit), false)
            .unwrap();
        self.props
            .define_property("Speed", PropertyValue::Float(speed), false)
            .unwrap();
        self.props
            .define_property("Acceleration", PropertyValue::Float(acceleration), false)
            .unwrap();
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Position" => Ok(PropertyValue::Float(self.query_value_um("1TP", 3)?)),
            "NegativeLimit" => Ok(PropertyValue::Float(self.query_value_um("1SL?", 3)?)),
            "PositiveLimit" => Ok(PropertyValue::Float(self.query_value_um("1SR?", 3)?)),
            "Speed" => Ok(PropertyValue::Float(self.query_value_um("1VA?", 3)?)),
            "Acceleration" => Ok(PropertyValue::Float(self.query_value_um("1AC?", 4)?)),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Port" && self.initialized {
            return Err(MmError::InvalidPropertyValue);
        }
        match (name, val) {
            ("SearchForHome", PropertyValue::String(v)) => {
                if v == "Search for HOME position now" {
                    self.send_cmd("1OR")?;
                }
                self.props.set(name, PropertyValue::String(String::new()))
            }
            ("Position", PropertyValue::Float(v)) => {
                self.send_cmd(&format!("1PA{:.6}", v / 1000.0))?;
                self.props.set(name, PropertyValue::Float(v))
            }
            ("NegativeLimit", PropertyValue::Float(v)) => {
                self.send_cmd(&format!("1SL{:.6}", v / 1000.0))?;
                self.props.set(name, PropertyValue::Float(v))
            }
            ("PositiveLimit", PropertyValue::Float(v)) => {
                self.send_cmd(&format!("1SR{:.6}", v / 1000.0))?;
                self.props.set(name, PropertyValue::Float(v))
            }
            ("Speed", PropertyValue::Float(v)) => {
                self.send_cmd(&format!("1VA{:.6}", v / 1000.0))?;
                self.props.set(name, PropertyValue::Float(v))
            }
            ("Acceleration", PropertyValue::Float(v)) => {
                self.send_cmd(&format!("1AC{:.6}", v / 1000.0))?;
                self.props.set(name, PropertyValue::Float(v))
            }
            (name, val) => self.props.set(name, val),
        }
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
        DeviceType::Stage
    }
    fn busy(&self) -> bool {
        self.cmd("1TS")
            .map(|resp| Self::moving_from_status(&resp))
            .unwrap_or(false)
    }
}

impl Stage for NewportConex {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        self.send_cmd(&format!("1PA{:.6}", z / 1000.0))
    }
    fn get_position_um(&self) -> MmResult<f64> {
        self.query_value_um("1TP", 3)
    }
    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        self.send_cmd(&format!("1PR{:.6}", dz / 1000.0))
    }
    fn home(&mut self) -> MmResult<()> {
        self.send_cmd("1OR")
    }
    fn stop(&mut self) -> MmResult<()> {
        self.send_cmd("1ST")
    }
    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((
            self.query_value_um("1SL?", 3)?,
            self.query_value_um("1SR?", 3)?,
        ))
    }
    fn get_focus_direction(&self) -> FocusDirection {
        FocusDirection::Unknown
    }
    fn is_continuous_focus_drive(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("1VE\r\n", "1VE CONEX-CC v1.0.0")
            .expect("1VA?\r\n", "1VA0.250000")
            .expect("1AC?\r\n", "1AC0.500000")
            .expect("1TP\r\n", "1TP+0.012500")
            .expect("1SL?\r\n", "1SL-25.000000")
            .expect("1SR?\r\n", "1SR25.000000")
    }

    #[test]
    fn initialize() {
        let t = make_transport().expect("1TP\r\n", "1TP+0.012500");
        let mut dev = NewportConex::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        // 0.0125 mm * 1000 = 12.5 µm
        assert!((dev.get_position_um().unwrap() - 12.5).abs() < 1e-6);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().expect("1TP\r\n", "1TP+5.000000");
        let mut dev = NewportConex::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_position_um(5000.0).unwrap();
        assert_eq!(dev.get_position_um().unwrap(), 5000.0);
    }

    #[test]
    fn move_relative() {
        let t = make_transport().expect("1TP\r\n", "1TP+0.112500");
        let mut dev = NewportConex::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_relative_position_um(100.0).unwrap();
        assert!((dev.get_position_um().unwrap() - 112.5).abs() < 1e-6);
    }

    #[test]
    fn parse_position_ok() {
        assert!((NewportConex::parse_position("1TP+0.012500").unwrap() - 12.5).abs() < 1e-6);
        assert!((NewportConex::parse_position("1TP-0.005000").unwrap() - (-5.0)).abs() < 1e-6);
    }

    #[test]
    fn wrong_device_error() {
        let t = MockTransport::new().expect("1VE\r\n", "1VE SMCRC-100 v2.0");
        let mut dev = NewportConex::new().with_transport(Box::new(t));
        assert!(dev.initialize().is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(NewportConex::new().initialize().is_err());
    }
}
