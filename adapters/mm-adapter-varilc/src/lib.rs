/// Cambridge Research VariLC liquid crystal controller adapter.
///
/// ASCII serial protocol, `\r` terminated.
///   Set standard mode:    `"B0\r"` → echo
///   Get version/range:    `"V?\r"` → `"0 <minwl> <maxwl> <serial>\r"`
///   Set wavelength:       `"L.<wl>\r"` → echo
///   Set retardance LC-A:  `"L1.<value>\r"` → echo  (LC index starts at 1)
///   Set retardance LC-B:  `"L2.<value>\r"` → echo
///   Get retardance LC-A:  `"L1?\r"` → `"L1 <value>\r"`

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Generic};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const MAX_LCS: usize = 4;

pub struct VariLC {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    num_active_lcs: usize,
    wavelength: f64,
    retardance: [f64; MAX_LCS],
    min_wavelength: f64,
    max_wavelength: f64,
    serial_number: String,
}

impl VariLC {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("NumActiveLCs", PropertyValue::Integer(2), false).unwrap();
        props.define_property("Wavelength", PropertyValue::Float(546.0), false).unwrap();
        props.set_property_limits("Wavelength", 400.0, 800.0).unwrap();
        for i in 0..2usize {
            let name = format!("Retardance LC-{}", (b'A' + i as u8) as char);
            props.define_property(&name, PropertyValue::Float(0.5), false).unwrap();
            props.set_property_limits(&name, 0.0001, 3.0).unwrap();
        }
        Self {
            props,
            transport: None,
            initialized: false,
            num_active_lcs: 2,
            wavelength: 546.0,
            retardance: [0.5; MAX_LCS],
            min_wavelength: 400.0,
            max_wavelength: 800.0,
            serial_number: String::new(),
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

    fn send_recv(&mut self, cmd: &str) -> MmResult<String> {
        self.call_transport(|t| Ok(t.send_recv(cmd)?.trim().to_string()))
    }

    fn set_mode_standard(&mut self) -> MmResult<()> {
        let _resp = self.send_recv("B0\r")?;
        Ok(())
    }

    fn query_version(&mut self) -> MmResult<(f64, f64, String)> {
        let resp = self.send_recv("V?\r")?;
        // Response: "0 400 800 12345"  (revision minwl maxwl serial)
        let parts: Vec<&str> = resp.split_whitespace().collect();
        if parts.len() < 4 {
            return Err(MmError::SerialInvalidResponse);
        }
        let min_wl: f64 = parts[1].parse().map_err(|_| MmError::SerialInvalidResponse)?;
        let max_wl: f64 = parts[2].parse().map_err(|_| MmError::SerialInvalidResponse)?;
        let serial = parts[3].to_string();
        Ok((min_wl, max_wl, serial))
    }

    fn set_wavelength_cmd(&mut self, wl: f64) -> MmResult<()> {
        let cmd = format!("L.{:.1}\r", wl);
        let _resp = self.send_recv(&cmd)?;
        Ok(())
    }

    fn set_retardance_cmd(&mut self, lc_index: usize, value: f64) -> MmResult<()> {
        let cmd = format!("L{}.{:.4}\r", lc_index + 1, value);
        let _resp = self.send_recv(&cmd)?;
        Ok(())
    }
}

impl Default for VariLC {
    fn default() -> Self { Self::new() }
}

impl Device for VariLC {
    fn name(&self) -> &str { "VariLC" }
    fn description(&self) -> &str { "Cambridge Research VariLC liquid crystal controller" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.set_mode_standard()?;
        let (min_wl, max_wl, serial) = self.query_version()?;
        self.min_wavelength = min_wl;
        self.max_wavelength = max_wl;
        self.serial_number = serial;
        self.set_wavelength_cmd(self.wavelength)?;
        for i in 0..self.num_active_lcs {
            self.set_retardance_cmd(i, self.retardance[i])?;
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if name == "Wavelength" {
            return Ok(PropertyValue::Float(self.wavelength));
        }
        if name == "SerialNumber" {
            return Ok(PropertyValue::String(self.serial_number.clone()));
        }
        for i in 0..self.num_active_lcs {
            let pname = format!("Retardance LC-{}", (b'A' + i as u8) as char);
            if name == pname {
                return Ok(PropertyValue::Float(self.retardance[i]));
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
            }
            self.wavelength = wl;
            return Ok(());
        }
        for i in 0..self.num_active_lcs {
            let pname = format!("Retardance LC-{}", (b'A' + i as u8) as char);
            if name == pname {
                let r = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.set_retardance_cmd(i, r)?;
                }
                self.retardance[i] = r;
                return Ok(());
            }
        }
        self.props.set(name, val)
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Generic }
    fn busy(&self) -> bool { false }
}

impl Generic for VariLC {}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_initialized_varilc() -> VariLC {
        let t = MockTransport::new()
            .expect("B0\r", "B0")
            .expect("V?\r", "0 400 800 SN12345")
            .expect("L.546.0\r", "L.546.0")
            .expect("L1.0.5000\r", "L1.0.5000")
            .expect("L2.0.5000\r", "L2.0.5000");
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
            .expect("V?\r", "0 400 800 SN12345")
            .expect("L.546.0\r", "L.546.0")
            .expect("L1.0.5000\r", "L1.0.5000")
            .expect("L2.0.5000\r", "L2.0.5000")
            .expect("L.633.0\r", "L.633.0");
        let mut v = VariLC::new().with_transport(Box::new(t));
        v.initialize().unwrap();
        v.set_property("Wavelength", PropertyValue::Float(633.0)).unwrap();
        assert_eq!(v.wavelength, 633.0);
    }

    #[test]
    fn set_retardance_lc_a() {
        let t = MockTransport::new()
            .expect("B0\r", "B0")
            .expect("V?\r", "0 400 800 SN12345")
            .expect("L.546.0\r", "L.546.0")
            .expect("L1.0.5000\r", "L1.0.5000")
            .expect("L2.0.5000\r", "L2.0.5000")
            .expect("L1.1.2000\r", "L1.1.2000");
        let mut v = VariLC::new().with_transport(Box::new(t));
        v.initialize().unwrap();
        v.set_property("Retardance LC-A", PropertyValue::Float(1.2)).unwrap();
        assert!((v.retardance[0] - 1.2).abs() < 1e-6);
    }

    #[test]
    fn wavelength_out_of_range() {
        let mut v = make_initialized_varilc();
        v.initialize().unwrap();
        assert!(v.set_property("Wavelength", PropertyValue::Float(1200.0)).is_err());
    }

    #[test]
    fn no_transport_error() {
        let mut v = VariLC::new();
        assert!(v.initialize().is_err());
    }
}
