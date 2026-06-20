/// Thorlabs Elliptec slider (ELL6 / ELL9) state device.
///
/// Protocol (TX/RX `\r`):
///   `<ch>in\r`    → device info (same as stage)
///   `<ch>gp\r`    → `<ch>PO<8-hex>`   get position (0x00000000 or non-zero)
///   `<ch>mofb\r`  → `<ch>PO...`       move forward  (to position 1)
///   `<ch>mobk\r`  → `<ch>PO...`       move backward (to position 0)
///
use crate::adapters::elliptec::status;
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElliptecSliderModel {
    Ell6,
    Ell9,
    Ell6Shutter,
}

impl ElliptecSliderModel {
    fn name(self) -> &'static str {
        match self {
            Self::Ell6 => "Thorlabs ELL6",
            Self::Ell9 => "Thorlabs ELL9",
            Self::Ell6Shutter => "Thorlabs ELL6 shutter",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Ell6 => "Thorlabs Elliptec 2-position Slider ELL6",
            Self::Ell9 => "Thorlabs Elliptec 4-position Slider ELL9",
            Self::Ell6Shutter => "Thorlabs Elliptec 2-position Slider ELL6 as shutter",
        }
    }

    fn module_code(self) -> &'static str {
        match self {
            Self::Ell6 | Self::Ell6Shutter => "06",
            Self::Ell9 => "09",
        }
    }

    fn positions(self) -> &'static [&'static str] {
        match self {
            Self::Ell6 | Self::Ell6Shutter => &["00000000", "0000001F"],
            Self::Ell9 => &["00000000", "0000001F", "0000003E", "0000005D"],
        }
    }
}

pub struct ElliptecSlider {
    props: PropertyMap,
    transport: Option<Mutex<Box<dyn Transport>>>,
    initialized: bool,
    channel: char,
    model: ElliptecSliderModel,
    labels: Vec<String>,
}

impl ElliptecSlider {
    pub fn new(channel: char) -> Self {
        Self::new_model(ElliptecSliderModel::Ell6, channel)
    }

    pub fn ell9(channel: char) -> Self {
        Self::new_model(ElliptecSliderModel::Ell9, channel)
    }

    pub fn ell6_shutter(channel: char) -> Self {
        Self::new_model(ElliptecSliderModel::Ell6Shutter, channel)
    }

