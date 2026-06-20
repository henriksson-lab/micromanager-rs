/// Thorlabs Elliptec Linear Stage (ELL17/ELL20).
///
/// Protocol (TX/RX `\r`):
///   `<ch>in\r`         → `<ch>IN<id><travel><pulses>`  device info
///   `<ch>gp\r`         → `<ch>PO<8-hex>`               get position in pulses
///   `<ch>ma<8-hex>\r`  → `<ch>MA<8-hex>`               move to absolute position
///   `<ch>pc\r`         → `<ch>PC`                       set as origin (zero)
///
/// Channel: single hex digit ('0'–'F').
/// Position encoding: signed 32-bit big-endian as 8 uppercase hex chars.
/// Conversion: position_um = (pulses * 1000) / pulses_per_mm.
/// The `info` response contains a 4-byte (8-hex) travel range and 4-byte pulses-per-mm.
///
/// Error: response last char 'N' or status code indicates fault.
use crate::adapters::elliptec::status;
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::sync::Mutex;

pub struct ElliptecStage {
    props: PropertyMap,
    transport: Option<Mutex<Box<dyn Transport>>>,
    initialized: bool,
    channel: char,
    pulses_per_mm: u32,
    travel_range_mm: u32,
}

impl ElliptecStage {
    pub fn new(channel: char) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Channel", PropertyValue::String(channel.to_string()), false)
            .unwrap();
        props
            .set_allowed_values(
                "Channel",
                &[
                    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F",
                ],
            )
            .unwrap();
        props
            .define_property("PulsesPerMm", PropertyValue::Integer(0), true)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            channel,
            pulses_per_mm: 10000, // default
            travel_range_mm: 0,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(Mutex::new(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_ref() {
            Some(t) => {
                let mut guard = t.lock().map_err(|_| {
                    MmError::LocallyDefined("Elliptec transport lock poisoned".into())
                })?;
                f(guard.as_mut())
            }
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let full = format!("{}{}\r", self.channel, command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(Self::remove_line_feed(r.trim()).to_string())
        })
    }

    fn remove_line_feed(answer: &str) -> &str {
        answer.strip_prefix('\n').unwrap_or(answer)
    }

    fn status_error(message: &str) -> Option<MmError> {
        status::status_error(message, "Elliptec")
    }

    /// Parse 8-char hex as signed i32 position in pulses.
    fn parse_pos_hex(hex: &str) -> i32 {
        u32::from_str_radix(hex.trim(), 16).unwrap_or(0) as i32
    }

    fn pulses_to_um(&self, pulses: i32) -> f64 {
        if self.pulses_per_mm == 0 {
            return 0.0;
        }
        (pulses as f64 * 1000.0) / self.pulses_per_mm as f64
    }

    fn um_to_pulses(&self, um: f64) -> i32 {
        ((um * self.pulses_per_mm as f64) / 1000.0).round() as i32
    }

    fn parse_info(&mut self, info: &str) -> MmResult<String> {
        if let Some(err) = Self::status_error(info) {
            return Err(err);
        }
        if info.len() < 33 || &info[1..3] != "IN" {
            return Err(MmError::SerialInvalidResponse);
        }
        let module = &info[3..5];
        if module != "14" && module != "11" {
            return Err(MmError::WrongDeviceType);
        }
        let id = info[3..18].to_string();
        self.travel_range_mm =
            u32::from_str_radix(&info[21..25], 16).map_err(|_| MmError::SerialInvalidResponse)?;
        self.pulses_per_mm =
            u32::from_str_radix(&info[25..33], 16).map_err(|_| MmError::SerialInvalidResponse)?;
        Ok(id)
    }

    fn parse_position_response(&self, gp: &str) -> MmResult<f64> {
        if let Some(err) = Self::status_error(gp) {
            return Err(err);
        }
        if gp.len() < 11 || &gp[1..3] != "PO" {
            return Err(MmError::SerialInvalidResponse);
        }
        let pulses = Self::parse_pos_hex(&gp[3..11]);
        Ok(self.pulses_to_um(pulses).round())
    }

    fn query_position_um(&self) -> MmResult<f64> {
        let gp = self.cmd("gp")?;
        self.parse_position_response(&gp)
    }

    fn query_busy(&self) -> MmResult<bool> {
        let gs = self.cmd("gs")?;
        if gs.len() < 5 || &gs[1..3] != "GS" {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(&gs[3..5] != "00")
    }
}

impl Default for ElliptecStage {
    fn default() -> Self {
        Self::new('0')
    }
}

impl Device for ElliptecStage {
    fn name(&self) -> &str {
        "Thorlabs ELL17/ELL20"
    }
    fn description(&self) -> &str {
        "Thorlabs Elliptec Linear Stage ELL17/ELL20"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let info = self.cmd("in")?;
        let id = self.parse_info(&info)?;
        if !self.props.has_property("ID") {
            self.props
                .define_property("ID", PropertyValue::String(id), true)
                .unwrap();
        } else if let Some(entry) = self.props.entry_mut("ID") {
            entry.value = PropertyValue::String(id);
        }
        self.props
            .entry_mut("PulsesPerMm")
            .map(|e| e.value = PropertyValue::Integer(self.pulses_per_mm as i64));
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
        match name {
            "Port" | "Channel" if self.initialized => Err(MmError::InvalidPropertyValue),
            "Channel" => {
                let s = val.as_str().to_string();
                let ch = s.chars().next().ok_or(MmError::InvalidPropertyValue)?;
                if !ch.is_ascii_hexdigit() {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.channel = ch.to_ascii_uppercase();
                self.props
                    .set(name, PropertyValue::String(self.channel.to_string()))
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
        DeviceType::Stage
    }
    fn busy(&self) -> bool {
        self.query_busy().unwrap_or(true)
    }
}

impl Stage for ElliptecStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let pulses = self.um_to_pulses(pos);
        let cmd = format!("ma{:08X}", pulses as u32);
        let response = self.cmd(&cmd)?;
        if let Some(err) = Self::status_error(&response) {
            return Err(err);
        }
        if response.len() >= 3 && &response[1..3] == "PO" {
            Ok(())
        } else {
            Err(MmError::SerialInvalidResponse)
        }
    }

    fn get_position_um(&self) -> MmResult<f64> {
        self.query_position_um()
    }

    fn home(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn stop(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let new_pos = self.query_position_um()? + d;
        self.set_position_um(new_pos)
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((0.0, 63500.0))
    }

    fn get_focus_direction(&self) -> FocusDirection {
        FocusDirection::Unknown
    }
    fn is_continuous_focus_drive(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        MockTransport::new().expect("0in\r", "0IN141234567822001000003C00002710")
    }

    #[test]
    fn initialize() {
        let mut s = ElliptecStage::new('0').with_transport(Box::new(make_init_transport()));
        s.initialize().unwrap();
        assert_eq!(s.pulses_per_mm, 10000);
        assert_eq!(s.travel_range_mm, 60);
        assert_eq!(
            s.get_property("ID").unwrap(),
            PropertyValue::String("141234567822001".into())
        );
    }

    #[test]
    fn set_position() {
        let t = make_init_transport()
            // move to 1000 µm = 1.0 mm = 10000 pulses = 0x00002710
            .expect("0ma00002710\r", "0PO00002710")
            .expect("0gp\r", "0PO00002710");
        let mut s = ElliptecStage::new('0').with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(1000.0).unwrap();
        assert!((s.get_position_um().unwrap() - 1000.0).abs() < 0.01);
    }

    #[test]
    fn home_and_stop_are_unsupported_like_upstream() {
        let t = make_init_transport();
        let mut s = ElliptecStage::new('0').with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.home().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(s.stop().unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn no_transport_error() {
        assert!(ElliptecStage::new('0').initialize().is_err());
    }

    #[test]
    fn channel_property_updates_before_init_and_is_locked_after_init() {
        let t = MockTransport::new().expect("Ain\r", "AIN141234567822001000003C00002710");
        let mut s = ElliptecStage::new('0').with_transport(Box::new(t));
        s.set_property("Channel", PropertyValue::String("A".into()))
            .unwrap();
        s.initialize().unwrap();
        assert_eq!(
            s.set_property("Channel", PropertyValue::String("1".into()))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            s.set_property("Port", PropertyValue::String("COM2".into()))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn busy_polls_gs() {
        let t = MockTransport::new()
            .expect("0in\r", "0IN141234567822001000003C00002710")
            .expect("0gs\r", "0GS09");
        let mut s = ElliptecStage::new('0').with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
    }

    #[test]
    fn rejects_wrong_stage_module_code() {
        let t = MockTransport::new().expect("0in\r", "0IN091234567822001000003C00002710");
        let mut s = ElliptecStage::new('0').with_transport(Box::new(t));
        assert_eq!(s.initialize().unwrap_err(), MmError::WrongDeviceType);
    }
}
