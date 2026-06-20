use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

/// Number of channels in the E600.
const NUM_CHANNELS: usize = 3;

/// Channel IDs used in the binary protocol.
const CHANNEL_IDS: [u8; NUM_CHANNELS] = [0x01, 0x02, 0x03];

/// Yodn E600 LED light source controller.
///
/// Implements `Shutter`: open = lamp on (`[0x60, 0x00, 0x01]`),
/// closed = lamp off (`[0x60, 0x00, 0x00]`).
///
/// Uses binary `send_bytes`/`receive_bytes` transport methods.
pub struct YodnE600 {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    /// Intensity per channel (0-100).
    intensities: [u8; NUM_CHANNELS],
    /// Channel use-state (on/off per channel).
    channel_use: [bool; NUM_CHANNELS],
    /// Last read error code.
    error_code: u8,
}

impl YodnE600 {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Main Version", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Panel Version", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Error", PropertyValue::String("0x00".into()), true)
            .unwrap();
        props
            .define_property("Lamp", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("Lamp", &["0", "1"]).unwrap();

        // Per-channel properties
        for i in 1..=NUM_CHANNELS {
            let intensity_name = format!("Intensity CH{}", i);
            props
                .define_property(&intensity_name, PropertyValue::Integer(0), false)
                .unwrap();
            props
                .set_property_limits(&intensity_name, 0.0, 100.0)
                .unwrap();

            let temp_name = format!("Temperature CH{}(Deg.C)", i);
            props
                .define_property(&temp_name, PropertyValue::Integer(0), true)
                .unwrap();

            let use_name = format!("Use CH{}", i);
            props
                .define_property(&use_name, PropertyValue::Integer(0), false)
                .unwrap();
            props.set_allowed_values(&use_name, &["0", "1"]).unwrap();

            let time_name = format!("Use Time CH{}", i);
            props
                .define_property(&time_name, PropertyValue::Integer(0), true)
                .unwrap();
        }

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            intensities: [0u8; NUM_CHANNELS],
            channel_use: [false; NUM_CHANNELS],
            error_code: 0,
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

    /// Send raw bytes and receive a fixed-length response.
    fn send_recv_bytes(&mut self, cmd: &[u8], resp_len: usize) -> MmResult<Vec<u8>> {
        let cmd_owned = cmd.to_vec();
        self.call_transport(|t| {
            t.send_bytes(&cmd_owned)?;
            let resp = t.receive_bytes(resp_len)?;
            Ok(resp)
        })
    }

    /// Send raw bytes without expecting a specific response.
    fn send_bytes_only(&mut self, cmd: &[u8]) -> MmResult<()> {
        let cmd_owned = cmd.to_vec();
        self.call_transport(|t| {
            t.send_bytes(&cmd_owned)?;
            Ok(())
        })
    }

    /// Get lamp state. Returns 0 (off) or 1 (on).
    fn get_lamp_state(&mut self) -> MmResult<u8> {
        let resp = self.send_recv_bytes(&[0x57, 0x00], 3)?;
        Ok(resp.get(2).copied().unwrap_or(0))
    }

    fn get_version(&mut self, command: u8) -> MmResult<String> {
        let resp = self.send_recv_bytes(&[command], 128)?;
        Ok(String::from_utf8_lossy(&resp)
            .trim_matches(char::from(0))
            .to_string())
    }

    /// Get channel intensity (0-100).
    fn get_channel_intensity(&mut self, ch_idx: usize) -> MmResult<u8> {
        let ch_id = CHANNEL_IDS[ch_idx];
        let resp = self.send_recv_bytes(&[0x56, ch_id], 3)?;
        Ok(resp.get(2).copied().unwrap_or(0))
    }

    /// Get channel temperature in degrees C.
    fn get_channel_temperature(&mut self, ch_idx: usize) -> MmResult<u8> {
        let ch_id = CHANNEL_IDS[ch_idx];
        let resp = self.send_recv_bytes(&[0x55, ch_id], 3)?;
        Ok(resp.get(2).copied().unwrap_or(0))
    }

    /// Get channel use state (0=off, 1=on).
    fn get_channel_use_state(&mut self, ch_idx: usize) -> MmResult<u8> {
        let ch_id = CHANNEL_IDS[ch_idx];
        let resp = self.send_recv_bytes(&[0x57, ch_id], 3)?;
        Ok(resp.get(2).copied().unwrap_or(0))
    }

    /// Get channel use time in hours.
    fn get_channel_use_time(&mut self, ch_idx: usize) -> MmResult<u16> {
        let ch_id = CHANNEL_IDS[ch_idx];
        let resp = self.send_recv_bytes(&[0x53, ch_id], 4)?;
        let high = resp.get(2).copied().unwrap_or(0) as u16;
        let low = resp.get(3).copied().unwrap_or(0) as u16;
        Ok(high * 256 + low)
    }

    /// Get error code.
    fn get_error_code(&mut self) -> MmResult<u8> {
        let resp = self.send_recv_bytes(&[0x52], 2)?;
        Ok(resp.get(1).copied().unwrap_or(0))
    }

    fn error_code_str(code: u8) -> String {
        match code {
            0 => "0x00 (No Error)".to_string(),
            1 => "0x01 (Over Heat)".to_string(),
            _ => format!("0x{:02X}", code),
        }
    }

    fn temperature_transform(raw: u8) -> i64 {
        if raw >= 128 {
            raw as i64 - 256
        } else {
            raw as i64
        }
    }
}

impl Default for YodnE600 {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for YodnE600 {
    fn name(&self) -> &str {
        "YodnE600"
    }

