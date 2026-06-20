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
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};

pub struct ThorlabsPM100x {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    /// Cached power in Watts (raw)
    power_w: Cell<f64>,
    /// Cached wavelength in nm
    wavelength_nm: Cell<f64>,
    /// Auto-range enabled
    auto_range: Cell<bool>,
    /// Manual power range in Watts
    power_range_w: Cell<f64>,
}

impl ThorlabsPM100x {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("PowerMeter", PropertyValue::String(String::new()))
            .unwrap();
        props.set_allowed_values("PowerMeter", &[""]).unwrap();
        props
            .define_property(
                "Sensor Serial Number",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Calibration Date",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("Author", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Power", PropertyValue::String("0W".into()), true)
            .unwrap();
        props
            .define_property("RawPower", PropertyValue::String("0.0".into()), true)
            .unwrap();
        props
            .define_property("RawUnit", PropertyValue::String("W".into()), true)
            .unwrap();
        props
            .define_property("Wavelength", PropertyValue::Float(488.0), false)
            .unwrap();
        props
            .define_property("AutoRange", PropertyValue::String("On".into()), false)
            .unwrap();
        props
            .set_allowed_values("AutoRange", &["On", "Off"])
            .unwrap();
        props
            .define_property("PowerRange", PropertyValue::Float(100.0), false)
            .unwrap();

        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            power_w: Cell::new(0.0),
            wavelength_nm: Cell::new(488.0),
            auto_range: Cell::new(true),
            power_range_w: Cell::new(100.0),
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
        let cmd = command.to_string();
        self.call_transport(|t| Ok(t.send_recv(&cmd)?.trim().to_string()))
    }

    fn parse_power_unit(resp: &str) -> String {
        let trimmed = resp.trim();
        if trimmed == "1"
            || trimmed.eq_ignore_ascii_case("DBM")
            || trimmed.eq_ignore_ascii_case("dBm")
        {
            "dBm".into()
        } else {
            "W".into()
        }
    }

    fn measure_power_with_unit(&self) -> MmResult<(f64, String)> {
        let unit = Self::parse_power_unit(&self.cmd("SENS:POW:UNIT?")?);
        let power = self.read_raw_power()?;
        Ok((power, unit))
    }

    fn read_raw_power(&self) -> MmResult<f64> {
        let resp = self.cmd("MEAS:POW?")?;
        let val: f64 = resp
            .parse()
            .map_err(|_| MmError::LocallyDefined(format!("Bad power: {}", resp)))?;
        self.power_w.set(val);
        Ok(val)
    }

    fn format_power(power: f64, unit: &str) -> String {
        if unit == "W" {
            if power < 1e-9 {
                return format!("{}nW", power * 1e9);
            } else if power < 1e-6 {
                return format!("{}uW", power * 1e6);
            } else if power < 1e-3 {
                return format!("{}mW", power * 1e3);
            }
        }
        format!("{}{}", power, unit)
    }

    /// Measure power and cache the result.
    pub fn measure_power(&mut self) -> MmResult<f64> {
        self.read_raw_power()
    }

    /// Set wavelength on the device.
    pub fn set_wavelength(&mut self, nm: f64) -> MmResult<()> {
        let cmd = format!("SENS:CORR:WAV {:.2}", nm);
        let _ = self.cmd(&cmd)?;
        self.wavelength_nm.set(nm);
        Ok(())
    }

    /// Set auto-range mode.
    pub fn set_auto_range(&mut self, on: bool) -> MmResult<()> {
        let setting = if on { "ON" } else { "OFF" };
        let cmd = format!("SENS:POW:RANG:AUTO {}", setting);
        let _ = self.cmd(&cmd)?;
        self.auto_range.set(on);
        Ok(())
    }

    /// Set manual power range.
    pub fn set_power_range(&mut self, range_w: f64) -> MmResult<()> {
        let cmd = format!("SENS:POW:RANG {:.6E}", range_w);
        let _ = self.cmd(&cmd)?;
        self.power_range_w.set(range_w);
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
        "ThorlabsPM100"
    }

    fn description(&self) -> &str {
        "Thorlabs PM100x power meter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        // Identify the device
        let idn = self.cmd("*IDN?")?;
        let serial = idn.split(',').nth(2).unwrap_or("").trim().to_string();
        if let Some(entry) = self.props.entry_mut("Sensor Serial Number") {
            entry.value = PropertyValue::String(serial);
        }
        // Read current wavelength
        let wl_resp = self.cmd("SENS:CORR:WAV?")?;
        if let Ok(wl) = wl_resp.parse::<f64>() {
            self.wavelength_nm.set(wl);
        }
        // Read auto-range state
        let ar_resp = self.cmd("SENS:POW:RANG:AUTO?")?;
        self.auto_range
            .set(ar_resp.trim().eq_ignore_ascii_case("on") || ar_resp.trim() == "1");
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        self.transport.borrow_mut().take();
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Power" => {
                let (power, unit) = self.measure_power_with_unit()?;
                Ok(PropertyValue::String(Self::format_power(power, &unit)))
            }
            "RawPower" => Ok(PropertyValue::String(format!(
                "{:.4}",
                self.read_raw_power()?
            ))),
            "RawUnit" => Ok(PropertyValue::String(Self::parse_power_unit(
                &self.cmd("SENS:POW:UNIT?")?,
            ))),
            "Wavelength" => {
                let resp = self.cmd("SENS:CORR:WAV?")?;
                if let Ok(wl) = resp.parse::<f64>() {
                    self.wavelength_nm.set(wl);
                }
                Ok(PropertyValue::Float(self.wavelength_nm.get()))
            }
            "AutoRange" => {
                let resp = self.cmd("SENS:POW:RANG:AUTO?")?;
                self.auto_range
                    .set(resp.trim().eq_ignore_ascii_case("on") || resp.trim() == "1");
                Ok(PropertyValue::String(
                    if self.auto_range.get() { "On" } else { "Off" }.into(),
                ))
            }
            "PowerRange" => {
                let resp = self.cmd("SENS:POW:RANG?")?;
                if let Ok(range) = resp.parse::<f64>() {
                    self.power_range_w.set(range);
                }
                Ok(PropertyValue::Float(self.power_range_w.get()))
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "PowerMeter" if self.initialized => Err(MmError::InvalidProperty),
            "Wavelength" => {
                let nm = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_wavelength(nm)
            }
            "AutoRange" => {
                let s = val.as_str().to_string();
                let on = if s == "On" || s == "1" {
                    true
                } else if s == "Off" || s == "0" {
                    false
                } else {
                    return Err(MmError::InvalidPropertyValue);
                };
                self.set_auto_range(on)
            }
            "PowerRange" => {
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
            "Power"
            | "RawPower"
            | "RawUnit"
            | "Sensor Serial Number"
            | "Calibration Date"
            | "Author" => true,
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
    use crate::transport::MockTransport;

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
        assert!((d.wavelength_nm.get() - 488.0).abs() < 0.01);
        assert!(d.auto_range.get());
        assert_eq!(
            d.get_property("Sensor Serial Number").unwrap(),
            PropertyValue::String("M00123456".into())
        );
        assert!(d.has_property("PowerMeter"));
        assert!(d.has_property("Power"));
        assert!(d.has_property("RawPower"));
        assert!(d.has_property("RawUnit"));
        assert!(d.has_property("Wavelength"));
        assert!(d.has_property("PowerRange"));
        assert_eq!(
            d.get_property("Calibration Date").unwrap(),
            PropertyValue::String(String::new())
        );
        assert_eq!(
            d.get_property("Author").unwrap(),
            PropertyValue::String(String::new())
        );
        assert!(d.is_property_read_only("Calibration Date"));
        assert!(d.is_property_read_only("Author"));
    }

    #[test]
    fn selector_is_bounded_to_transport_resource_placeholder() {
        let d = ThorlabsPM100x::new();
        let allowed = &d.props.entry("PowerMeter").unwrap().allowed_values;
        assert_eq!(allowed, &vec![String::new()]);
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
    fn get_property_reads_live_power_surfaces() {
        let t = MockTransport::new()
            .expect("*IDN?", "Thorlabs,PM100USB,M00123,1.0")
            .expect("SENS:CORR:WAV?", "488.00")
            .expect("SENS:POW:RANG:AUTO?", "ON")
            .expect("SENS:POW:UNIT?", "W")
            .expect("MEAS:POW?", "2.5e-6")
            .expect("MEAS:POW?", "2.5e-6")
            .expect("SENS:POW:UNIT?", "W");
        let mut d = ThorlabsPM100x::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(
            d.get_property("Power").unwrap(),
            PropertyValue::String("0.0025mW".into())
        );
        assert_eq!(
            d.get_property("RawPower").unwrap(),
            PropertyValue::String("0.0000".into())
        );
        assert_eq!(
            d.get_property("RawUnit").unwrap(),
            PropertyValue::String("W".into())
        );
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
        assert!((d.wavelength_nm.get() - 532.0).abs() < 0.01);
    }

    #[test]
    fn shutdown_closes_transport() {
        let mut d = initialized_device();
        d.initialize().unwrap();
        d.shutdown().unwrap();
        assert!(d.measure_power().is_err());
    }

    #[test]
    fn device_type_is_generic() {
        assert_eq!(ThorlabsPM100x::new().device_type(), DeviceType::Generic);
    }

    #[test]
    fn registered_device_name_matches_upstream() {
        assert_eq!(ThorlabsPM100x::new().name(), "ThorlabsPM100");
    }

    #[test]
    fn power_property_is_read_only() {
        assert!(ThorlabsPM100x::new().is_property_read_only("Power"));
    }
}
