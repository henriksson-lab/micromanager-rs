/// Nikon Intensi-Light shutter + ND filter adapter.
///
/// Protocol (TX `\r`, RX `\r\n`):
///   `rVER\r`       → `oVER{version}\r\n`  (version query)
///   `cTS0\r`       → `oTS\r\n`            (close shutter)
///   `cTS1\r`       → `oTS\r\n`            (open shutter)
///   `cND{val}\r`   → `oND\r\n`            (set ND filter; val in {1,2,4,8,16,32})
///
/// Success prefix `o{CMD}`, error prefix `n{CMD}{code}`.
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Shutter};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

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
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("NDFilter", PropertyValue::Integer(1), false).unwrap();
        Self { props, transport: None, initialized: false, open: false, nd: 1 }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where F: FnOnce(&mut dyn Transport) -> MmResult<R> {
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
        let expected_ok = format!("o{}", cmd);
        if resp.starts_with(&expected_ok) {
            Ok(())
        } else if resp.starts_with('n') {
            Err(MmError::LocallyDefined(format!("IntensiLight error: '{}'", resp)))
        } else {
            Err(MmError::LocallyDefined(format!("IntensiLight unexpected response: '{}'", resp)))
        }
    }

    pub fn set_nd_filter(&mut self, nd: u8) -> MmResult<()> {
        if !ND_VALUES.contains(&nd) {
            return Err(MmError::LocallyDefined(format!("Invalid ND value: {}", nd)));
        }
        let resp = self.cmd(&format!("cND{}", nd))?;
        Self::check_response(&resp, "ND")?;
        self.nd = nd;
        Ok(())
    }

    pub fn get_nd_filter(&self) -> u8 { self.nd }
}

impl Default for NikonIntensiLight { fn default() -> Self { Self::new() } }

impl Device for NikonIntensiLight {
    fn name(&self) -> &str { "NikonIntensiLight" }
    fn description(&self) -> &str { "Nikon Intensi-Light shutter and ND filter controller" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let resp = self.cmd("rVER")?;
        if !resp.starts_with("oVER") {
            return Err(MmError::LocallyDefined(format!("IntensiLight version failed: '{}'", resp)));
        }
        // Close shutter on init
        let resp = self.cmd("cTS0")?;
        Self::check_response(&resp, "TS")?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if name == "NDFilter" { return Ok(PropertyValue::Integer(self.nd as i64)); }
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "NDFilter" {
            if let PropertyValue::Integer(v) = val {
                return self.set_nd_filter(v as u8);
            }
        }
        self.props.set(name, val)
    }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Shutter }
    fn busy(&self) -> bool { false }
}

impl Shutter for NikonIntensiLight {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let resp = self.cmd(if open { "cTS1" } else { "cTS0" })?;
        Self::check_response(&resp, "TS")?;
        self.open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.open) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::NotSupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    #[test]
    fn initialize_and_open_close() {
        let t = MockTransport::new().any("oVER1.0").any("oTS").any("oTS").any("oTS");
        let mut s = NikonIntensiLight::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn nd_filter_set() {
        let t = MockTransport::new().any("oVER1.0").any("oTS").any("oND");
        let mut s = NikonIntensiLight::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_nd_filter(8).unwrap();
        assert_eq!(s.get_nd_filter(), 8);
    }

    #[test]
    fn invalid_nd_rejected() {
        let t = MockTransport::new().any("oVER1.0").any("oTS");
        let mut s = NikonIntensiLight::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_nd_filter(7).is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(NikonIntensiLight::new().initialize().is_err());
    }
}