    fn description(&self) -> &str {
        "YODN Hyper E600"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Send open/handshake command
        let _resp = self.send_recv_bytes(&[0x70], 1)?;

        let main_version = self.get_version(0x50)?;
        self.props
            .entry_mut("Main Version")
            .map(|e| e.value = PropertyValue::String(main_version));

        let mut panel_version = self.get_version(0x51)?;
        if !panel_version.is_empty() {
            panel_version.remove(0);
        }
        self.props
            .entry_mut("Panel Version")
            .map(|e| e.value = PropertyValue::String(panel_version));

        // Read lamp state
        let lamp = self.get_lamp_state()?;
        self.is_open = lamp != 0;
        self.props
            .entry_mut("Lamp")
            .map(|e| e.value = PropertyValue::Integer(lamp as i64));

        // Read per-channel data in the same order as E600Controller::Update().
        for i in 0..NUM_CHANNELS {
            let intensity = self.get_channel_intensity(i)?;
            self.intensities[i] = intensity;
            let intensity_name = format!("Intensity CH{}", i + 1);
            self.props
                .entry_mut(&intensity_name)
                .map(|e| e.value = PropertyValue::Integer(intensity as i64));
        }

        for i in 0..NUM_CHANNELS {
            let use_state = self.get_channel_use_state(i)?;
            self.channel_use[i] = use_state != 0;
            let use_name = format!("Use CH{}", i + 1);
            self.props
                .entry_mut(&use_name)
                .map(|e| e.value = PropertyValue::Integer(use_state as i64));
        }

        for i in 0..NUM_CHANNELS {
            let temp = self.get_channel_temperature(i)?;
            let temp_name = format!("Temperature CH{}(Deg.C)", i + 1);
            self.props
                .entry_mut(&temp_name)
                .map(|e| e.value = PropertyValue::Integer(Self::temperature_transform(temp)));
        }

        for i in 0..NUM_CHANNELS {
            let use_time = self.get_channel_use_time(i)?;
            let time_name = format!("Use Time CH{}", i + 1);
            self.props
                .entry_mut(&time_name)
                .map(|e| e.value = PropertyValue::Integer(use_time as i64));
        }

        // Read error code
        let err = self.get_error_code()?;
        self.error_code = err;
        self.props
            .entry_mut("Error")
            .map(|e| e.value = PropertyValue::String(Self::error_code_str(err)));

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            // Send close command
            let _ = self.send_bytes_only(&[0x75]);
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Lamp" {
            let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            if self.initialized {
                if v == 1 {
                    self.send_bytes_only(&[0x60, 0x00, 0x01])?;
                    self.is_open = true;
                } else {
                    self.send_bytes_only(&[0x60, 0x00, 0x00])?;
                    self.is_open = false;
                }
            }
            return self.props.set(name, PropertyValue::Integer(v));
        }

