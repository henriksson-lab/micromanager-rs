/// Newport SMC100 single-axis motion controller.
///
/// Protocol (`\r\n` terminator, address prefix "1"):
///   `1ID\r\n`         → "1IDSMC100CC..."  (identity)
///   `1TP\r\n`         → "1TP<value>"  (current position, mm)
///   `1PA<+mm.6f>\r\n` → absolute move (mm)
///   `1PR<+mm.6f>\r\n` → relative move (mm)
///   `1OR\r\n`         → home search
///   `1ST\r\n`         → stop
///   `1ZT\r\n`         → "1ZT<min> <max>" (travel limits, mm)
///   `1TS\r\n`         → "1TS000000XX" (last 2 hex chars = state code)
///                        0x32/0x33/0x34 = READY
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

pub struct NewportSmc {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    pos_um: f64,
    min_um: f64,
    max_um: f64,
}

impl NewportSmc {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Identity", PropertyValue::String(String::new()), true).unwrap();
        Self { props, transport: None, initialized: false, pos_um: 0.0, min_um: -25_000.0, max_um: 25_000.0 }
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
        let c = format!("{}\r\n", command);
        self.call_transport(|t| { let r = t.send_recv(&c)?; Ok(r.trim().to_string()) })
    }

    /// Parse "1TP<value>" → µm
    fn parse_position(resp: &str) -> MmResult<f64> {
        let s = resp.trim();
        let val_str = if s.starts_with("1TP") { &s[3..] } else { s };
        val_str.parse::<f64>()
            .map(|mm| mm * 1000.0)
            .map_err(|_| MmError::LocallyDefined(format!("Cannot parse position: {}", s)))
    }

    /// Parse "1ZT<min> <max>" → (min_um, max_um)
    fn parse_limits(resp: &str) -> Option<(f64, f64)> {
        let s = resp.trim();
        let s = if s.starts_with("1ZT") { &s[3..] } else { s };
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() >= 2 {
            let min = parts[0].parse::<f64>().ok()? * 1000.0;
            let max = parts[1].parse::<f64>().ok()? * 1000.0;
            Some((min, max))
        } else {
            None
        }
    }
}

impl Default for NewportSmc { fn default() -> Self { Self::new() } }

impl Device for NewportSmc {
    fn name(&self) -> &str { "NewportSMC100" }
    fn description(&self) -> &str { "Newport SMC100 single-axis controller" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let id = self.cmd("1ID")?;
        self.props.entry_mut("Identity").map(|e| e.value = PropertyValue::String(id));
        if let Ok(r) = self.cmd("1ZT") {
            if let Some((min, max)) = Self::parse_limits(&r) {
                self.min_um = min;
                self.max_um = max;
            }
        }
        let resp = self.cmd("1TP")?;
        self.pos_um = Self::parse_position(&resp)?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> { self.props.set(name, val) }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Stage }
    fn busy(&self) -> bool { false }
}

impl Stage for NewportSmc {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        self.cmd(&format!("1PA{:+.6}", z / 1000.0))?;
        self.pos_um = z;
        Ok(())
    }
    fn get_position_um(&self) -> MmResult<f64> { Ok(self.pos_um) }
    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        self.cmd(&format!("1PR{:+.6}", dz / 1000.0))?;
        self.pos_um += dz;
        Ok(())
    }
    fn home(&mut self) -> MmResult<()> {
        self.cmd("1OR")?;
        self.pos_um = 0.0;
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> { let _ = self.cmd("1ST"); Ok(()) }
    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((self.min_um, self.max_um)) }
    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any("1IDSMC100CC v2.0")
            .any("1ZT-25.000000 25.000000")
            .any("1TP+0.010000")
    }

    #[test]
    fn initialize() {
        let mut dev = NewportSmc::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        // 0.01 mm * 1000 = 10 µm
        assert!((dev.get_position_um().unwrap() - 10.0).abs() < 1e-6);
        assert!((dev.min_um - (-25_000.0)).abs() < 1e-3);
        assert!((dev.max_um - 25_000.0).abs() < 1e-3);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any("OK");
        let mut dev = NewportSmc::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_position_um(3000.0).unwrap();
        assert_eq!(dev.get_position_um().unwrap(), 3000.0);
    }

    #[test]
    fn parse_limits_ok() {
        let (min, max) = NewportSmc::parse_limits("1ZT-25.000000 25.000000").unwrap();
        assert!((min - (-25_000.0)).abs() < 1e-3);
        assert!((max - 25_000.0).abs() < 1e-3);
    }

    #[test]
    fn no_transport_error() { assert!(NewportSmc::new().initialize().is_err()); }
}
