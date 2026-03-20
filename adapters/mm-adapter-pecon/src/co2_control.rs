/// Pecon CO2 Controller.
///
/// Protocol — same raw 3-byte scheme as TempControl:
///   `A000`  → 3 bytes: echo `A00<addr>`  (select device)
///   `S000`  → 3 bytes: device status
///   `R000`  → 3 bytes: BCD actual CO2 %  (e.g. `052` = 5.2%)
///   `N000`  → 3 bytes: BCD nominal CO2 %
///   With value encoded: `N052` = set nominal to 5.2%
///
/// CO2 BCD: same encoding as temperature but represents percentage (0.0–99.9%).
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::Device;
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

use crate::temp_control::PeconTempControl;

pub struct PeconCO2Control {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    nominal_co2: f64,
    actual_co2: f64,
}

impl PeconCO2Control {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("CO2_Nominal_%", PropertyValue::Float(5.0), false).unwrap();
        props.define_property("CO2_Actual_%",  PropertyValue::Float(0.0), true).unwrap();
        Self { props, transport: None, initialized: false, nominal_co2: 5.0, actual_co2: 0.0 }
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

    fn raw_cmd(&mut self, cmd: &str) -> MmResult<Vec<u8>> {
        let c = cmd.to_string();
        self.call_transport(|t| {
            t.send(&c)?;
            t.receive_bytes(3)
        })
    }
}

impl Default for PeconCO2Control { fn default() -> Self { Self::new() } }

impl Device for PeconCO2Control {
    fn name(&self) -> &str { "PeconCO2Control" }
    fn description(&self) -> &str { "Pecon CO2 Controller" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let r = self.raw_cmd("A000")?;
        if r.len() < 3 || r[2] != b'0' {
            return Err(MmError::LocallyDefined("Pecon CO2 device not found".into()));
        }
        let bytes = self.raw_cmd("R000")?;
        self.actual_co2 = PeconTempControl::decode_temp(&bytes);
        self.props.entry_mut("CO2_Actual_%").map(|e| e.value = PropertyValue::Float(self.actual_co2));
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "CO2_Nominal_%" => Ok(PropertyValue::Float(self.nominal_co2)),
            "CO2_Actual_%"  => Ok(PropertyValue::Float(self.actual_co2)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "CO2_Nominal_%" {
            let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            if self.initialized {
                self.raw_cmd(&PeconTempControl::encode_temp('N', pct))?;
            }
            self.nominal_co2 = pct;
            self.props.entry_mut("CO2_Nominal_%").map(|e| e.value = PropertyValue::Float(pct));
            return Ok(());
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

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    #[test]
    fn initialize() {
        let t = MockTransport::new()
            .expect_binary(b"A00")   // A000 response
            .expect_binary(b"052");  // R000 → 5.2%
        let mut dev = PeconCO2Control::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert!((dev.actual_co2 - 5.2).abs() < 0.05);
    }

    #[test]
    fn set_nominal_co2() {
        let t = MockTransport::new()
            .expect_binary(b"A00")
            .expect_binary(b"052")
            .expect_binary(b"070"); // N070 response for 7.0%
        let mut dev = PeconCO2Control::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("CO2_Nominal_%", PropertyValue::Float(7.0)).unwrap();
        assert_eq!(dev.nominal_co2, 7.0);
    }

    #[test]
    fn no_transport_error() { assert!(PeconCO2Control::new().initialize().is_err()); }
}