        // Handle intensity: "Intensity CH<N>"
        if let Some(rest) = name.strip_prefix("Intensity CH") {
            if let Ok(n) = rest.parse::<usize>() {
                if let Some(ch) = n.checked_sub(1).filter(|&ch| ch < NUM_CHANNELS) {
                    let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u8;
                    if self.initialized {
                        // Set intensity command.
                        let ch_id = CHANNEL_IDS[ch];
                        let _ = self.send_bytes_only(&[0x61, ch_id, v]);
                    }
                    self.intensities[ch] = v;
                    return self.props.set(name, PropertyValue::Integer(v as i64));
                }
            }
        }

        // Handle use state: "Use CH<N>"
        if let Some(rest) = name.strip_prefix("Use CH") {
            if let Ok(n) = rest.parse::<usize>() {
                if let Some(ch) = n.checked_sub(1).filter(|&ch| ch < NUM_CHANNELS) {
                    let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                    let on = v != 0;
                    if self.initialized {
                        let ch_id = CHANNEL_IDS[ch];
                        let _ = self.send_bytes_only(&[0x60, ch_id, if on { 1 } else { 0 }]);
                    }
                    self.channel_use[ch] = on;
                    return self.props.set(name, PropertyValue::Integer(v));
                }
            }
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

impl Shutter for YodnE600 {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        if open {
            self.send_bytes_only(&[0x60, 0x00, 0x01])?;
        } else {
            self.send_bytes_only(&[0x60, 0x00, 0x00])?;
        }
        self.is_open = open;
        self.props
            .entry_mut("Lamp")
            .map(|e| e.value = PropertyValue::Integer(if open { 1 } else { 0 }));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.is_open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            // open handshake — response: [0x70] (1 byte)
            .expect_binary(b"\x70")
            // main and panel versions
            .expect_binary(b"M1.0")
            .expect_binary(b"Q2.0")
            // get lamp state — response: 3 bytes, byte[2]=0 (off)
            .expect_binary(b"\x57\x00\x00")
            // get channel intensities — 3 bytes each, byte[2]=intensity
            .expect_binary(b"\x56\x01\x00")
            .expect_binary(b"\x56\x02\x00")
            .expect_binary(b"\x56\x03\x00")
            // get channel use states — 3 bytes each, byte[2]=state
            .expect_binary(b"\x57\x01\x00")
            .expect_binary(b"\x57\x02\x00")
            .expect_binary(b"\x57\x03\x00")
            // get channel temperatures — 3 bytes each
            .expect_binary(b"\x55\x01\x19")
            .expect_binary(b"\x55\x02\x19")
            .expect_binary(b"\x55\x03\x19")
            // get channel use times — 4 bytes each
            .expect_binary(b"\x53\x01\x00\x00")
            .expect_binary(b"\x53\x02\x00\x00")
            .expect_binary(b"\x53\x03\x00\x00")
            // get error code — 2 bytes, byte[1]=0 (no error)
            .expect_binary(b"\x52\x00")
    }

    #[test]
    fn initialize_no_error() {
        let mut dev = YodnE600::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(
            dev.get_property("Error").unwrap(),
            PropertyValue::String("0x00 (No Error)".into())
        );
        assert_eq!(
            dev.get_property("Panel Version").unwrap(),
            PropertyValue::String("2.0".into())
        );
    }

    #[test]
    fn open_close_lamp() {
        // The lamp on/off commands use send_bytes_only (no response expected)
        let t = make_transport();
        let mut dev = YodnE600::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() {
        let mut dev = YodnE600::new();
        assert!(dev.initialize().is_err());
    }

    #[test]
    fn fire_is_unsupported_like_upstream() {
        let mut dev = YodnE600::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert_eq!(dev.fire(1.0).unwrap_err(), MmError::UnsupportedCommand);
    }
}
