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
/// Initialisation handshake:
///   Send "Start" via serial (ASCII, no CRC).
///
/// Current range: -293 mA to +293 mA.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Generic};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

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
    [0x41, 0x77, val_hi, val_lo, (crc & 0xFF) as u8, (crc >> 8) as u8]
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
            .define_property("Current-mA", PropertyValue::Float(0.0), false)
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
        let clamped = current_ma
            .max(self.min_current_ma)
            .min(self.max_current_ma);
        let cmd = build_set_current_cmd(clamped);
        self.call_transport(|t| t.send_bytes(&cmd))?;
        self.current_ma = clamped;
        Ok(())
    }

    /// Read back the current from the device.
    /// Sends the hard-coded get-current command and parses the 6-byte response.
    pub fn get_current(&mut self) -> MmResult<f64> {
        let get_cmd: [u8; 6] = [0x41, 0x72, 0x00, 0x00, 0xB4, 0x27];
        self.call_transport(|t| t.send_bytes(&get_cmd))?;
        let resp = self.call_transport(|t| t.receive_bytes(6))?;
        if resp.len() < 3 {
            return Ok(self.current_ma);
        }
        // Decode as per C++ empirical formula:
        //   i1 = signed(resp[1]), i2 = unsigned(resp[2])
        //   current = (i1 * 255 + i2) * 293 / 4096
        let i1 = resp[1] as i8 as i32;
        let i2 = resp[2] as i32;
        let current = (i1 * 255 + i2) as f64 * 293.0 / 4096.0;
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
        "Optotune Electrically Tunable Lens"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Send the "Start" initialisation handshake (ASCII, via send())
        self.call_transport(|t| t.send("Start"))?;
        // Set current to 0 mA at init
        self.set_current(0.0)?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            // Set current to 0 on shutdown
            let _ = self.set_current(0.0);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
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
    use micromanager::transport::MockTransport;

    fn make_initialized() -> EtlDevice {
        // init: send "Start" (string), then set_current(0.0) sends 6 bytes
        let t = MockTransport::new(); // send_bytes records to received_bytes, no response needed
        let mut d = EtlDevice::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d
    }

    #[test]
    fn initialize_succeeds() {
        let d = make_initialized();
        assert!(d.initialized);
        assert_eq!(d.current(), 0.0);
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
    fn current_clamped_to_range() {
        let mut d = make_initialized();
        d.transport = Some(Box::new(MockTransport::new()));
        d.set_current(999.0).unwrap();
        assert_eq!(d.current(), 293.0);
        d.transport = Some(Box::new(MockTransport::new()));
        d.set_current(-999.0).unwrap();
        assert_eq!(d.current(), -293.0);
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
    fn no_transport_error() {
        assert!(EtlDevice::new().initialize().is_err());
    }
}