    pub fn new_model(model: ElliptecSliderModel, channel: char) -> Self {
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
        let labels = (0..model.positions().len())
            .map(|i| format!("Position {}", i))
            .collect();
        Self {
            props,
            transport: None,
            initialized: false,
            channel: channel.to_ascii_uppercase(),
            model,
            labels,
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

    fn parse_id(&self, resp: &str) -> MmResult<String> {
        if let Some(err) = Self::status_error(resp) {
            return Err(err);
        }
        if resp.len() < 18 || &resp[1..3] != "IN" {
            return Err(MmError::SerialInvalidResponse);
        }
        if &resp[3..5] != self.model.module_code() {
            return Err(MmError::WrongDeviceType);
        }
        Ok(resp[3..18].to_string())
    }

    fn parse_position(&self, resp: &str) -> MmResult<u64> {
        if let Some(err) = Self::status_error(resp) {
            return Err(err);
        }
        if resp.len() < 11 || &resp[1..3] != "PO" {
            return Err(MmError::SerialInvalidResponse);
        }
        let pos = &resp[3..11];
        self.model
            .positions()
            .iter()
            .position(|p| *p == pos)
            .map(|p| p as u64)
            .ok_or(MmError::UnknownPosition)
    }

    fn query_position(&self) -> MmResult<u64> {
        let gp = self.cmd("gp")?;
        self.parse_position(&gp)
    }

    fn query_busy(&self) -> MmResult<bool> {
        let gs = self.cmd("gs")?;
        if gs.len() < 5 || &gs[1..3] != "GS" {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(&gs[3..5] != "00")
    }

    pub fn model(&self) -> ElliptecSliderModel {
        self.model
    }
}

impl Default for ElliptecSlider {
    fn default() -> Self {
        Self::new('0')
    }
}

impl Device for ElliptecSlider {
    fn name(&self) -> &str {
        self.model.name()
    }
    fn description(&self) -> &str {
        self.model.description()
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let info = self.cmd("in")?;
        let id = self.parse_id(&info)?;
        if !self.props.has_property("ID") {
            self.props
                .define_property("ID", PropertyValue::String(id), true)
                .unwrap();
        } else if let Some(entry) = self.props.entry_mut("ID") {
            entry.value = PropertyValue::String(id);
        }
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
        match self.model {
            ElliptecSliderModel::Ell6Shutter => DeviceType::Shutter,
            _ => DeviceType::State,
        }
    }
    fn busy(&self) -> bool {
        self.query_busy().unwrap_or(true)
    }
}

impl StateDevice for ElliptecSlider {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        let positions = self.model.positions();
        let hex = positions.get(pos as usize).ok_or_else(|| {
            MmError::LocallyDefined(format!(
                "Position {} out of range (0-{})",
                pos,
                positions.len() - 1
            ))
        })?;
        let response = self.cmd(&format!("ma{}", hex))?;
        if let Some(err) = Self::status_error(&response) {
            return Err(err);
        }
        if response.len() >= 3 && &response[1..3] == "PO" {
            self.parse_position(&response)?;
            Ok(())
        } else {
            Err(MmError::SerialInvalidResponse)
        }
    }

    fn get_position(&self) -> MmResult<u64> {
        self.query_position()
    }
    fn get_number_of_positions(&self) -> u64 {
        self.model.positions().len() as u64
    }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels
            .get(pos as usize)
            .cloned()
            .ok_or_else(|| MmError::LocallyDefined(format!("Position {} out of range", pos)))
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self
            .labels
            .iter()
            .position(|l| l == label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64;
        self.set_position(pos)
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        if pos >= self.get_number_of_positions() {
            return Err(MmError::LocallyDefined(format!(
                "Position {} out of range",
                pos
            )));
        }
        self.labels[pos as usize] = label.to_string();
        Ok(())
    }

    fn set_gate_open(&mut self, _open: bool) -> MmResult<()> {
        Ok(())
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(true)
    }
}

impl Shutter for ElliptecSlider {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        if self.model != ElliptecSliderModel::Ell6Shutter {
            return Err(MmError::WrongDeviceType);
        }
        let response = self.cmd(if open { "fw" } else { "bw" })?;
        if let Some(err) = Self::status_error(&response) {
            return Err(err);
        }
        if response.len() >= 3 && &response[1..3] == "PO" {
            self.parse_position(&response)?;
            Ok(())
        } else {
            Err(MmError::SerialInvalidResponse)
        }
    }

    fn get_open(&self) -> MmResult<bool> {
        if self.model != ElliptecSliderModel::Ell6Shutter {
            return Err(MmError::WrongDeviceType);
        }
        Ok(self.query_position()? == 1)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_validates_ell6_id() {
        let t = MockTransport::new().expect("0in\r", "0IN06123456782200100000000002710");
        let mut s = ElliptecSlider::new('0').with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.get_property("ID").unwrap(),
            PropertyValue::String("061234567822001".into())
        );
    }

    #[test]
    fn ell6_move_uses_absolute_positions_and_live_gp_helper() {
        let t = MockTransport::new()
            .expect("0in\r", "0IN06123456782200100000000002710")
            .expect("0ma0000001F\r", "0PO0000001F")
            .expect("0ma00000000\r", "0PO00000000")
            .expect("0gp\r", "0PO0000001F");
        let mut s = ElliptecSlider::new('0').with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position(1).unwrap();
        s.set_position(0).unwrap();
        assert_eq!(s.query_position().unwrap(), 1);
    }

    #[test]
    fn ell9_has_four_position_surface_and_validates_id() {
        let t = MockTransport::new()
            .expect("0in\r", "0IN09123456782200100000000002710")
            .expect("0ma0000005D\r", "0PO0000005D")
            .expect("0gp\r", "0PO0000003E");
        let mut s = ElliptecSlider::ell9('0').with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.name(), "Thorlabs ELL9");
        assert_eq!(s.get_number_of_positions(), 4);
        assert_eq!(s.get_position_label(2).unwrap(), "Position 2");
        s.set_position(3).unwrap();
        assert_eq!(s.query_position().unwrap(), 2);
    }

    #[test]
    fn wrong_model_id_is_rejected() {
        let t = MockTransport::new().expect("0in\r", "0IN09123456782200100000000002710");
        let mut s = ElliptecSlider::new('0').with_transport(Box::new(t));
        assert_eq!(s.initialize().unwrap_err(), MmError::WrongDeviceType);
    }

    #[test]
    fn no_transport_error() {
        assert!(ElliptecSlider::new('0').initialize().is_err());
    }

    #[test]
    fn channel_property_updates_before_init_and_is_locked_after_init() {
        let t = MockTransport::new().expect("Ain\r", "AIN06123456782200100000000002710");
        let mut s = ElliptecSlider::new('0').with_transport(Box::new(t));
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
    fn ell6_shutter_uses_upstream_fw_bw_surface() {
        let t = MockTransport::new()
            .expect("0in\r", "0IN06123456782200100000000002710")
            .expect("0fw\r", "0PO0000001F")
            .expect("0gp\r", "0PO0000001F")
            .expect("0bw\r", "0PO00000000")
            .expect("0gp\r", "0PO00000000");
        let mut s = ElliptecSlider::ell6_shutter('0').with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.device_type(), DeviceType::Shutter);
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn maps_extended_status_codes() {
        assert_eq!(
            ElliptecSlider::status_error("0GS01").unwrap(),
            MmError::SerialTimeout
        );
        assert_eq!(
            ElliptecSlider::status_error("0GS0A").unwrap(),
            MmError::LocallyDefined("Elliptec sensor error".into())
        );
        assert_eq!(
            ElliptecSlider::status_error("0GS0C").unwrap(),
            MmError::InvalidPropertyValue
        );
    }
}
