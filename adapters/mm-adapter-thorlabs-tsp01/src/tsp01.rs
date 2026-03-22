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
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Generic};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct ThorlabsTSP01 {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    /// Cached internal temperature (°C)
    temp_internal: f64,
    /// Cached humidity (%)
    humidity: f64,
    /// Cached probe 1 temperature (°C)
    temp_probe1: f64,
    /// Cached probe 2 temperature (°C)
    temp_probe2: f64,
}

impl ThorlabsTSP01 {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
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
            transport: None,
            initialized: false,
            temp_internal: 24.0,
            humidity: 50.0,
            temp_probe1: 24.0,
            temp_probe2: 24.0,
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

    fn parse_float(resp: &str, label: &str) -> MmResult<f64> {
        resp.parse::<f64>()
            .map_err(|_| MmError::LocallyDefined(format!("Bad {}: {}", label, resp)))
    }

    /// Read all sensor values and cache them.
    pub fn poll(&mut self) -> MmResult<()> {
        let t = self.cmd("SENS:TEMP:INT?")?;
        self.temp_internal = Self::parse_float(&t, "internal temp")?;

        let h = self.cmd("SENS:HUM?")?;
        self.humidity = Self::parse_float(&h, "humidity")?;

        let p1 = self.cmd("SENS:TEMP:EXT1?")?;
        self.temp_probe1 = Self::parse_float(&p1, "probe1 temp")?;

        let p2 = self.cmd("SENS:TEMP:EXT2?")?;
        self.temp_probe2 = Self::parse_float(&p2, "probe2 temp")?;

        Ok(())
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
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let _idn = self.cmd("*IDN?")?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "USBDeviceTemp" => Ok(PropertyValue::Float(self.temp_internal)),
            "USBDeviceHumidity" => Ok(PropertyValue::Float(self.humidity)),
            "TempProbe1" => Ok(PropertyValue::Float(self.temp_probe1)),
            "TempProbe2" => Ok(PropertyValue::Float(self.temp_probe2)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        self.props.set(name, val)
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
            "USBDeviceTemp" | "USBDeviceHumidity" | "TempProbe1" | "TempProbe2"
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
    use micromanager::transport::MockTransport;

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
        assert!((d.temp_internal - 25.3).abs() < 0.01);
        assert!((d.humidity - 55.1).abs() < 0.01);
        assert!((d.temp_probe1 - 24.8).abs() < 0.01);
        assert!((d.temp_probe2 - 23.9).abs() < 0.01);
    }

    #[test]
    fn get_property_returns_cached() {
        let t = MockTransport::new()
            .expect("*IDN?", "Thorlabs,TSP01,M00123,1.0")
            .expect("SENS:TEMP:INT?", "22.0")
            .expect("SENS:HUM?", "45.0")
            .expect("SENS:TEMP:EXT1?", "21.0")
            .expect("SENS:TEMP:EXT2?", "20.0");
        let mut d = ThorlabsTSP01::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.poll().unwrap();
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
    }

    #[test]
    fn device_type_is_generic() {
        assert_eq!(ThorlabsTSP01::new().device_type(), DeviceType::Generic);
    }
}
