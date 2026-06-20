/// Yokogawa CSU-W1 spinning disk confocal — shutter.
///
/// Protocol (TX `\r`, RX `\r`):
///   `SHO\r`      → `A`       open main shutter
///   `SHC\r`      → `A`       close main shutter
///   `SH, ?\r`    → `OPEN\rA` or `CLOSED\rA`  query state
///   `SH2O\r`     → `A`       open NIR shutter
///   `SH2C\r`     → `A`       close NIR shutter
///   `SH2, ?\r`   → `OPEN\rA` or `CLOSED\rA`
///
/// Responses: `A` = acknowledged, `N` = negative/error.
/// Query responses: value line, then `A` line.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct CsuShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    nir: bool,
    open: bool,
}

impl CsuShutter {
    pub fn new(nir: bool) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("State", PropertyValue::String("Closed".into()), false)
            .unwrap();
        props
            .set_allowed_values("State", &["Closed", "Open"])
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            nir,
            open: false,
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
        let full = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    fn prefix(&self) -> &str {
        if self.nir {
            "SH2"
        } else {
            "SH"
        }
    }
}

impl Default for CsuShutter {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Device for CsuShutter {
    fn name(&self) -> &str {
        if self.nir {
            "CSUW1-NIR Shutter"
        } else {
            "CSUW1-Shutter"
        }
    }
    fn description(&self) -> &str {
        if self.nir {
            "NIR Shutter"
        } else {
            "CSUW1 Shutter"
        }
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let q = format!("{}, ?", self.prefix());
        let resp = self.cmd(&q)?;
        self.open = resp.contains("OPEN");
        self.props.set(
            "State",
            PropertyValue::String(if self.open { "Open" } else { "Closed" }.into()),
        )?;
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
            "State" => Ok(PropertyValue::String(
                if self.open { "Open" } else { "Closed" }.into(),
            )),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let state = val.as_str();
                if state == "Open" {
                    self.set_open(true)
                } else if state == "Closed" {
                    self.set_open(false)
                } else {
                    Err(MmError::InvalidPropertyValue)
                }
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
        false
    }
}

impl Shutter for CsuShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = format!("{}{}", self.prefix(), if open { "O" } else { "C" });
        let resp = self.cmd(&cmd)?;
        if resp.contains('N') {
            return Err(MmError::LocallyDefined(format!(
                "CSU shutter NAK: {}",
                resp
            )));
        }
        self.open = open;
        self.props.set(
            "State",
            PropertyValue::String(if self.open { "Open" } else { "Closed" }.into()),
        )?;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_closed() {
        let t = MockTransport::new().expect("SH, ?\r", "CLOSED\rA");
        let mut s = CsuShutter::new(false).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn open_close() {
        let t = MockTransport::new()
            .expect("SH, ?\r", "CLOSED\rA")
            .expect("SHO\r", "A")
            .expect("SHC\r", "A");
        let mut s = CsuShutter::new(false).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn nir_shutter() {
        let t = MockTransport::new()
            .expect("SH2, ?\r", "OPEN\rA")
            .expect("SH2C\r", "A");
        let mut s = CsuShutter::new(true).with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() {
        assert!(CsuShutter::new(false).initialize().is_err());
    }

    #[test]
    fn shutdown_does_not_close_shutter_like_upstream() {
        let t = MockTransport::new().expect("SH, ?\r", "OPEN\rA");
        let mut s = CsuShutter::new(false).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.shutdown().unwrap();
    }

    #[test]
    fn fire_is_unsupported_like_upstream() {
        let mut s = CsuShutter::new(false);
        assert_eq!(s.fire(1.0).unwrap_err(), MmError::UnsupportedCommand);
    }
}
