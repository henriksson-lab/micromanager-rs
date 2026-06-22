/// Nikon Intensi-Light shutter + ND filter adapter.
///
/// Protocol (TX `\r`, RX `\r\n`):
///   `rVEN\r`       → `aVEN{version}\r\n`  (version query)
///   `cSXC2\r`      → `aSXC\r\n`           (close shutter)
///   `cSXC1\r`      → `aSXC\r\n`           (open shutter)
///   `cNDM{idx}\r`  → `aNDM\r\n`           (set ND filter; idx in 1..=6)
///
/// Success prefix `o{CMD}`, error prefix `n{CMD}{code}`.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

/// Valid ND filter values (optical density / attenuation positions).
const ND_VALUES: &[u8] = &[1, 2, 4, 8, 16, 32];

pub struct NikonIntensiLight {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    open: bool,
    nd: u8,
}

impl NikonIntensiLight {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            open: false,
            nd: 1,
        }
    }

    fn define_runtime_properties(&mut self, version: String) -> MmResult<()> {
        if !self.props.has_property("Version") {
            self.props
                .define_property("Version", PropertyValue::String(version), true)?;
        } else {
            self.props
                .entry_mut("Version")
                .map(|e| e.value = PropertyValue::String(version));
        }
        if !self.props.has_property("State") {
            self.props
                .define_property("State", PropertyValue::Integer(0), false)?;
            self.props.set_allowed_values("State", &["0", "1"])?;
        }
        if !self.props.has_property("ND") {
            self.props
                .define_property("ND", PropertyValue::String("1".into()), false)?;
            self.props
                .set_allowed_values("ND", &["1", "2", "4", "8", "16", "32"])?;
        }
        Ok(())
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
        let c = format!("{}\r", command);
        self.call_transport(|t| Ok(t.send_recv(&c)?.trim().to_string()))
    }

    fn check_response(resp: &str, cmd: &str) -> MmResult<()> {
        if resp.len() >= 4 && resp[1..].starts_with(cmd) && !resp.starts_with('n') {
            Ok(())
        } else if resp.starts_with('n') {
            Err(MmError::LocallyDefined(format!(
                "IntensiLight error: '{}'",
                resp
            )))
        } else {
            Err(MmError::LocallyDefined(format!(
                "IntensiLight unexpected response: '{}'",
                resp
            )))
        }
    }

    fn read_open(&self) -> MmResult<bool> {
        let resp = self.cmd("rSXR")?;
        if resp.starts_with('n') {
            return Err(MmError::LocallyDefined(format!(
                "IntensiLight error: '{}'",
                resp
            )));
        }
        if resp.len() >= 5 && resp[1..].starts_with("SXR") {
            return Ok(resp.as_bytes()[4] == b'1');
        }
        Err(MmError::LocallyDefined(format!(
            "IntensiLight unexpected response: '{}'",
            resp
        )))
    }

    fn read_nd_filter(&self) -> MmResult<u8> {
        let resp = self.cmd("rNAR")?;
        if resp.starts_with('n') {
            return Err(MmError::LocallyDefined(format!(
                "IntensiLight error: '{}'",
                resp
            )));
        }
        if resp.len() >= 5 && resp[1..].starts_with("NAR") {
            let idx = resp[4..]
                .parse::<usize>()
                .map_err(|_| MmError::SerialInvalidResponse)?;
            return ND_VALUES
                .get(idx.saturating_sub(1))
                .copied()
                .ok_or(MmError::SerialInvalidResponse);
        }
        Err(MmError::LocallyDefined(format!(
            "IntensiLight unexpected response: '{}'",
            resp
        )))
    }

    pub fn set_nd_filter(&mut self, nd: u8) -> MmResult<()> {
        if !ND_VALUES.contains(&nd) {
            return Err(MmError::LocallyDefined(format!("Invalid ND value: {}", nd)));
        }
        let nd_index = ND_VALUES
            .iter()
            .position(|&v| v == nd)
            .map(|i| i + 1)
            .ok_or_else(|| MmError::LocallyDefined(format!("Invalid ND value: {}", nd)))?;
        let resp = self.cmd(&format!("cNDM{}", nd_index))?;
        Self::check_response(&resp, "NDM")?;
        self.nd = nd;
        self.props
            .entry_mut("ND")
            .map(|e| e.value = PropertyValue::String(nd.to_string()));
        Ok(())
    }

    pub fn get_nd_filter(&self) -> u8 {
        self.nd
    }
}

