/// Omicron PhoxX / LuxX / BrixX laser controller.
///
/// Protocol (TX/RX `\r`):
///   Commands: `?<CMD><DATA>\r`   Responses: `!<CMD><DATA>`
///   Prefix after `?` echoed as `!` in response; strip first 4 chars to get data.
///   Separator for multi-field responses: `\xA7` (§).
///
///   `?GOM\r`        → `!GOM<4-hex>`   get operating mode bitmask
///   `?SOM<4-hex>\r` → `!SOM...`       set operating mode (init: clear bit 13)
///   `?GFw\r`        → `!GFw§<ver>§<type>` firmware version and device type
///   `?GSI\r`        → `!GSI§<nm>§<mW>` wavelength and max power
///   `?GSN\r`        → `!GSN<serial>`  serial number
///   `?GLP\r`        → `!GLP<3-hex>`  laser power setpoint (0x000–0xFFF = 0–100%)
///   `?SLP<3-hex>\r` → `!SLP...`      set laser power
///   `?GAS\r`        → `!GAS<hex>`    status (bit 1 = laser on)
///   `?LOn\r`        → `!LOn...`      laser on
///   `?LOf\r`        → `!LOf...`      laser off
///
/// Power encoding: 12-bit (0x000–0xFFF maps to 0.0–100.0%).
/// Operating mode bitmask (bit 13 must be cleared during init).
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Shutter};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

const SEP: char = '\u{00A7}'; // § = 0xA7

pub struct OmicronLaser {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    power_pct: f64,
    laser_on: bool,
    max_power_mw: f64,
}

impl OmicronLaser {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("FirmwareVersion", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("SerialNumber",    PropertyValue::String(String::new()), true).unwrap();
        props.define_property("WavelengthNm",    PropertyValue::Integer(0), true).unwrap();
        props.define_property("MaxPower_mW",     PropertyValue::Float(0.0), true).unwrap();
        props.define_property("Power_Percent",   PropertyValue::Float(0.0), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            power_pct: 0.0,
            laser_on: false,
            max_power_mw: 0.0,
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

    /// Send command, strip `!CMD` echo prefix, return data portion.
    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        // Echo prefix: replace leading '?' with '!' → first 4 chars of response
        let echo = format!("!{}", &command[1..]);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            let trimmed = r.trim();
            let data = trimmed.strip_prefix(&echo).unwrap_or(trimmed);
            Ok(data.to_string())
        })
    }

    /// Parse 3-digit hex power value to percentage.
    fn hex_to_pct(hex: &str) -> f64 {
        let val = u32::from_str_radix(hex.trim(), 16).unwrap_or(0);
        (val as f64 / 4095.0) * 100.0
    }

    /// Encode percentage as 3-digit uppercase hex (0–FFF).
    fn pct_to_hex(pct: f64) -> String {
        let val = ((pct / 100.0) * 4095.0).round() as u32;
        let val = val.min(0xFFF);
        format!("{:03X}", val)
    }
}

impl Default for OmicronLaser { fn default() -> Self { Self::new() } }

