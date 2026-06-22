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
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

fn check_tirf_response(resp: &str, cmd: &str) -> MmResult<()> {
    let expected_ok = format!("o{}", cmd);
    if resp.starts_with(&expected_ok) {
        Ok(())
    } else if resp.starts_with('n') {
        Err(MmError::LocallyDefined(format!(
            "Nikon TIRF error: '{}'",
            resp
        )))
    } else {
        Err(MmError::LocallyDefined(format!(
            "Nikon TIRF unexpected response: '{}'",
            resp
        )))
    }
}

fn parse_version(resp: &str) -> String {
    resp.get(4..)
        .unwrap_or("")
        .chars()
        .take(5)
        .collect::<String>()
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
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            open: false,
            channel: 1,
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

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let c = format!("{}\r", command);
        self.call_transport(|t| Ok(t.send_recv(&c)?.trim().to_string()))
    }

    fn define_runtime_properties(&mut self, version: String) -> MmResult<()> {
        if !self.props.has_property("State") {
            self.props
                .define_property("State", PropertyValue::Integer(0), false)?;
            self.props.set_allowed_values("State", &["0", "1"])?;
        }
        if !self.props.has_property("Channel") {
            self.props
                .define_property("Channel", PropertyValue::Integer(1), false)?;
            self.props.set_allowed_values("Channel", &["1", "2", "3"])?;
        }
        if !self.props.has_property("Version") {
            self.props
                .define_property("Version", PropertyValue::String(version), true)?;
        } else {
            self.props
                .entry_mut("Version")
                .map(|e| e.value = PropertyValue::String(version));
        }
        Ok(())
    }
}

impl Default for NikonTiRFShutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for NikonTiRFShutter {
    fn name(&self) -> &str {
        "TIRFShutter"
    }
    fn description(&self) -> &str {
        "Nikon TIRFS Shutter Controller T-LUSU driver adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let resp = self.cmd("rVER")?;
        if !resp.starts_with("aVER") {
            return Err(MmError::LocallyDefined(format!(
                "TIRF version query failed: '{}'",
                resp
            )));
        }
        let version = parse_version(&resp);
        self.define_runtime_properties(version)?;
        // Close shutter on init
        let resp = self.cmd("cTSC")?;
        check_tirf_response(&resp, "TSC")?;
        self.open = false;
        self.props
            .entry_mut("State")
            .map(|e| e.value = PropertyValue::Integer(0));
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Port" && self.initialized {
            return Err(MmError::InvalidPropertyValue);
        }
        if name == "Channel" {
            if !self.props.has_property(name) {
                return Err(MmError::UnknownLabel(name.to_string()));
            }
            let ch = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            if !(1..=3).contains(&ch) {
                return Err(MmError::InvalidPropertyValue);
            }
            self.props.set(name, val)?;
            self.channel = ch as u8;
            return Ok(());
        } else if name == "State" {
            if !self.props.has_property(name) {
                return Err(MmError::UnknownLabel(name.to_string()));
            }
            let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            if !(0..=1).contains(&state) {
                return Err(MmError::InvalidPropertyValue);
            }
            if self.initialized {
                self.set_open(state == 1)?;
            } else {
                self.props.set(name, val)?;
                self.open = state == 1;
            }
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
        DeviceType::Shutter
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Shutter for NikonTiRFShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let resp = if open {
            let ch = self.channel;
            self.cmd(&format!("cTSO{}", ch))?
        } else {
            self.cmd("cTSC")?
        };
        let cmd_str = if open {
            "TSO".to_string()
        } else {
            "TSC".to_string()
        };
        check_tirf_response(&resp, &cmd_str)?;
        self.open = open;
        self.props
            .entry_mut("State")
            .map(|e| e.value = PropertyValue::Integer(if open { 1 } else { 0 }));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.open)
    }

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
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            open: false,
            channel: 1,
            mode: 0,
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

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let c = format!("{}\r", command);
        self.call_transport(|t| Ok(t.send_recv(&c)?.trim().to_string()))
    }

    fn define_runtime_properties(&mut self, version: String) -> MmResult<()> {
        if !self.props.has_property("State") {
            self.props
                .define_property("State", PropertyValue::Integer(0), false)?;
            self.props.set_allowed_values("State", &["0", "1"])?;
        }
        if !self.props.has_property("Channel") {
            self.props
                .define_property("Channel", PropertyValue::Integer(1), false)?;
            self.props.set_allowed_values("Channel", &["1", "2", "3"])?;
        }
        if !self.props.has_property("Version") {
            self.props
                .define_property("Version", PropertyValue::String(version), true)?;
        } else {
            self.props
                .entry_mut("Version")
                .map(|e| e.value = PropertyValue::String(version));
        }
        Ok(())
    }
}

