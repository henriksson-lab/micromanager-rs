/// BlueboxOptics niji multi-channel LED illuminator.
///
/// Protocol (TX `\r\n`, binary sync at init):
///   Init: send 12-byte binary sync `\x4f\xff\x50` × 4 (no terminator).
///   `?\r\n`           → multi-line status including "Firmware,<ver>,"
///   `D,<ch>,<0|1>\r\n` → "D,<ch>,<0|1>,"     enable/disable channel (1–7)
///   `d,<ch>,<0-100>\r\n` → "d,<ch>,<intensity>," set channel intensity
///   `r\r\n`           → "R,<temp>,...,"       LED readout
///
/// Channels correspond to wavelengths (1-indexed):
///   1=395nm, 2=445nm, 3=470nm, 4=515nm, 5=575nm, 6=630nm, 7=745nm
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Shutter};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

const WAVELENGTHS: [u32; 7] = [395, 445, 470, 515, 575, 630, 745];
const NUM_CHANNELS: usize = 7;

pub struct NijiController {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    channel_enabled: [bool; NUM_CHANNELS],
    channel_intensity: [u32; NUM_CHANNELS], // 0–100
    open: bool,
}

impl NijiController {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("FirmwareVersion", PropertyValue::String(String::new()), true).unwrap();
        for (i, &nm) in WAVELENGTHS.iter().enumerate() {
            let ch = i + 1;
            props.define_property(
                &format!("Channel{}_{}nm_Enable", ch, nm),
                PropertyValue::String("0".into()), false).unwrap();
            props.set_allowed_values(&format!("Channel{}_{}nm_Enable", ch, nm), &["0", "1"]).unwrap();
            props.define_property(
                &format!("Channel{}_{}nm_Intensity", ch, nm),
                PropertyValue::Integer(100), false).unwrap();
        }
        Self {
            props,
            transport: None,
            initialized: false,
            channel_enabled: [false; NUM_CHANNELS],
            channel_intensity: [100; NUM_CHANNELS],
            open: false,
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

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let full = format!("{}\r\n", command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    fn set_channel(&mut self, ch: usize, enable: bool) -> MmResult<()> {
        let cmd = format!("D,{},{}", ch + 1, if enable { 1 } else { 0 });
        self.cmd(&cmd)?;
        self.channel_enabled[ch] = enable;
        Ok(())
    }

    fn set_channel_intensity(&mut self, ch: usize, intensity: u32) -> MmResult<()> {
        let cmd = format!("d,{},{}", ch + 1, intensity);
        self.cmd(&cmd)?;
        self.channel_intensity[ch] = intensity;
        Ok(())
    }

    fn illuminate(&mut self, open: bool) -> MmResult<()> {
        for ch in 0..NUM_CHANNELS {
            let state = open && self.channel_enabled[ch];
            let cmd = format!("D,{},{}", ch + 1, if state { 1 } else { 0 });
            self.cmd(&cmd)?;
        }
        self.open = open;
        Ok(())
    }
}

impl Default for NijiController { fn default() -> Self { Self::new() } }

impl Device for NijiController {
    fn name(&self) -> &str { "NijiController" }
    fn description(&self) -> &str { "BlueboxOptics niji LED Illuminator" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Binary synchronization sequence
        let sync: [u8; 12] = [0x4f, 0xff, 0x50, 0x4f, 0xff, 0x50, 0x4f, 0xff, 0x50, 0x4f, 0xff, 0x50];
        self.call_transport(|t| t.send_bytes(&sync))?;
        // Query status to get firmware version
        let status = self.cmd("?")?;
        for line in status.lines() {
            if let Some(ver) = line.strip_prefix("Firmware,") {
                let ver = ver.trim_end_matches(',').to_string();
                self.props.entry_mut("FirmwareVersion").map(|e| e.value = PropertyValue::String(ver));
                break;
            }
        }
        // Set all channels off at init
        for ch in 0..NUM_CHANNELS {
            let cmd = format!("D,{},0", ch + 1);
            self.cmd(&cmd)?;
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.illuminate(false);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        // Handle channel enable/intensity properties
        for (i, &nm) in WAVELENGTHS.iter().enumerate() {
            let ch = i + 1;
            let enable_key = format!("Channel{}_{}nm_Enable", ch, nm);
            let intensity_key = format!("Channel{}_{}nm_Intensity", ch, nm);
            if name == enable_key {
                let v = val.as_str() == "1";
                if self.initialized {
                    self.set_channel(i, v)?;
                } else {
                    self.channel_enabled[i] = v;
                }
                self.props.entry_mut(name).map(|e| e.value = PropertyValue::String(if v { "1" } else { "0" }.into()));
                return Ok(());
            }
            if name == intensity_key {
                let pct = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32;
                if self.initialized {
                    self.set_channel_intensity(i, pct)?;
                } else {
                    self.channel_intensity[i] = pct;
                }
                self.props.entry_mut(name).map(|e| e.value = PropertyValue::Integer(pct as i64));
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

impl Shutter for NijiController {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.illuminate(open)
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.open) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        let mut t = MockTransport::new()
            .expect_binary(b"") // binary sync (any)
            .expect("?\r\n", "Firmware,1.0.3,\nStatus,0,")
            // 7 × D,ch,0
            .expect("D,1,0\r\n", "D,1,0,")
            .expect("D,2,0\r\n", "D,2,0,")
            .expect("D,3,0\r\n", "D,3,0,")
            .expect("D,4,0\r\n", "D,4,0,")
            .expect("D,5,0\r\n", "D,5,0,")
            .expect("D,6,0\r\n", "D,6,0,")
            .expect("D,7,0\r\n", "D,7,0,");
        t
    }

    #[test]
    fn initialize() {
        let mut dev = NijiController::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn open_close() {
        // Enable channel 1 first (not initialized, so no transport needed for that)
        let t = make_init_transport()
            .expect("D,1,1\r\n", "D,1,1,")
            .expect("D,2,0\r\n", "D,2,0,")
            .expect("D,3,0\r\n", "D,3,0,")
            .expect("D,4,0\r\n", "D,4,0,")
            .expect("D,5,0\r\n", "D,5,0,")
            .expect("D,6,0\r\n", "D,6,0,")
            .expect("D,7,0\r\n", "D,7,0,")
            // close: all 0
            .expect("D,1,0\r\n", "D,1,0,")
            .expect("D,2,0\r\n", "D,2,0,")
            .expect("D,3,0\r\n", "D,3,0,")
            .expect("D,4,0\r\n", "D,4,0,")
            .expect("D,5,0\r\n", "D,5,0,")
            .expect("D,6,0\r\n", "D,6,0,")
            .expect("D,7,0\r\n", "D,7,0,");
        let mut dev = NijiController::new().with_transport(Box::new(t));
        dev.channel_enabled[0] = true; // enable ch1 before init
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() { assert!(NijiController::new().initialize().is_err()); }
}
