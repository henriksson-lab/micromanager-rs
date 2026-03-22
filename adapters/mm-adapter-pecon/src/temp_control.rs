/// Pecon Incubation TempControl 37-2 temperature controller.
///
/// Protocol — raw bytes, NO line terminator on commands, 3-byte binary responses:
///   `A000`  → 3 bytes: echo `A00<addr_digit>`   (detect/select device)
///   `S000`  → 3 bytes: device code               (verify device type)
///   `R100`  → 3 bytes: BCD temperature CH1       (read real temp)
///   `R200`  → 3 bytes: BCD temperature CH2
///   `N<d><u><t>`→ 3 bytes: echo of set value     (set nominal temp CH1;
///              d=tens, u=units, t=tenths ASCII digits, e.g. `N375` = 37.5 °C)
///   `O<d><u><t>`→ 3 bytes: set nominal temp CH2
///   `H100`  → 3 bytes: turn heating CH1 OFF
///   `H101`  → 3 bytes: turn heating CH1 ON
///   `H200`  → 3 bytes: turn heating CH2 OFF
///   `H201`  → 3 bytes: turn heating CH2 ON
///
/// Temperature BCD decoding:
///   If byte[0] == b'-': temp = -(byte[1]-b'0' + 0.1*(byte[2]-b'0'))
///   Else:               temp = (byte[0]-b'0')*10 + (byte[1]-b'0') + 0.1*(byte[2]-b'0')
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::Device;
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct PeconTempControl {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    nominal_temp: [f64; 2],
    real_temp: [f64; 2],
    heating_on: [bool; 2],
}

impl PeconTempControl {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Channel1_NominalTemperature", PropertyValue::Float(37.0), false).unwrap();
        props.define_property("Channel2_NominalTemperature", PropertyValue::Float(37.0), false).unwrap();
        props.define_property("Channel1_RealTemperature",    PropertyValue::Float(0.0),  true).unwrap();
        props.define_property("Channel2_RealTemperature",    PropertyValue::Float(0.0),  true).unwrap();
        props.define_property("Channel1_Heating", PropertyValue::String("Off".into()), false).unwrap();
        props.set_allowed_values("Channel1_Heating", &["On", "Off"]).unwrap();
        props.define_property("Channel2_Heating", PropertyValue::String("Off".into()), false).unwrap();
        props.set_allowed_values("Channel2_Heating", &["On", "Off"]).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            nominal_temp: [37.0; 2],
            real_temp: [0.0; 2],
            heating_on: [false; 2],
        }
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

    /// Send 4-char command (no terminator) and read exactly 3 bytes.
    fn raw_cmd(&mut self, cmd: &str) -> MmResult<Vec<u8>> {
        let c = cmd.to_string();
        self.call_transport(|t| {
            t.send(&c)?;
            t.receive_bytes(3)
        })
    }

    /// Decode a 3-byte BCD temperature response.
    pub fn decode_temp(bytes: &[u8]) -> f64 {
        if bytes.len() < 3 { return 0.0; }
        if bytes[0] == b'-' {
            -(((bytes[1].wrapping_sub(b'0')) as f64)
                + 0.1 * (bytes[2].wrapping_sub(b'0')) as f64)
        } else {
            (bytes[0].wrapping_sub(b'0')) as f64 * 10.0
                + (bytes[1].wrapping_sub(b'0')) as f64
                + 0.1 * (bytes[2].wrapping_sub(b'0')) as f64
        }
    }

    /// Encode temperature to the last 3 chars of a command like "N375" for 37.5 °C.
    pub fn encode_temp(prefix: char, temp: f64) -> String {
        let temp = temp.abs().min(99.9);
        let tens  = (temp / 10.0) as u8;
        let units = (temp as u8) % 10;
        let tenths = ((temp * 10.0).round() as u8) % 10;
        format!("{}{}{}{}", prefix, tens, units, tenths)
    }

    fn read_temp(&mut self, channel: usize) -> MmResult<f64> {
        let cmd = if channel == 0 { "R100" } else { "R200" };
        let bytes = self.raw_cmd(cmd)?;
        Ok(Self::decode_temp(&bytes))
    }

    fn set_temp(&mut self, channel: usize, temp: f64) -> MmResult<()> {
        let prefix = if channel == 0 { 'N' } else { 'O' };
        self.raw_cmd(&Self::encode_temp(prefix, temp))?;
        self.nominal_temp[channel] = temp;
        let key = format!("Channel{}_NominalTemperature", channel + 1);
        self.props.entry_mut(&key).map(|e| e.value = PropertyValue::Float(temp));
        Ok(())
    }

    fn set_heating(&mut self, channel: usize, on: bool) -> MmResult<()> {
        // H100=CH1 off, H101=CH1 on, H200=CH2 off, H201=CH2 on
        let cmd = format!("H{}0{}", channel + 1, if on { 1 } else { 0 });
        self.raw_cmd(&cmd)?;
        self.heating_on[channel] = on;
        let key = format!("Channel{}_Heating", channel + 1);
        let val = if on { "On" } else { "Off" };
        self.props.entry_mut(&key).map(|e| e.value = PropertyValue::String(val.into()));
        Ok(())
    }
}

