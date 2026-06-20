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

/// Valid ND filter values (optical density / attenuation positions).
const ND_VALUES: &[u8] = &[1, 2, 4, 8, 16, 32];

pub struct NikonIntensiLight {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
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
        props
            .define_property("Version", PropertyValue::String("Undefined".into()), true)
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("ND", PropertyValue::String("1".into()), false)
            .unwrap();
        props
            .set_allowed_values("ND", &["1", "2", "4", "8", "16", "32"])
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            open: false,
            nd: 1,
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
        "NikonIntensiLight"
    }
    fn description(&self) -> &str {
        "Nikon Intensi-Light shutter and ND filter controller"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let resp = self.cmd("rVEN")?;
        if !resp.starts_with("aVEN") {
            return Err(MmError::LocallyDefined(format!(
                "IntensiLight version failed: '{}'",
                resp
            )));
        }
        self.props
            .entry_mut("Version")
            .map(|e| e.value = PropertyValue::String(resp.get(4..).unwrap_or("").to_string()));
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if name == "ND" {
            return Ok(PropertyValue::String(self.nd.to_string()));
        }
        if name == "State" {
            return Ok(PropertyValue::Integer(if self.open { 1 } else { 0 }));
        }
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "ND" {
            let nd = val
                .as_str()
                .parse::<u8>()
                .map_err(|_| MmError::InvalidPropertyValue)?;
            return self.set_nd_filter(nd);
        }
        if name == "State" {
            let open = val.as_i64().ok_or(MmError::InvalidPropertyValue)? != 0;
            return self.set_open(open);
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
        Ok(self.open)
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
        let t = MockTransport::new().any("aVEN1.0").any("aSXC").any("aSXC");
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
}