impl Default for NikonTiTiRFShutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for NikonTiTiRFShutter {
    fn name(&self) -> &str {
        "TiTIRFShutter"
    }
    fn description(&self) -> &str {
        "Nikon Ti-TIRF Shutter Controller"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let resp = self.cmd("rVER")?;
        if !resp.starts_with("aVER") {
            return Err(MmError::LocallyDefined(format!(
                "Ti-TIRF version query failed: '{}'",
                resp
            )));
        }
        let version = parse_version(&resp);
        let resp = self.cmd("rTEX")?;
        if !resp.starts_with("aTEX") {
            return Err(MmError::LocallyDefined(format!(
                "Ti-TIRF mode query failed: '{}'",
                resp
            )));
        }
        let mode = resp
            .get(4..)
            .unwrap_or("")
            .trim()
            .parse::<u8>()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        if mode > 1 {
            return Err(MmError::InvalidPropertyValue);
        }
        self.mode = mode;
        self.define_runtime_properties(version)?;
        let resp = self.cmd("cTSC")?;
        check_tirf_response(&resp, "TSC")?;
        self.open = false;
        self.props
            .entry_mut("State")
            .map(|e| e.value = PropertyValue::Integer(0));
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Port" && self.initialized {
            return Err(MmError::InvalidPropertyValue);
        }
        if name == "Channel" {
            if !self.props.has_property(name) {
                return Err(MmError::UnknownLabel(name.to_string()));
            }
            let ch = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            if !(1..=3).contains(&ch) {
                return Err(MmError::InvalidPropertyValue);
            }
            self.props.set(name, val)?;
            self.channel = ch as u8;
            return Ok(());
        } else if name == "State" {
            if !self.props.has_property(name) {
                return Err(MmError::UnknownLabel(name.to_string()));
            }
            let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            if !(0..=1).contains(&state) {
                return Err(MmError::InvalidPropertyValue);
            }
            if self.initialized {
                self.set_open(state == 1)?;
            } else {
                self.props.set(name, val)?;
                self.open = state == 1;
            }
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
        DeviceType::Shutter
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Shutter for NikonTiTiRFShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let (cmd_str, expected) = if open {
            if self.mode == 0 {
                let ch = self.channel;
                (format!("cTSO{}", ch), "TSO".to_string())
            } else {
                let bitmask = 1u8 << (self.channel - 1);
                (format!("cTSD{}", bitmask), "TSD".to_string())
            }
        } else {
            ("cTSC".to_string(), "TSC".to_string())
        };
        let resp = self.cmd(&cmd_str)?;
        check_tirf_response(&resp, &expected)?;
        self.open = open;
        self.props
            .entry_mut("State")
            .map(|e| e.value = PropertyValue::Integer(if open { 1 } else { 0 }));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::NotSupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn tirf_initialize_and_open() {
        let t = MockTransport::new()
            .any("aVER1.0")
            .any("oTSC")
            .any("oTSO")
            .any("oTSC");
        let mut s = NikonTiRFShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        assert_eq!(s.get_property("State").unwrap(), PropertyValue::Integer(1));
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
        assert_eq!(s.get_property("State").unwrap(), PropertyValue::Integer(0));
    }

    #[test]
    fn titirf_multi_channel_mode() {
        // Live mode 1, channel 2 -> bitmask = 2
        let t = MockTransport::new()
            .any("aVER1.0")
            .any("aTEX1")
            .any("oTSC")
            .any("oTSD");
        let mut s = NikonTiTiRFShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("Channel", PropertyValue::Integer(2))
            .unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn tirf_no_transport_error() {
        assert!(NikonTiRFShutter::new().initialize().is_err());
    }

    #[test]
    fn tirf_bad_channel_type_does_not_update_cache() {
        let t = MockTransport::new()
            .expect("rVER\r", "aVER1.0")
            .expect("cTSC\r", "oTSC");
        let mut s = NikonTiRFShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.set_property("Channel", PropertyValue::String("bad".into())),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(s.channel, 1);
    }

    #[test]
    fn titirf_bad_mode_type_does_not_update_cache() {
        let mut s = NikonTiTiRFShutter::new();
        assert_eq!(
            s.set_property("Mode", PropertyValue::String("bad".into())),
            Err(MmError::UnknownLabel("Mode".into()))
        );
        assert_eq!(s.mode, 0);
    }

    #[test]
    fn tirf_identity_matches_upstream() {
        let s = NikonTiRFShutter::new();
        assert_eq!(s.name(), "TIRFShutter");
        assert_eq!(
            s.description(),
            "Nikon TIRFS Shutter Controller T-LUSU driver adapter"
        );
        let s = NikonTiTiRFShutter::new();
        assert_eq!(s.name(), "TiTIRFShutter");
        assert_eq!(s.description(), "Nikon Ti-TIRF Shutter Controller");
    }

    #[test]
    fn tirf_rejects_invalid_channel_without_clamping() {
        let t = MockTransport::new()
            .expect("rVER\r", "aVER1.0")
            .expect("cTSC\r", "oTSC");
        let mut s = NikonTiRFShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.set_property("Channel", PropertyValue::Integer(9)),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(s.channel, 1);
        assert_eq!(
            s.get_property("Channel").unwrap(),
            PropertyValue::Integer(1)
        );
    }

    #[test]
    fn tirf_state_property_is_ack_gated() {
        let t = MockTransport::new()
            .expect("rVER\r", "aVER1.0")
            .expect("cTSC\r", "oTSC")
            .expect("cTSO1\r", "bad");
        let mut s = NikonTiRFShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.set_property("State", PropertyValue::Integer(1)),
            Err(MmError::LocallyDefined(
                "Nikon TIRF unexpected response: 'bad'".into()
            ))
        );
        assert!(!s.get_open().unwrap());
        assert_eq!(s.get_property("State").unwrap(), PropertyValue::Integer(0));
    }

    #[test]
    fn titirf_initializes_mode_from_live_rtex_and_rejects_port_change() {
        let t = MockTransport::new()
            .expect("rVER\r", "aVER1.0")
            .expect("rTEX\r", "aTEX1")
            .expect("cTSC\r", "oTSC");
        let mut s = NikonTiTiRFShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.mode, 1);
        assert!(!s.has_property("Mode"));
        assert_eq!(
            s.set_property("Port", PropertyValue::String("COM2".into())),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            s.get_property("Port").unwrap(),
            PropertyValue::String("Undefined".into())
        );
    }

    #[test]
    fn tirf_runtime_properties_are_created_after_successful_initialize() {
        let t = MockTransport::new()
            .expect("rVER\r", "aVER1.0")
            .expect("cTSC\r", "oTSC");
        let mut s = NikonTiRFShutter::new().with_transport(Box::new(t));
        assert_eq!(s.property_names(), vec!["Port"]);
        assert_eq!(
            s.set_property("Channel", PropertyValue::Integer(2)),
            Err(MmError::UnknownLabel("Channel".into()))
        );

        s.initialize().unwrap();

        assert_eq!(
            s.property_names(),
            vec!["Port", "State", "Channel", "Version"]
        );
        assert_eq!(
            s.get_property("Version").unwrap(),
            PropertyValue::String("1.0".into())
        );
    }

    #[test]
    fn tirf_failed_initialize_leaves_runtime_properties_absent() {
        let t = MockTransport::new().expect("rVER\r", "nVER1");
        let mut s = NikonTiRFShutter::new().with_transport(Box::new(t));

        assert!(s.initialize().is_err());

        assert_eq!(s.property_names(), vec!["Port"]);
        assert!(!s.has_property("State"));
        assert!(!s.has_property("Channel"));
        assert!(!s.has_property("Version"));
    }
}
