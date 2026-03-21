/// Nikon T-LUSU(2) TIRF shutter adapter.
///
/// Protocol (TX `\r`, RX `\n`):
///   `rVER\r`            → `aVER{version}\n`  (version query)
///   `cTSO{channel}\r`   → `oTSO\n`           (open shutter, channel 1-3)
///   `cTSC\r`            → `oTSC\n`           (close shutter)
///
/// Success prefix `o{CMD}`, error prefix `n{CMD}{code}`.
///
/// ---
///
/// Nikon Ti-TIRF shutter (TiTIRFShutter) adds multi-channel bitmask mode:
///   Mode 0 (single): `cTSO{channel}\r`
///   Mode 1 (multi):  `cTSD{bitmask}\r`  where bitmask = OR of (1<<(ch-1))
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Shutter};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

fn check_tirf_response(resp: &str, cmd: &str) -> MmResult<()> {
    let expected_ok = format!("o{}", cmd);
    if resp.starts_with(&expected_ok) {
        Ok(())
    } else if resp.starts_with('n') {
        Err(MmError::LocallyDefined(format!("Nikon TIRF error: '{}'", resp)))
    } else {
        Err(MmError::LocallyDefined(format!("Nikon TIRF unexpected response: '{}'", resp)))
    }
}

// ─── T-LUSU(2) single-channel TIRF shutter ─────────────────────────────────

pub struct NikonTiRFShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    open: bool,
    channel: u8,
}

impl NikonTiRFShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Channel", PropertyValue::Integer(1), false).unwrap();
        Self { props, transport: None, initialized: false, open: false, channel: 1 }
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
        self.call_transport(|t| Ok(t.send_recv(&c)?.trim().to_string()))
    }
}

impl Default for NikonTiRFShutter { fn default() -> Self { Self::new() } }

impl Device for NikonTiRFShutter {
    fn name(&self) -> &str { "NikonTiRFShutter" }
    fn description(&self) -> &str { "Nikon T-LUSU(2) TIRF shutter" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let resp = self.cmd("rVER")?;
        if !resp.starts_with("aVER") {
            return Err(MmError::LocallyDefined(format!("TIRF version query failed: '{}'", resp)));
        }
        // Close shutter on init
        let resp = self.cmd("cTSC")?;
        check_tirf_response(&resp, "TSC")?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Channel" {
            if let PropertyValue::Integer(ch) = val { self.channel = ch.clamp(1, 3) as u8; }
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

impl Shutter for NikonTiRFShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let resp = if open {
            let ch = self.channel;
            self.cmd(&format!("cTSO{}", ch))?
        } else {
            self.cmd("cTSC")?
        };
        let cmd_str = if open { format!("TSO{}", self.channel) } else { "TSC".to_string() };
        check_tirf_response(&resp, &cmd_str)?;
        self.open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.open) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::NotSupported)
    }
}

// ─── Ti-TIRF variant with single/multi-channel bitmask mode ─────────────────

pub struct NikonTiTiRFShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    open: bool,
    channel: u8,
    /// Mode 0 = single channel, Mode 1 = multi-channel bitmask
    mode: u8,
}

impl NikonTiTiRFShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Channel", PropertyValue::Integer(1), false).unwrap();
        props.define_property("Mode", PropertyValue::Integer(0), false).unwrap();
        Self { props, transport: None, initialized: false, open: false, channel: 1, mode: 0 }
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
        self.call_transport(|t| Ok(t.send_recv(&c)?.trim().to_string()))
    }
}

impl Default for NikonTiTiRFShutter { fn default() -> Self { Self::new() } }

impl Device for NikonTiTiRFShutter {
    fn name(&self) -> &str { "NikonTiTiRFShutter" }
    fn description(&self) -> &str { "Nikon Ti-TIRF shutter with multi-channel bitmask mode" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let resp = self.cmd("rVER")?;
        if !resp.starts_with("aVER") {
            return Err(MmError::LocallyDefined(format!("Ti-TIRF version query failed: '{}'", resp)));
        }
        let resp = self.cmd("cTSC")?;
        check_tirf_response(&resp, "TSC")?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Channel" {
            if let PropertyValue::Integer(ch) = val { self.channel = ch.clamp(1, 3) as u8; }
        } else if name == "Mode" {
            if let PropertyValue::Integer(m) = val { self.mode = m.clamp(0, 1) as u8; }
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

impl Shutter for NikonTiTiRFShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let (cmd_str, expected) = if open {
            if self.mode == 0 {
                let ch = self.channel;
                (format!("cTSO{}", ch), format!("TSO{}", ch))
            } else {
                let bitmask = 1u8 << (self.channel - 1);
                (format!("cTSD{}", bitmask), format!("TSD{}", bitmask))
            }
        } else {
            ("cTSC".to_string(), "TSC".to_string())
        };
        let resp = self.cmd(&cmd_str)?;
        check_tirf_response(&resp, &expected)?;
        self.open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.open) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::NotSupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    #[test]
    fn tirf_initialize_and_open() {
        let t = MockTransport::new().any("aVER1.0").any("oTSC").any("oTSO1").any("oTSC");
        let mut s = NikonTiRFShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn titirf_multi_channel_mode() {
        // Mode 1, channel 2 → bitmask = 2
        let t = MockTransport::new().any("aVER1.0").any("oTSC").any("oTSD2");
        let mut s = NikonTiTiRFShutter::new().with_transport(Box::new(t));
        s.set_property("Mode", PropertyValue::Integer(1)).unwrap();
        s.set_property("Channel", PropertyValue::Integer(2)).unwrap();
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn tirf_no_transport_error() {
        assert!(NikonTiRFShutter::new().initialize().is_err());
    }
}
