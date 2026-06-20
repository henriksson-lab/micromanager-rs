/// Thorlabs TSP01 temperature/humidity sensor adapter.
///
/// The TSP01 is a USB device (originally using a vendor DLL/VISA). This Rust
/// adapter uses a serial (USB-VCP) SCPI-like protocol, following the same
/// approach as the PM100x adapter.
///
/// SCPI commands:
///   `*IDN?`                   → identification string
///   `SENS:TEMP:INT?`          → internal USB-device temperature (°C)
///   `SENS:HUM?`               → relative humidity (%)
///   `SENS:TEMP:EXT1?`         → external probe 1 temperature (°C)
///   `SENS:TEMP:EXT2?`         → external probe 2 temperature (°C)
///
/// Implements `Generic` device type; readings are exposed as read-only properties.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};

pub struct ThorlabsTSP01 {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    /// Cached internal temperature (°C)
    temp_internal: Cell<f64>,
    /// Cached humidity (%)
    humidity: Cell<f64>,
    /// Cached probe 1 temperature (°C)
    temp_probe1: Cell<f64>,
    /// Cached probe 2 temperature (°C)
    temp_probe2: Cell<f64>,
}

impl ThorlabsTSP01 {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Thermometer", PropertyValue::String("ThorlabsTSP01".into()))
            .unwrap();
        props
            .set_allowed_values("Thermometer", &["ThorlabsTSP01"])
            .unwrap();
        props
            .define_property(
                "Sensor Serial Number",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("USBDeviceTemp", PropertyValue::Float(24.0), true)
            .unwrap();
        props
            .define_property("USBDeviceHumidity", PropertyValue::Float(50.0), true)
            .unwrap();
        props
            .define_property("TempProbe1", PropertyValue::Float(24.0), true)
            .unwrap();
        props
            .define_property("TempProbe2", PropertyValue::Float(24.0), true)
            .unwrap();

        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            temp_internal: Cell::new(24.0),
            humidity: Cell::new(50.0),
            temp_probe1: Cell::new(24.0),
            temp_probe2: Cell::new(24.0),
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

    fn parse_float(resp: &str, label: &str) -> MmResult<f64> {
        resp.parse::<f64>()
            .map_err(|_| MmError::LocallyDefined(format!("Bad {}: {}", label, resp)))
    }

    /// Read all sensor values and cache them.
    pub fn poll(&mut self) -> MmResult<()> {
        self.poll_update_once().map(|_| ())
    }

    /// Run one synchronous pass of the upstream background polling loop.
    pub fn poll_update_once(&self) -> MmResult<bool> {
        let old_internal = self.temp_internal.get();
        let old_humidity = self.humidity.get();
        let old_probe1 = self.temp_probe1.get();
        let old_probe2 = self.temp_probe2.get();

        let internal = self.read_internal_temp()?;
        let humidity = self.read_humidity()?;
        let probe1 = self.read_probe1_temp()?;
        let probe2 = self.read_probe2_temp()?;

        Ok(internal != old_internal
            || humidity != old_humidity
            || probe1 != old_probe1
            || probe2 != old_probe2)
    }

    fn read_internal_temp(&self) -> MmResult<f64> {
        let t = self.cmd("SENS:TEMP:INT?")?;
        let val = Self::parse_float(&t, "internal temp")?;
        self.temp_internal.set(val);
        Ok(val)
    }

    fn read_humidity(&self) -> MmResult<f64> {
        let h = self.cmd("SENS:HUM?")?;
        let val = Self::parse_float(&h, "humidity")?;
        self.humidity.set(val);
        Ok(val)
    }

    fn read_probe1_temp(&self) -> MmResult<f64> {
        let p1 = self.cmd("SENS:TEMP:EXT1?")?;
        let val = Self::parse_float(&p1, "probe1 temp")?;
        self.temp_probe1.set(val);
        Ok(val)
    }

    fn read_probe2_temp(&self) -> MmResult<f64> {
        let p2 = self.cmd("SENS:TEMP:EXT2?")?;
        let val = Self::parse_float(&p2, "probe2 temp")?;
        self.temp_probe2.set(val);
        Ok(val)
    }
}

impl Default for ThorlabsTSP01 {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ThorlabsTSP01 {
    fn name(&self) -> &str {
        "ThorlabsTSP01"
    }