impl Default for PeconTempControl { fn default() -> Self { Self::new() } }

impl Device for PeconTempControl {
    fn name(&self) -> &str { "PeconTempControl" }
    fn description(&self) -> &str { "Pecon Incubation TempControl 37-2" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Auto-address: check address 0
        let r = self.raw_cmd("A000")?;
        if r.len() < 3 || r[2] != b'0' + 0 {
            return Err(MmError::LocallyDefined("Pecon device not found".into()));
        }
        // Read initial temperatures
        let t1 = self.read_temp(0)?;
        let t2 = self.read_temp(1)?;
        self.real_temp[0] = t1;
        self.real_temp[1] = t2;
        self.props.entry_mut("Channel1_RealTemperature").map(|e| e.value = PropertyValue::Float(t1));
        self.props.entry_mut("Channel2_RealTemperature").map(|e| e.value = PropertyValue::Float(t2));
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.set_heating(0, false);
            let _ = self.set_heating(1, false);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Channel1_NominalTemperature" => Ok(PropertyValue::Float(self.nominal_temp[0])),
            "Channel2_NominalTemperature" => Ok(PropertyValue::Float(self.nominal_temp[1])),
            "Channel1_RealTemperature"    => Ok(PropertyValue::Float(self.real_temp[0])),
            "Channel2_RealTemperature"    => Ok(PropertyValue::Float(self.real_temp[1])),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Channel1_NominalTemperature" => {
                let t = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized { self.set_temp(0, t)?; } else { self.nominal_temp[0] = t; }
                Ok(())
            }
            "Channel2_NominalTemperature" => {
                let t = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized { self.set_temp(1, t)?; } else { self.nominal_temp[1] = t; }
                Ok(())
            }
            "Channel1_Heating" => {
                let on = val.as_str() == "On";
                if self.initialized { self.set_heating(0, on)?; } else { self.heating_on[0] = on; }
                Ok(())
            }
            "Channel2_Heating" => {
                let on = val.as_str() == "On";
                if self.initialized { self.set_heating(1, on)?; } else { self.heating_on[1] = on; }
                Ok(())
            }
            _ => self.props.set(name, val),
        }
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
    use micromanager::transport::MockTransport;

    fn make_transport() -> MockTransport {
        // A000 → 3 bytes "A00" + address byte (address 0 → '0')
        // R100 → 3 bytes "375" = 37.5°C
        // R200 → 3 bytes "200" = 20.0°C
        MockTransport::new()
            .expect_binary(b"A00")   // A000 response: last byte is address
            .expect_binary(b"375")   // R100 → 37.5°C
            .expect_binary(b"200")   // R200 → 20.0°C
    }

    #[test]
    fn initialize() {
        let mut dev = PeconTempControl::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!((dev.real_temp[0] - 37.5).abs() < 0.05);
        assert!((dev.real_temp[1] - 20.0).abs() < 0.05);
    }

    #[test]
    fn decode_temp_positive() {
        // "375" → 37.5
        assert!((PeconTempControl::decode_temp(b"375") - 37.5).abs() < 0.001);
        // "050" → 5.0
        assert!((PeconTempControl::decode_temp(b"050") - 5.0).abs() < 0.001);
    }

    #[test]
    fn decode_temp_negative() {
        // "-15" → -1.5
        assert!((PeconTempControl::decode_temp(b"-15") - (-1.5)).abs() < 0.001);
    }

    #[test]
    fn encode_temp() {
        assert_eq!(PeconTempControl::encode_temp('N', 37.5), "N375");
        assert_eq!(PeconTempControl::encode_temp('N', 5.0),  "N050");
        assert_eq!(PeconTempControl::encode_temp('O', 20.0), "O200");
    }

    #[test]
    fn no_transport_error() { assert!(PeconTempControl::new().initialize().is_err()); }
}