impl Default for NikonIntensiLight {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for NikonIntensiLight {
    fn name(&self) -> &str {
        "IntensiLightShutter"
    }
    fn description(&self) -> &str {
        "Nikon IntensiLight Shutter adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        let resp = self.cmd("rVEN")?;
        if !resp.starts_with("aVEN") {
            return Err(MmError::LocallyDefined(format!(
                "IntensiLight version failed: '{}'",
                resp
            )));
        }
        self.define_runtime_properties(resp.get(4..).unwrap_or("").to_string())?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if name == "ND" {
            if !self.props.has_property(name) {
                return Err(MmError::UnknownLabel(name.to_string()));
            }
            return Ok(PropertyValue::String(self.read_nd_filter()?.to_string()));
        }
        if name == "State" {
            if !self.props.has_property(name) {
                return Err(MmError::UnknownLabel(name.to_string()));
            }
            return Ok(PropertyValue::Integer(if self.read_open()? {
                1
            } else {
                0
            }));
        }
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Port" && self.initialized {
            return Err(MmError::InvalidPropertyValue);
        }
        if name == "ND" {
            if !self.props.has_property(name) {
                return Err(MmError::UnknownLabel(name.to_string()));
            }
            let nd = val
                .as_str()
                .parse::<u8>()
                .map_err(|_| MmError::InvalidPropertyValue)?;
            return self.set_nd_filter(nd);
        }
        if name == "State" {
            if !self.props.has_property(name) {
                return Err(MmError::UnknownLabel(name.to_string()));
            }
            let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            return match state {
                0 => self.set_open(false),
                1 => self.set_open(true),
                _ => Err(MmError::InvalidPropertyValue),
            };
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

impl Shutter for NikonIntensiLight {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let resp = self.cmd(if open { "cSXC1" } else { "cSXC2" })?;
        Self::check_response(&resp, "SXC")?;
        self.open = open;
        self.props
            .entry_mut("State")
            .map(|e| e.value = PropertyValue::Integer(if open { 1 } else { 0 }));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        self.read_open()
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::NotSupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_and_open_close() {
        let t = MockTransport::new()
            .any("aVEN1.0")
            .expect("cSXC1\r", "aSXC")
            .expect("rSXR\r", "aSXR1")
            .expect("cSXC2\r", "aSXC")
            .expect("rSXR\r", "aSXR0");
        let mut s = NikonIntensiLight::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn nd_filter_set() {
        let t = MockTransport::new().any("aVEN1.0").any("aNDM");
        let mut s = NikonIntensiLight::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_nd_filter(8).unwrap();
        assert_eq!(s.get_nd_filter(), 8);
    }

    #[test]
    fn state_and_nd_properties_live_query() {
        let t = MockTransport::new()
            .expect("rVEN\r", "aVEN1.0")
            .expect("rSXR\r", "aSXR1")
            .expect("rNAR\r", "aNAR4");
        let mut s = NikonIntensiLight::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_property("State").unwrap(), PropertyValue::Integer(1));
        assert_eq!(
            s.get_property("ND").unwrap(),
            PropertyValue::String("8".into())
        );
    }

    #[test]
    fn invalid_nd_rejected() {
        let t = MockTransport::new().any("aVEN1.0");
        let mut s = NikonIntensiLight::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_nd_filter(7).is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(NikonIntensiLight::new().initialize().is_err());
    }

    #[test]
    fn upstream_name_description_and_property_lifecycle() {
        let t = MockTransport::new().expect("rVEN\r", "aVEN1.0");
        let mut s = NikonIntensiLight::new().with_transport(Box::new(t));
        assert_eq!(s.name(), "IntensiLightShutter");
        assert_eq!(s.description(), "Nikon IntensiLight Shutter adapter");
        assert_eq!(s.property_names(), vec!["Port"]);
        assert!(!s.has_property("Version"));
        assert!(!s.has_property("State"));
        assert!(!s.has_property("ND"));
        assert_eq!(
            s.get_property("State").unwrap_err(),
            MmError::UnknownLabel("State".into())
        );

        s.initialize().unwrap();
        assert!(s.has_property("Version"));
        assert!(s.has_property("State"));
        assert!(s.has_property("ND"));
        assert_eq!(
            s.get_property("Version").unwrap(),
            PropertyValue::String("1.0".into())
        );
    }

    #[test]
    fn initialized_port_change_is_rejected() {
        let t = MockTransport::new().any("aVEN1.0");
        let mut s = NikonIntensiLight::new().with_transport(Box::new(t));
        s.set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        s.initialize().unwrap();

        assert!(s
            .set_property("Port", PropertyValue::String("COM2".into()))
            .is_err());
        assert_eq!(
            s.get_property("Port").unwrap(),
            PropertyValue::String("COM1".into())
        );
    }

    #[test]
    fn invalid_state_property_is_rejected() {
        let t = MockTransport::new().expect("rVEN\r", "aVEN1.0");
        let mut s = NikonIntensiLight::new().with_transport(Box::new(t));
        s.initialize().unwrap();

        assert_eq!(
            s.set_property("State", PropertyValue::Integer(2))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }
}
