/// Spectral LMM5 laser combiner (serial RS-232 mode).
///
/// Protocol (serial mode): binary commands encoded as ASCII uppercase hex, terminated with `\r`.
///   Each command byte → 2 hex chars. Responses also as ASCII hex (parse back to bytes).
///
///   `08\r`                 → `08 <nm_h nm_l> ...`  detect laser lines (up to 8)
///   `14\r`                 → `14 <major> <minor>`   firmware version
///   `02\r`                 → `02 <state> xx xx`     get shutter state (state[1])
///   `0101\r`               → `01`                   set shutter open
///   `0100\r`               → `01`                   set shutter closed
///   `05 <line>\r`          → `05 <th> <tl>`         get transmission % × 10 for line
///   `04 <line> <th> <tl>\r`→ `04`                   set transmission
///
/// Wavelength: big-endian u16, units = 0.1 nm (divide by 10 for nm).
/// Transmission: big-endian i16, units = 0.1% (multiply by 10 for value, divide for read).
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Shutter};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

const MAX_LINES: usize = 8;

fn encode_cmd(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02X}", b)).collect()
}

fn decode_resp(hex: &str) -> Vec<u8> {
    hex.trim().as_bytes().chunks(2)
        .filter_map(|c| std::str::from_utf8(c).ok().and_then(|s| u8::from_str_radix(s, 16).ok()))
        .collect()
}

pub struct SpectralLmm5 {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    wavelengths_nm: Vec<u32>,      // nm for each detected line
    transmissions: Vec<f64>,       // 0.0–100.0 per line
    shutter_open: bool,
}

impl SpectralLmm5 {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("FirmwareVersion", PropertyValue::String(String::new()), true).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            wavelengths_nm: Vec::new(),
            transmissions: Vec::new(),
            shutter_open: false,
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

    fn cmd_bytes(&mut self, bytes: &[u8]) -> MmResult<Vec<u8>> {
        let hex = encode_cmd(bytes);
        let full = format!("{}\r", hex);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(decode_resp(r.trim()))
        })
    }

    pub fn num_lines(&self) -> usize { self.wavelengths_nm.len() }

    pub fn set_transmission(&mut self, line: usize, pct: f64) -> MmResult<()> {
        if line >= self.wavelengths_nm.len() {
            return Err(MmError::LocallyDefined(format!("Line {} out of range", line)));
        }
        let val = ((pct * 10.0).round() as i16).clamp(0, 1000) as u16;
        let [th, tl] = val.to_be_bytes();
        self.cmd_bytes(&[0x04, line as u8, th, tl])?;
        self.transmissions[line] = pct;
        let key = format!("Transmission_{}nm", self.wavelengths_nm[line]);
        self.props.entry_mut(&key).map(|e| e.value = PropertyValue::Float(pct));
        Ok(())
    }
}

impl Default for SpectralLmm5 { fn default() -> Self { Self::new() } }

