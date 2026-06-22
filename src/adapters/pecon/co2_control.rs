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
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::Device;
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

use super::temp_control::PeconTempControl;

pub struct PeconCO2Control {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    address: u8,
    nominal_co2: f64,
    actual_co2: f64,
}

impl PeconCO2Control {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("CO2_Nominal_%", PropertyValue::Float(5.0), false)
            .unwrap();
        props
            .define_property("CO2_Actual_%", PropertyValue::Float(0.0), true)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            address: 0,
            nominal_co2: 5.0,
            actual_co2: 0.0,
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

    fn raw_cmd(&mut self, cmd: &str) -> MmResult<Vec<u8>> {
        self.raw_cmd_bytes(cmd, 3)
    }

    fn raw_cmd_bytes(&mut self, cmd: &str, count: usize) -> MmResult<Vec<u8>> {
        let c = cmd.to_string();
        self.call_transport(|t| {
            t.send(&c)?;
            t.receive_bytes(count)
        })
    }

    fn wake_up(&mut self) -> MmResult<()> {
        let cmd = format!("A00{}", self.address);
        let answer = self.raw_cmd(&cmd)?;
        if answer.len() >= 3
            && answer[0] == b'0'
            && answer[1] == b'0'
            && answer[2] == b'0' + self.address
        {
            Ok(())
        } else {
            Err(MmError::SerialInvalidResponse)
        }
    }

    fn find_device_address(&mut self) -> MmResult<()> {
        for address in 0..=5 {
            let answer = self.raw_cmd(&format!("A00{}", address))?;
            if answer.len() < 3 || answer[2] != b'0' + address {
                continue;
            }
            let status = self.raw_cmd("S000")?;
            if status.len() >= 3 && status[1] == b'2' {
                self.address = address;
                return Ok(());
            }
        }
        Err(MmError::SerialInvalidResponse)
    }

    fn read_status_frame(&mut self) -> MmResult<Vec<u8>> {
        self.wake_up()?;
        self.raw_cmd_bytes("S001", 11)
    }

    fn read_nominal_from_status(&mut self) -> MmResult<f64> {
        let status = self.read_status_frame()?;
        if status.len() < 6 {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(PeconTempControl::decode_temp(&status[3..6]))
    }
}

impl Default for PeconCO2Control {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for PeconCO2Control {
    fn name(&self) -> &str {
        "CO2Controller"
    }
    fn description(&self) -> &str {
        "Pecon Incubation CO2 Controller adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.find_device_address()?;
        self.wake_up()?;
        let bytes = self.raw_cmd("R000")?;
        self.actual_co2 = PeconTempControl::decode_temp(&bytes);
        self.nominal_co2 = self.read_nominal_from_status()?;
        self.props
            .entry_mut("CO2_Actual_%")
            .map(|e| e.value = PropertyValue::Float(self.actual_co2));
        self.props
            .entry_mut("CO2_Nominal_%")
            .map(|e| e.value = PropertyValue::Float(self.nominal_co2));
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "CO2_Nominal_%" => Ok(PropertyValue::Float(self.nominal_co2)),
            "CO2_Actual_%" => Ok(PropertyValue::Float(self.actual_co2)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "CO2_Nominal_%" {
            let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            let pct = pct.clamp(0.0, 10.0);
            if self.initialized {
                self.wake_up()?;
                let cmd = PeconTempControl::encode_temp('N', pct);
                let answer = self.raw_cmd(&cmd)?;
                if answer.as_slice() != cmd.as_bytes().get(1..).unwrap_or_default() {
                    return Err(MmError::SerialInvalidResponse);
                }
            }
            self.nominal_co2 = pct;
            self.props
                .entry_mut("CO2_Nominal_%")
                .map(|e| e.value = PropertyValue::Float(pct));
            return Ok(());
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
        DeviceType::Generic
    }
    fn busy(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize() {
        let t = MockTransport::new()
            .expect_binary(b"000")
            .expect_binary(b"020")
            .expect_binary(b"000")
            .expect_binary(b"052") // R000 → 5.2%
            .expect_binary(b"000")
            .expect_binary(b"05207010000"); // S001 → nominal 7.0%
        let mut dev = PeconCO2Control::new().with_transport(Box::new(t));
        assert_eq!(dev.name(), "CO2Controller");
        assert_eq!(dev.description(), "Pecon Incubation CO2 Controller adapter");
        dev.initialize().unwrap();
        assert!((dev.actual_co2 - 5.2).abs() < 0.05);
        assert_eq!(
            dev.get_property("CO2_Nominal_%").unwrap(),
            PropertyValue::Float(7.0)
        );
    }

    #[test]
    fn set_nominal_co2() {
        let t = MockTransport::new()
            .expect_binary(b"000")
            .expect_binary(b"020")
            .expect_binary(b"000")
            .expect_binary(b"052")
            .expect_binary(b"000")
            .expect_binary(b"05205010000")
            .expect_binary(b"000")
            .expect_binary(b"070"); // N070 response for 7.0%
        let mut dev = PeconCO2Control::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("CO2_Nominal_%", PropertyValue::Float(7.0))
            .unwrap();
        assert_eq!(dev.nominal_co2, 7.0);
    }

    #[test]
    fn set_nominal_clamps_and_keeps_cache_on_bad_echo() {
        let t = MockTransport::new()
            .expect_binary(b"000")
            .expect_binary(b"020")
            .expect_binary(b"000")
            .expect_binary(b"052")
            .expect_binary(b"000")
            .expect_binary(b"05205010000")
            .expect_binary(b"000")
            .expect_binary(b"999");
        let mut dev = PeconCO2Control::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert!(dev
            .set_property("CO2_Nominal_%", PropertyValue::Float(12.0))
            .is_err());
        assert_eq!(
            dev.get_property("CO2_Nominal_%").unwrap(),
            PropertyValue::Float(5.0)
        );
    }

    #[test]
    fn no_transport_error() {
        assert!(PeconCO2Control::new().initialize().is_err());
    }
}
