/// Lumen Dynamics PrecisExcite LED illuminator.
///
/// Protocol (TX `\r`, RX `\n`):
///   `LAMS\r`         → repeated "LAM<ch> <name>\n" lines until empty line
///   `C<ch>N\r`       → turn channel on  (ch = A–D)
///   `C<ch>F\r`       → turn channel off
///   `C<ch>I<0-100>\r`→ set channel intensity (0–100)
///   `STATUS\r`       → "S<A><B><C><D>" where each char is '0' or '1' (on/off)
///
/// Implemented as a Shutter over channel A only; full multi-channel access via
/// properties "ChannelA_Intensity" … "ChannelD_Intensity".
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Shutter};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

const CHANNELS: [char; 4] = ['A', 'B', 'C', 'D'];

pub struct PrecisExcite {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    /// Is channel A (shutter) open?
    is_open: bool,
    /// Intensity per channel [0–100].
    intensity: [u8; 4],
}

impl PrecisExcite {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        for ch in CHANNELS {
            let name = format!("Channel{}_Intensity", ch);
            props.define_property(&name, PropertyValue::Integer(0), false).unwrap();
        }
        Self { props, transport: None, initialized: false, is_open: false, intensity: [0; 4] }
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
        let c = format!("{}\r", command);
        self.call_transport(|t| { let r = t.send_recv(&c)?; Ok(r.trim().to_string()) })
    }

    fn set_channel_intensity(&mut self, ch_idx: usize, level: u8) -> MmResult<()> {
        let ch = CHANNELS[ch_idx];
        let level = level.min(100);
        self.cmd(&format!("C{}I{}", ch, level))?;
        self.intensity[ch_idx] = level;
        let name = format!("Channel{}_Intensity", ch);
        self.props.entry_mut(&name).map(|e| e.value = PropertyValue::Integer(level as i64));
        Ok(())
    }
}

impl Default for PrecisExcite { fn default() -> Self { Self::new() } }

impl Device for PrecisExcite {
    fn name(&self) -> &str { "PrecisExcite" }
    fn description(&self) -> &str { "Lumen Dynamics PrecisExcite LED illuminator" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Turn all channels off and set intensity to 0
        for ch in CHANNELS {
            let _ = self.cmd(&format!("C{}F", ch));
            let _ = self.cmd(&format!("C{}I0", ch));
        }
        self.is_open = false;
        self.intensity = [0; 4];
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            for ch in CHANNELS { let _ = self.cmd(&format!("C{}F", ch)); }
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        for (i, ch) in CHANNELS.iter().enumerate() {
            let pname = format!("Channel{}_Intensity", ch);
            if name == pname {
                let level = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u8;
                if self.initialized { self.set_channel_intensity(i, level)?; } else {
                    self.intensity[i] = level.min(100);
                    self.props.entry_mut(name).map(|e| e.value = val.clone());
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

impl Shutter for PrecisExcite {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = format!("CA{}", if open { 'N' } else { 'F' });
        self.cmd(&cmd)?;
        self.is_open = open;
        Ok(())
    }
    fn get_open(&self) -> MmResult<bool> { Ok(self.is_open) }
    fn fire(&mut self, _dt: f64) -> MmResult<()> { self.set_open(true) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .any("OK") // CAF
            .any("OK") // CAI0
            .any("OK") // CBF
            .any("OK") // CBI0
            .any("OK") // CCF
            .any("OK") // CCI0
            .any("OK") // CDF
            .any("OK") // CDI0
    }

    #[test]
    fn initialize() {
        let mut dev = PrecisExcite::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn open_close() {
        let t = make_init_transport().any("OK").any("OK");
        let mut dev = PrecisExcite::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_intensity() {
        let t = make_init_transport().any("OK");
        let mut dev = PrecisExcite::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("ChannelA_Intensity", PropertyValue::Integer(75)).unwrap();
        assert_eq!(dev.intensity[0], 75);
    }

    #[test]
    fn no_transport_error() { assert!(PrecisExcite::new().initialize().is_err()); }
}