impl Device for SpectralLmm5 {
    fn name(&self) -> &str { "SpectralLMM5" }
    fn description(&self) -> &str { "Spectral LMM5 Laser Combiner" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Get firmware version
        let fw = self.cmd_bytes(&[0x14])?;
        if fw.len() >= 3 {
            let ver = format!("{}.{}", fw[1], fw[2]);
            self.props.entry_mut("FirmwareVersion").map(|e| e.value = PropertyValue::String(ver));
        }
        // Detect laser lines
        let lines = self.cmd_bytes(&[0x08])?;
        // Response: [0x08, nm_h0, nm_l0, nm_h1, nm_l1, ...]
        let n = if lines.len() > 1 { (lines.len() - 1) / 2 } else { 0 };
        let n = n.min(MAX_LINES);
        self.wavelengths_nm.clear();
        self.transmissions.clear();
        for i in 0..n {
            let nm_raw = u16::from_be_bytes([lines[1 + i * 2], lines[2 + i * 2]]);
            let nm = (nm_raw / 10) as u32;
            self.wavelengths_nm.push(nm);
            self.transmissions.push(100.0);
            // Get current transmission
            let tr = self.cmd_bytes(&[0x05, i as u8])?;
            if tr.len() >= 3 {
                let val = u16::from_be_bytes([tr[1], tr[2]]);
                let pct = val as f64 / 10.0;
                self.transmissions[i] = pct;
            }
            // Define property per line
            let key = format!("Transmission_{}nm", nm);
            let _ = self.props.define_property(&key, PropertyValue::Float(self.transmissions[i]), false);
        }
        // Query shutter state
        let sh = self.cmd_bytes(&[0x02])?;
        if sh.len() >= 2 { self.shutter_open = sh[1] != 0; }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.set_open(false);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        // Handle per-line transmission properties
        for (i, &nm) in self.wavelengths_nm.iter().enumerate() {
            let key = format!("Transmission_{}nm", nm);
            if name == key {
                let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.set_transmission(i, pct)?;
                } else {
                    self.transmissions[i] = pct;
                    self.props.entry_mut(name).map(|e| e.value = PropertyValue::Float(pct));
                }
                return Ok(());
            }
        }
        self.props.set(name, val)
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Shutter }
    fn busy(&self) -> bool { false }
}

impl Shutter for SpectralLmm5 {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.cmd_bytes(&[0x01, if open { 1 } else { 0 }])?;
        self.shutter_open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.shutter_open) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            // firmware: major=1, minor=30 (0x1E)
            .expect("14\r", "14011E")
            // detect lines: 488nm=0x12C0/10=4800/10=480 (use 0x1310=4880→488nm)
            // Use 0x1310 = 4880 → 488nm exactly
            .expect("08\r", "08131007D0")   // 2 lines: 0x1310=488nm, 0x07D0=200?nm → 488, 20
            // Actually: 0x1310=4880/10=488, 0x07D0=2000/10=200
            // get transmission line 0: 0x03E8=1000/10=100.0%
            .expect("0500\r", "0503E8")
            // get transmission line 1
            .expect("0501\r", "050190")     // 0x0190=400/10=40.0%
            // get shutter state: closed
            .expect("02\r", "02000000")
    }

    #[test]
    fn initialize() {
        let mut dev = SpectralLmm5::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        assert_eq!(dev.num_lines(), 2);
        assert_eq!(dev.wavelengths_nm[0], 488);
        assert_eq!(dev.wavelengths_nm[1], 200);
        assert!((dev.transmissions[0] - 100.0).abs() < 0.1);
        assert!((dev.transmissions[1] - 40.0).abs() < 0.1);
        assert!(!dev.shutter_open);
    }

    #[test]
    fn open_close() {
        let t = make_init_transport()
            .expect("0101\r", "01")  // open
            .expect("0100\r", "01"); // close
        let mut dev = SpectralLmm5::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_transmission() {
        let t = make_init_transport()
            .expect("04003200\r", "04"); // set line 0 to 50% → 500 = 0x01F4... wait:
            // 50% * 10 = 500 = 0x01F4 → "040001F4"
        // Fix the test: 50% → 500 (0x01F4)
        let t = make_init_transport()
            .expect("040001F4\r", "04");
        let mut dev = SpectralLmm5::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_transmission(0, 50.0).unwrap();
        assert!((dev.transmissions[0] - 50.0).abs() < 0.1);
    }

    #[test]
    fn hex_encoding() {
        assert_eq!(encode_cmd(&[0x01, 0x01]), "0101");
        assert_eq!(decode_resp("14011E"), vec![0x14, 0x01, 0x1E]);
        assert_eq!(decode_resp("0101"), vec![0x01, 0x01]);
    }

    #[test]
    fn no_transport_error() { assert!(SpectralLmm5::new().initialize().is_err()); }
}