    fn description(&self) -> &str {
        "Thorlabs TSP01 temperature/humidity sensor"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        let idn = self.cmd("*IDN?")?;
        let serial = idn.split(',').nth(2).unwrap_or("").trim().to_string();
        if let Some(entry) = self.props.entry_mut("Sensor Serial Number") {
            entry.value = PropertyValue::String(serial);
        }
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
            "USBDeviceTemp" => Ok(PropertyValue::Float(self.read_internal_temp()?)),
            "USBDeviceHumidity" => Ok(PropertyValue::Float(self.read_humidity()?)),
            "TempProbe1" => Ok(PropertyValue::Float(self.read_probe1_temp()?)),
            "TempProbe2" => Ok(PropertyValue::Float(self.read_probe2_temp()?)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Thermometer" if self.initialized => Err(MmError::InvalidProperty),
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
        matches!(
            name,
            "USBDeviceTemp"
                | "USBDeviceHumidity"
                | "TempProbe1"
                | "TempProbe2"
                | "Sensor Serial Number"
        ) || self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Generic
    }

    fn busy(&self) -> bool {
        false
    }
}

impl Generic for ThorlabsTSP01 {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_initialized() -> ThorlabsTSP01 {
        let t = MockTransport::new().expect("*IDN?", "Thorlabs,TSP01,M00123,1.0");
        let mut d = ThorlabsTSP01::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d
    }

    #[test]
    fn initialize_succeeds() {
        let d = make_initialized();
        assert!(d.initialized);
        assert!(d.has_property("Thermometer"));
        assert_eq!(
            d.get_property("Sensor Serial Number").unwrap(),
            PropertyValue::String("M00123".into())
        );
    }

    #[test]
    fn no_transport_error() {
        assert!(ThorlabsTSP01::new().initialize().is_err());
    }

    #[test]
    fn poll_reads_all_channels() {
        let t = MockTransport::new()
            .expect("*IDN?", "Thorlabs,TSP01,M00123,1.0")
            .expect("SENS:TEMP:INT?", "25.3")
            .expect("SENS:HUM?", "55.1")
            .expect("SENS:TEMP:EXT1?", "24.8")
            .expect("SENS:TEMP:EXT2?", "23.9");
        let mut d = ThorlabsTSP01::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.poll().unwrap();
        assert!((d.temp_internal.get() - 25.3).abs() < 0.01);
        assert!((d.humidity.get() - 55.1).abs() < 0.01);
        assert!((d.temp_probe1.get() - 24.8).abs() < 0.01);
        assert!((d.temp_probe2.get() - 23.9).abs() < 0.01);
    }

    #[test]
    fn poll_update_once_reports_whether_values_changed() {
        let t = MockTransport::new()
            .expect("*IDN?", "Thorlabs,TSP01,M00123,1.0")
            .expect("SENS:TEMP:INT?", "24.0")
            .expect("SENS:HUM?", "50.0")
            .expect("SENS:TEMP:EXT1?", "24.0")
            .expect("SENS:TEMP:EXT2?", "24.0")
            .expect("SENS:TEMP:INT?", "25.0")
            .expect("SENS:HUM?", "50.0")
            .expect("SENS:TEMP:EXT1?", "24.0")
            .expect("SENS:TEMP:EXT2?", "24.0");
        let mut d = ThorlabsTSP01::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert!(!d.poll_update_once().unwrap());
        assert!(d.poll_update_once().unwrap());
        assert!((d.temp_internal.get() - 25.0).abs() < 0.01);
    }

    #[test]
    fn get_property_reads_live_value() {
        let t = MockTransport::new()
            .expect("*IDN?", "Thorlabs,TSP01,M00123,1.0")
            .expect("SENS:TEMP:INT?", "22.0");
        let mut d = ThorlabsTSP01::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        let v = d.get_property("USBDeviceTemp").unwrap();
        if let PropertyValue::Float(f) = v {
            assert!((f - 22.0).abs() < 0.01);
        } else {
            panic!("Expected Float");
        }
    }

    #[test]
    fn sensor_properties_are_read_only() {
        let d = ThorlabsTSP01::new();
        assert!(d.is_property_read_only("USBDeviceTemp"));
        assert!(d.is_property_read_only("USBDeviceHumidity"));
        assert!(d.is_property_read_only("TempProbe1"));
        assert!(d.is_property_read_only("TempProbe2"));
        assert!(d.is_property_read_only("Sensor Serial Number"));
    }

    #[test]
    fn thermometer_selector_is_bounded_to_current_transport_resource() {
        let d = ThorlabsTSP01::new();
        let allowed = &d.props.entry("Thermometer").unwrap().allowed_values;
        assert_eq!(allowed, &vec!["ThorlabsTSP01".to_string()]);
    }

    #[test]
    fn initialized_thermometer_change_is_rejected() {
        let mut d = make_initialized();
        assert_eq!(
            d.set_property("Thermometer", PropertyValue::String("other".into()))
                .unwrap_err(),
            MmError::InvalidProperty
        );
    }

    #[test]
    fn shutdown_closes_transport() {
        let mut d = make_initialized();
        d.shutdown().unwrap();
        assert!(d.poll().is_err());
    }

    #[test]
    fn device_type_is_generic() {
        assert_eq!(ThorlabsTSP01::new().device_type(), DeviceType::Generic);
    }
}
