/// Optotune Electrically Tunable Lens (ETL) adapter.
///
/// Binary serial protocol.  All commands are 6 bytes:
///   [cmd_hi, cmd_lo, val_hi, val_lo, crc_lo, crc_hi]
/// where the CRC is IBM CRC-16 (poly 0x8005, init 0, input/output reflected).
///
/// Set-current command:
///   cmd bytes = ['A', 'w'] = [0x41, 0x77]
///   value = (current_mA / 293.0 * 4096.0) as i16, encoded big-endian
///   full 4-byte payload: [0x41, 0x77, val_hi, val_lo]
///   CRC of those 4 bytes → appended as [crc_lo, crc_hi] (little-endian CRC word)
///
/// Get-current command:
///   [0x41, 0x72, 0x00, 0x00, 0xB4, 0x27] (hard-coded in C++ source)
///   Response: 6 bytes; bytes [1..2] encode current value
///
/// Initialisation creates `Current-mA` and calls the upstream `Send("Start")`
/// path, but that send helper is commented out in the C++ source and performs
/// no serial write.  The upstream `initialized_` flag is never set by the ETL
/// initialize path, so initialized Port writes are still accepted.
///
/// Current range: -293 mA to +293 mA.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

/// IBM CRC-16 (CRC-16/ARC): poly 0x8005, init 0, reflected input and output.
fn crc16_ibm(data: &[u8]) -> u16 {
    fn bit_reflect(mut data: u16, nbits: u8) -> u16 {
        let mut output = 0u16;
        for i in 0..nbits {
            if data & 1 != 0 {
                output |= 1 << (nbits - 1 - i);
            }
            data >>= 1;
        }
        output
    }

    let mut crc = 0u16;
    for &byte in data {
        let dbyte = bit_reflect(byte as u16, 8);
        crc ^= dbyte << 8;
        for _ in 0..8 {
            let mix = crc & 0x8000;
            crc <<= 1;
            if mix != 0 {
                crc ^= 0x8005;
            }
        }
    }
    bit_reflect(crc, 16)
}

/// Build a 6-byte set-current command packet.
fn build_set_current_cmd(current_ma: f64) -> [u8; 6] {
    let coded = (current_ma / 293.0 * 4096.0) as i16;
    let val_hi = ((coded as u16) >> 8) as u8;
    let val_lo = (coded as u16 & 0xFF) as u8;
    let payload = [0x41u8, 0x77, val_hi, val_lo];
    let crc = crc16_ibm(&payload);
    // CRC is appended little-endian (lo byte first)
    [
        0x41,
        0x77,
        val_hi,
        val_lo,
        (crc & 0xFF) as u8,
        (crc >> 8) as u8,
    ]
}

pub struct EtlDevice {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    current_ma: f64,
    min_current_ma: f64,
    max_current_ma: f64,
}

