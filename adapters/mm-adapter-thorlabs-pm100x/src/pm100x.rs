/// Thorlabs PM100x power meter adapter.
///
/// SCPI-like serial protocol (USB-VCP, typically 115200 baud):
///   `*IDN?`            → identification string
///   `MEAS:POW?`        → measure power (returns float in current units)
///   `SENS:POW:UNIT?`   → query power unit: "W" or "DBM"
///   `SENS:CORR:WAV <nm>` → set wavelength
///   `SENS:CORR:WAV?`   → get wavelength
///   `SENS:POW:RANG:AUTO ON|OFF` → enable/disable auto-range
///   `SENS:POW:RANG <W>`        → set manual power range
///   `SENS:POW:RANG?`           → get current range
///
/// Implements `Generic` (no extra trait methods beyond Device), exposing readings
/// via properties so mm-core can poll them.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Generic};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct ThorlabsPM100x {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    /// Cached power in Watts (raw)
    power_w: f64,
    /// Cached wavelength in nm
    wavelength_nm: f64,
    /// Auto-range enabled
    auto_range: bool,
    /// Manual power range in Watts
    power_range_w: f64,
}

impl ThorlabsPM100x {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Power_W", PropertyValue::Float(0.0), true)
            .unwrap();
        props
            .define_property("Wavelength_nm", PropertyValue::Float(488.0), false)
            .unwrap();
        props
            .define_property("AutoRange", PropertyValue::String("On".into()), false)
            .unwrap();
        props
            .define_property("PowerRange_W", PropertyValue::Float(0.001), false)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            power_w: 0.0,
            wavelength_nm: 488.0,
            auto_range: true,
            power_range_w: 0.001,
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
        let cmd = command.to_string();
        self.call_transport(|t| Ok(t.send_recv(&cmd)?.trim().to_string()))
    }

    /// Measure power and cache the result.
    pub fn measure_power(&mut self) -> MmResult<f64> {
        let resp = self.cmd("MEAS:POW?")?;
        let val: f64 = resp
            .parse()
            .map_err(|_| MmError::LocallyDefined(format!("Bad power: {}", resp)))?;
        self.power_w = val;
        Ok(val)
    }

    /// Set wavelength on the device.
    pub fn set_wavelength(&mut self, nm: f64) -> MmResult<()> {
        let cmd = format!("SENS:CORR:WAV {:.2}", nm);
        let _ = self.cmd(&cmd)?;
        self.wavelength_nm = nm;
        Ok(())
    }

    /// Set auto-range mode.
    pub fn set_auto_range(&mut self, on: bool) -> MmResult<()> {
        let setting = if on { "ON" } else { "OFF" };
        let cmd = format!("SENS:POW:RANG:AUTO {}", setting);
        let _ = self.cmd(&cmd)?;
        self.auto_range = on;
        Ok(())
    }

    /// Set manual power range.
    pub fn set_power_range(&mut self, range_w: f64) -> MmResult<()> {
        let cmd = format!("SENS:POW:RANG {:.6E}", range_w);
        let _ = self.cmd(&cmd)?;
        self.power_range_w = range_w;
        Ok(())
    }
}

impl Default for ThorlabsPM100x {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ThorlabsPM100x {
    fn name(&self) -> &str {
        "ThorlabsPM100x"
    }

    fn description(&self) -> &str {
        "Thorlabs PM100x power meter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Identify the device
        let _idn = self.cmd("*IDN?")?;
        // Read current wavelength
        let wl_resp = self.cmd("SENS:CORR:WAV?")?;
        if let Ok(wl) = wl_resp.parse::<f64>() {
            self.wavelength_nm = wl;
        }
        // Read auto-range state
        let ar_resp = self.cmd("SENS:POW:RANG:AUTO?")?;
        self.auto_range = ar_resp.trim().eq_ignore_ascii_case("on")
            || ar_resp.trim() == "1";
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Power_W" => Ok(PropertyValue::Float(self.power_w)),
            "Wavelength_nm" => Ok(PropertyValue::Float(self.wavelength_nm)),
            "AutoRange" => Ok(PropertyValue::String(
                if self.auto_range { "On" } else { "Off" }.into(),
            )),
            "PowerRange_W" => Ok(PropertyValue::Float(self.power_range_w)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Wavelength_nm" => {
                let nm = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_wavelength(nm)
            }
            "AutoRange" => {
                let s = val.as_str().to_string();
                let on = s.eq_ignore_ascii_case("on") || s == "1";
                self.set_auto_range(on)
            }
            "PowerRange_W" => {
                let r = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_power_range(r)
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
        match name {
            "Power_W" => true,
            _ => self.props.entry(name).map(|e| e.read_only).unwrap_or(false),
        }
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Generic
    }

    fn busy(&self) -> bool {
        false
    }
}

impl Generic for ThorlabsPM100x {}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn initialized_device() -> ThorlabsPM100x {
        let t = MockTransport::new()
            .expect("*IDN?", "Thorlabs,PM100USB,M00123456,1.0")
            .expect("SENS:CORR:WAV?", "488.00")
            .expect("SENS:POW:RANG:AUTO?", "ON");
        ThorlabsPM100x::new().with_transport(Box::new(t))
    }

    #[test]
    fn initialize_succeeds() {
        let mut d = initialized_device();
        d.initialize().unwrap();
        assert!(d.initialized);
        assert!((d.wavelength_nm - 488.0).abs() < 0.01);
        assert!(d.auto_range);
    }

    #[test]
    fn no_transport_error() {
        assert!(ThorlabsPM100x::new().initialize().is_err());
    }

    #[test]
    fn measure_power_parses_float() {
        let t = MockTransport::new()
            .expect("*IDN?", "Thorlabs,PM100USB,M00123,1.0")
            .expect("SENS:CORR:WAV?", "532.00")
            .expect("SENS:POW:RANG:AUTO?", "ON")
            .expect("MEAS:POW?", "1.23e-3");
        let mut d = ThorlabsPM100x::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        let p = d.measure_power().unwrap();
        assert!((p - 1.23e-3).abs() < 1e-10);
    }

    #[test]
    fn set_wavelength_sends_command() {
        let t = MockTransport::new()
            .expect("*IDN?", "Thorlabs,PM100USB,M00123,1.0")
            .expect("SENS:CORR:WAV?", "488.00")
            .expect("SENS:POW:RANG:AUTO?", "ON")
            .expect("SENS:CORR:WAV 532.00", "");
        let mut d = ThorlabsPM100x::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_wavelength(532.0).unwrap();
        assert!((d.wavelength_nm - 532.0).abs() < 0.01);
    }

    #[test]
    fn device_type_is_generic() {
        assert_eq!(ThorlabsPM100x::new().device_type(), DeviceType::Generic);
    }

    #[test]
    fn power_property_is_read_only() {
        assert!(ThorlabsPM100x::new().is_property_read_only("Power_W"));
    }
}
