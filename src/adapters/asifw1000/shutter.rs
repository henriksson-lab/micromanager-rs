/// ASI FW1000 Shutter.
///
/// Protocol (TX `\r`):
///   `SO <n>\r`   → echo + "1"   open shutter n
///   `SC <n>\r`   → echo + "0"   close shutter n
///   `SQ <n>\r`   → state bitfield in final two chars
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};
use std::time::Instant;

pub struct AsiFw1000Shutter {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    shutter_nr: u8,
    shutter_type: String,
    open: bool,
    delay_ms: f64,
    changed_time: Cell<Instant>,
}

impl AsiFw1000Shutter {
    pub fn new(shutter_nr: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property(
                "ASIShutterNumber",
                PropertyValue::Integer(shutter_nr as i64),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("ASIShutterNumber", &["0", "1"])
            .unwrap();
        props
            .define_property(
                "ASIShutterType",
                PropertyValue::String("Normally Open".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("ASIShutterType", &["Normally Open", "Normally Closed"])
            .unwrap();
        props
            .define_property("Delay", PropertyValue::Float(0.0), false)
            .unwrap();
        props.set_property_limits("Delay", 0.0, f64::MAX).unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            shutter_nr,
            shutter_type: "Normally Open".into(),
            open: false,
            delay_ms: 0.0,
            changed_time: Cell::new(Instant::now()),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = RefCell::new(Some(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.borrow_mut().as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }
}

impl Default for AsiFw1000Shutter {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Device for AsiFw1000Shutter {
    fn name(&self) -> &str {
        "ASIShutter"
    }
    fn description(&self) -> &str {
        "ASIFW1000 Shutter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        let resp = self.cmd(&format!("SQ {}", self.shutter_nr))?;
        self.open = self.parse_shutter_state(&resp)?;
        self.props
            .define_property("Name", PropertyValue::String("ASIShutter".into()), true)
            .unwrap();
        self.props
            .define_property(
                "Description",
                PropertyValue::String("ASIFW1000 Shutter".into()),
                true,
            )
            .unwrap();
        self.props
            .define_property(
                "State",
                PropertyValue::Integer(if self.open { 1 } else { 0 }),
                false,
            )
            .unwrap();
        self.props.set_allowed_values("State", &["0", "1"]).unwrap();
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
        match name {
            "State" => Ok(PropertyValue::Integer(if self.get_open()? { 1 } else { 0 })),
            "ASIShutterNumber" => Ok(PropertyValue::Integer(self.shutter_nr as i64)),
            "ASIShutterType" => Ok(PropertyValue::String(self.shutter_type.clone())),
            "Delay" => Ok(PropertyValue::Float(self.delay_ms)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let open = val.as_i64().ok_or(MmError::InvalidPropertyValue)? == 1;
                self.set_open(open)
            }
            "ASIShutterNumber" => {
                if self.initialized {
                    return Err(MmError::CanNotSetProperty);
                }
                let nr = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if nr != 0 && nr != 1 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.shutter_nr = nr as u8;
                self.props.set(name, PropertyValue::Integer(nr))
            }
            "ASIShutterType" => {
                let shutter_type = val.as_str().to_string();
                self.props
                    .set(name, PropertyValue::String(shutter_type.clone()))?;
                self.shutter_type = shutter_type;
                Ok(())
            }
            "Delay" => {
                let delay = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if delay < 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.delay_ms = delay;
                self.props.set(name, PropertyValue::Float(delay))
            }
            _ => self.props.set(name, val),
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
        DeviceType::Shutter
    }
    fn busy(&self) -> bool {
        self.changed_time.get().elapsed().as_secs_f64() * 1000.0 < self.delay_ms
    }
}

impl Shutter for AsiFw1000Shutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let commanded_open = if self.shutter_type == "Normally Closed" {
            !open
        } else {
            open
        };
        let cmd = if commanded_open {
            format!("SO {}", self.shutter_nr)
        } else {
            format!("SC {}", self.shutter_nr)
        };
        self.changed_time.set(Instant::now());
        self.cmd(&cmd)?;
        self.open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        let resp = self.cmd(&format!("SQ {}", self.shutter_nr))?;
        self.parse_shutter_state(&resp)
    }
    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

impl AsiFw1000Shutter {
    fn parse_shutter_state(&self, resp: &str) -> MmResult<bool> {
        let mut bits = self.parse_response_bits(resp)?;
        if bits < 16 {
            let retry = self.cmd(&format!("SQ {}", self.shutter_nr))?;
            bits = self.parse_response_bits(&retry)?;
        }
        if self.shutter_nr == 1 {
            bits >>= 1;
        }
        let open = bits & 1 == 1;
        Ok(if self.shutter_type == "Normally Closed" {
            !open
        } else {
            open
        })
    }

    fn parse_response_bits(&self, resp: &str) -> MmResult<u8> {
        let trimmed = resp.trim();
        if trimmed.len() < 2 {
            return Err(MmError::LocallyDefined("Shutter was not found".into()));
        }
        trimmed[trimmed.len() - 2..]
            .parse::<u8>()
            .map_err(|_| MmError::SerialInvalidResponse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_closed() {
        let t = MockTransport::new()
            .expect("SQ 0\r", "16")
            .expect("SQ 0\r", "16");
        let mut s = AsiFw1000Shutter::new(0).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn open_close() {
        let t = MockTransport::new()
            .expect("SQ 0\r", "16")
            .expect("SO 0\r", "1")
            .expect("SQ 0\r", "17")
            .expect("SC 0\r", "0")
            .expect("SQ 0\r", "16");
        let mut s = AsiFw1000Shutter::new(0).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() {
        assert!(AsiFw1000Shutter::new(1).initialize().is_err());
    }

    #[test]
    fn normally_closed_inverts_commands() {
        let t = MockTransport::new()
            .expect("SQ 0\r", "16")
            .expect("SC 0\r", "0")
            .expect("SO 0\r", "1");
        let mut s = AsiFw1000Shutter::new(0).with_transport(Box::new(t));
        s.set_property(
            "ASIShutterType",
            PropertyValue::String("Normally Closed".into()),
        )
        .unwrap();
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        s.set_open(false).unwrap();
    }

    #[test]
    fn state_query_retries_response_below_16() {
        let t = MockTransport::new()
            .expect("SQ 1\r", "00")
            .expect("SQ 1\r", "18")
            .expect("SQ 1\r", "18");
        let mut s = AsiFw1000Shutter::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn state_property_queries_live_shutter_state() {
        let t = MockTransport::new()
            .expect("SQ 0\r", "16")
            .expect("SQ 0\r", "17");
        let mut s = AsiFw1000Shutter::new(0).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_property("State").unwrap(), PropertyValue::Integer(1));
    }

    #[test]
    fn busy_uses_delay_timer_after_set_open() {
        let t = MockTransport::new()
            .expect("SQ 0\r", "16")
            .expect("SO 0\r", "1");
        let mut s = AsiFw1000Shutter::new(0).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("Delay", PropertyValue::Float(50.0)).unwrap();
        s.set_open(true).unwrap();
        assert!(s.busy());
        std::thread::sleep(std::time::Duration::from_millis(60));
        assert!(!s.busy());
    }

    #[test]
    fn fire_is_unsupported() {
        let mut s = AsiFw1000Shutter::new(0);
        assert_eq!(s.fire(1.0), Err(MmError::UnsupportedCommand));
    }
}