impl Device for OmicronLaser {
    fn name(&self) -> &str { "OmicronLaser" }
    fn description(&self) -> &str { "Omicron PhoxX/LuxX/BrixX Laser" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Get and fix operating mode (clear bit 13)
        let mode_hex = self.cmd("?GOM")?;
        let mode = u32::from_str_radix(mode_hex.trim(), 16).unwrap_or(0);
        let mode = mode & !(1u32 << 13);
        self.cmd(&format!("?SOM{:04X}", mode))?;
        // Get firmware version and device type
        // Response after prefix: "§2.1.0§3"
        let fw = self.cmd("?GFw")?;
        let fw_fields: Vec<&str> = fw.split(SEP).filter(|s| !s.is_empty()).collect();
        let version = fw_fields.get(0).unwrap_or(&"").to_string();
        self.props.entry_mut("FirmwareVersion").map(|e| e.value = PropertyValue::String(version));
        // Get spec info: wavelength and max power
        // Response after prefix: "§488§50" — leading § before fields
        let gsi = self.cmd("?GSI")?;
        let fields: Vec<&str> = gsi.split(SEP).filter(|s| !s.is_empty()).collect();
        if let Some(nm_str) = fields.get(0) {
            let nm: i64 = nm_str.trim().parse().unwrap_or(0);
            self.props.entry_mut("WavelengthNm").map(|e| e.value = PropertyValue::Integer(nm));
        }
        if let Some(mw_str) = fields.get(1) {
            let mw: f64 = mw_str.trim().parse().unwrap_or(0.0);
            self.max_power_mw = mw;
            self.props.entry_mut("MaxPower_mW").map(|e| e.value = PropertyValue::Float(mw));
        }
        // Get serial number
        let sn = self.cmd("?GSN")?;
        self.props.entry_mut("SerialNumber").map(|e| e.value = PropertyValue::String(sn));
        // Query current power
        let lp = self.cmd("?GLP")?;
        self.power_pct = Self::hex_to_pct(lp.trim());
        self.props.entry_mut("Power_Percent").map(|e| e.value = PropertyValue::Float(self.power_pct));
        // Query laser state
        let gas = self.cmd("?GAS")?;
        let status = u32::from_str_radix(gas.trim(), 16).unwrap_or(0);
        self.laser_on = (status >> 1) & 1 == 1;
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
        match name {
            "Power_Percent" => Ok(PropertyValue::Float(self.power_pct)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Power_Percent" {
            let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            let pct = pct.clamp(0.0, 100.0);
            if self.initialized {
                self.cmd(&format!("?SLP{}", Self::pct_to_hex(pct)))?;
            }
            self.power_pct = pct;
            self.props.entry_mut(name).map(|e| e.value = PropertyValue::Float(pct));
            return Ok(());
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

impl Shutter for OmicronLaser {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        if open {
            self.cmd("?LOn")?;
        } else {
            self.cmd("?LOf")?;
        }
        self.laser_on = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.laser_on) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect("?GOM\r", "!GOM2010")         // mode with bit 13 set
            .expect("?SOM0010\r", "!SOM0010")      // cleared bit 13 (0x2010 & !0x2000 = 0x0010)
            .expect("?GFw\r", "!GFw\u{00A7}2.1.0\u{00A7}3")   // version + device type 3=PhoxX
            .expect("?GSI\r", "!GSI\u{00A7}488\u{00A7}50")     // 488nm, 50mW
            .expect("?GSN\r", "!GSN12345678")
            .expect("?GLP\r", "!GLP7FF")            // ~50%
            .expect("?GAS\r", "!GAS002")            // bit 1 = on
    }

    #[test]
    fn initialize() {
        let mut dev = OmicronLaser::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        assert!((dev.power_pct - 49.98).abs() < 0.1);
        assert!(dev.laser_on); // bit 1 set in 0x002
        assert_eq!(dev.max_power_mw, 50.0);
    }

    #[test]
    fn power_encoding() {
        assert_eq!(OmicronLaser::pct_to_hex(0.0),   "000");
        assert_eq!(OmicronLaser::pct_to_hex(100.0),  "FFF");
        assert_eq!(OmicronLaser::pct_to_hex(50.0),   "800");
        assert!((OmicronLaser::hex_to_pct("7FF") - 49.98).abs() < 0.1);
    }

    #[test]
    fn laser_on_off() {
        let t = make_init_transport()
            .expect("?LOn\r", "!LOn")
            .expect("?LOf\r", "!LOf");
        let mut dev = OmicronLaser::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_power() {
        let t = make_init_transport()
            .expect("?SLP800\r", "!SLP800");
        let mut dev = OmicronLaser::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Power_Percent", PropertyValue::Float(50.0)).unwrap();
        assert!((dev.power_pct - 50.0).abs() < 0.01);
    }

    #[test]
    fn no_transport_error() { assert!(OmicronLaser::new().initialize().is_err()); }
}