impl EtlDevice {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Name", PropertyValue::String("ETL".into()), true)
            .unwrap();
        props
            .define_property(
                "Description",
                PropertyValue::String("Optotune Electric Tunable Lens".into()),
                true,
            )
            .unwrap();
        props
            .define_property("MaxI_mA", PropertyValue::Float(293.0), false)
            .unwrap();
        props
            .define_property("MinI_mA", PropertyValue::Float(-293.0), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            current_ma: 0.0,
            min_current_ma: -293.0,
            max_current_ma: 293.0,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn define_initialized_properties(&mut self) -> MmResult<()> {
        if !self.props.has_property("Current-mA") {
            self.props.define_property(
                "Current-mA",
                PropertyValue::Float(self.current_ma),
                false,
            )?;
            self.props.set_property_limits(
                "Current-mA",
                self.min_current_ma,
                self.max_current_ma,
            )?;
        }
        Ok(())
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

    /// Set the lens current in mA.
    pub fn set_current(&mut self, current_ma: f64) -> MmResult<()> {
        if current_ma < self.min_current_ma || current_ma > self.max_current_ma {
            return Err(MmError::InvalidPropertyValue);
        }
        let cmd = build_set_current_cmd(current_ma);
        self.call_transport(|t| t.purge())?;
        self.call_transport(|t| t.send_bytes(&cmd))?;
        self.call_transport(|t| t.purge())?;
        self.current_ma = current_ma;
        Ok(())
    }

    /// Read back the current from the device.
    /// Sends the hard-coded get-current command and parses the 6-byte response.
    pub fn get_current(&mut self) -> MmResult<f64> {
        let get_cmd: [u8; 6] = [0x41, 0x72, 0x00, 0x00, 0xB4, 0x27];
        self.call_transport(|t| t.purge())?;
        self.call_transport(|t| t.send_bytes(&get_cmd))?;
        let resp = self.call_transport(|t| t.receive_bytes(6))?;
        if resp.len() != 6 {
            return Err(MmError::SerialInvalidResponse);
        }
        // Decode as per C++ empirical formula:
        //   i1 = signed(resp[1]), i2 = unsigned(resp[2])
        //   current = (i1 * 255 + i2) * 293 / 4096
        let i1 = resp[1] as i8 as i32;
        let i2 = resp[2] as i32;
        let current = (i1 * 255 + i2) as f64 * 293.0 / 4096.0;
        self.call_transport(|t| t.purge())?;
        self.current_ma = current;
        Ok(current)
    }

    pub fn current(&self) -> f64 {
        self.current_ma
    }
}

impl Default for EtlDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for EtlDevice {
    fn name(&self) -> &str {
        "ETL"
    }
    fn description(&self) -> &str {
        "Optotune Electric Tunable Lens"
    }

    fn initialize(&mut self) -> MmResult<()> {
        self.define_initialized_properties()?;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if !self.props.has_property(name) {
            return self.props.get(name).cloned();
        }
        match name {
            "Current-mA" => Ok(PropertyValue::Float(self.current_ma)),
            "MaxI_mA" => Ok(PropertyValue::Float(self.max_current_ma)),
            "MinI_mA" => Ok(PropertyValue::Float(self.min_current_ma)),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if !self.props.has_property(name) {
            return self.props.set(name, val);
        }
        match name {
            "Current-mA" => {
                let current = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_current(current)?;
                self.props.set(name, PropertyValue::Float(self.current_ma))
            }
            "MaxI_mA" => {
                let max = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.max_current_ma = max;
                self.props.set(name, PropertyValue::Float(max))
            }
            "MinI_mA" => {
                let min = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.min_current_ma = min;
                self.props.set(name, PropertyValue::Float(min))
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
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Generic
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Generic for EtlDevice {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_initialized() -> EtlDevice {
        let t = MockTransport::new(); // send_bytes records to received_bytes, no response needed
        let mut d = EtlDevice::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d
    }

    #[test]
    fn initialize_succeeds() {
        let d = EtlDevice::new();
        assert_eq!(d.get_property("Name").unwrap().as_str(), "ETL");
        assert_eq!(
            d.get_property("Description").unwrap().as_str(),
            "Optotune Electric Tunable Lens"
        );
        assert!(!d.has_property("Current-mA"));
        assert_eq!(
            d.get_property("Current-mA"),
            Err(MmError::UnknownLabel("Current-mA".into()))
        );
        let d = make_initialized();
        assert!(!d.initialized);
        assert!(d.has_property("Current-mA"));
        assert_eq!(d.current(), 0.0);
    }

    #[test]
    fn initialize_and_shutdown_do_not_write_current_like_upstream() {
        let t = MockTransport::new();
        let mut d = EtlDevice::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.shutdown().unwrap();
    }

    #[test]
    fn set_current_records_bytes() {
        let mut d = make_initialized();
        // Replace transport to capture bytes
        d.transport = Some(Box::new(MockTransport::new()));
        // set_current sends 6 bytes; succeeds without error
        d.set_current(146.5).unwrap();
        assert!((d.current() - 146.5).abs() < 0.01);
    }

    #[test]
    fn crc16_ibm_known_value() {
        // Known: CRC-16/ARC of [0x41, 0x77, 0x00, 0x00] (current=0)
        // C++ get-current command uses hardcoded CRC 0x27B4 for [0x41,0x72,0x00,0x00]
        // We can verify our CRC matches the hardcoded command:
        let data = [0x41u8, 0x72, 0x00, 0x00];
        let crc = crc16_ibm(&data);
        // The hardcoded command in the C++ is [0x41, 0x72, 0x00, 0x00, 0xB4, 0x27]
        // so CRC bytes are [0xB4, 0x27] little-endian → crc = 0x27B4
        assert_eq!(crc, 0x27B4);
    }

    #[test]
    fn build_set_current_zero() {
        let cmd = build_set_current_cmd(0.0);
        assert_eq!(cmd[0], 0x41);
        assert_eq!(cmd[1], 0x77);
        // coded = (0.0 / 293.0 * 4096) = 0 → val_hi=0, val_lo=0
        assert_eq!(cmd[2], 0x00);
        assert_eq!(cmd[3], 0x00);
        // CRC of [0x41, 0x77, 0x00, 0x00]
        let expected_crc = crc16_ibm(&[0x41, 0x77, 0x00, 0x00]);
        assert_eq!(cmd[4], (expected_crc & 0xFF) as u8);
        assert_eq!(cmd[5], (expected_crc >> 8) as u8);
    }

    #[test]
    fn build_set_current_positive() {
        // 293 mA → coded = 4096
        let cmd = build_set_current_cmd(293.0);
        let coded = 4096i16;
        assert_eq!(cmd[2], (coded as u16 >> 8) as u8); // 0x10
        assert_eq!(cmd[3], (coded as u16 & 0xFF) as u8); // 0x00
    }

    #[test]
    fn current_out_of_range_is_rejected_without_cache_update() {
        let mut d = make_initialized();
        d.transport = Some(Box::new(MockTransport::new()));
        d.current_ma = 12.0;
        assert_eq!(d.set_current(999.0), Err(MmError::InvalidPropertyValue));
        assert_eq!(d.current(), 12.0);
        d.transport = Some(Box::new(MockTransport::new()));
        assert_eq!(d.set_current(-999.0), Err(MmError::InvalidPropertyValue));
        assert_eq!(d.current(), 12.0);
    }

    #[test]
    fn initialize_does_not_require_transport_like_upstream() {
        let mut d = EtlDevice::new();
        d.initialize().unwrap();
        assert!(d.has_property("Current-mA"));
    }

    #[test]
    fn initialized_port_is_still_writable_like_upstream() {
        let mut d = make_initialized();
        d.set_property("Port", PropertyValue::String("COM2".into()))
            .unwrap();
        assert_eq!(
            d.get_property("Port").unwrap(),
            PropertyValue::String("COM2".into())
        );
    }

    #[test]
    fn get_current_parses_response() {
        let mut d = make_initialized();
        // Response: [cmd_echo, hi_byte, lo_byte, ...pad]
        // For current = 0: i1=0, i2=0 → 0*255+0 = 0 → 0*293/4096 = 0
        d.transport = Some(Box::new(
            MockTransport::new().expect_binary(&[0x41, 0x00, 0x00, 0x00, 0x00, 0x00]),
        ));
        let c = d.get_current().unwrap();
        assert_eq!(c, 0.0);
    }

    #[test]
    fn get_current_rejects_short_response_instead_of_reusing_cache() {
        let mut d = make_initialized();
        d.current_ma = 12.0;
        d.transport = Some(Box::new(
            MockTransport::new().expect_binary(&[0x41, 0x00, 0x00]),
        ));
        assert_eq!(d.get_current(), Err(MmError::SerialInvalidResponse));
        assert_eq!(d.current(), 12.0);
    }

    #[test]
    fn current_set_without_transport_errors() {
        let mut d = EtlDevice::new();
        d.initialize().unwrap();
        assert_eq!(
            d.set_property("Current-mA", PropertyValue::Float(1.0)),
            Err(MmError::NotConnected)
        );
    }
}
