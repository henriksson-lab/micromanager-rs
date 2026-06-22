/// Zeiss CAN-bus focus (Z) stage.
///
/// Protocol (TX `\r`, RX `\r`):
///   `FPZp\r`/`HPZp\r`       -> `PF{hex6}\r`/`PH{hex6}\r`
///   `FPZT{hex6}\r`/`HPZT...` -> no-answer move command
///
/// Step size: 0.025 µm / step.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::Cell;

use super::hub::{decode_pos, encode_pos, ZeissHub, DEVICE_NAME_FOCUS, DEVICE_NAME_Z_STAGE};

const STEPS_PER_UM: f64 = 40.0; // 0.025 µm/step → 40 steps/µm

fn zeiss_focus_um_to_steps(pos_um: f64, step_size_um: f64) -> i32 {
    (pos_um / step_size_um + 0.5) as i32
}

pub struct ZeissFocusStage {
    props: PropertyMap,
    hub: ZeissHub,
    name: &'static str,
    command_prefix: &'static str,
    response_prefix: &'static str,
    has_load_position: bool,
    initialized: bool,
    pos_um: Cell<f64>,
    step_size_um: Cell<f64>,
    focus_firmware: String,
    lower_limit: f64,
    upper_limit: f64,
}

impl ZeissFocusStage {
    pub fn new() -> Self {
        Self::new_inner(ZeissHub::new(), DEVICE_NAME_FOCUS, "FP", "PF", true)
    }

    pub fn new_z_stage() -> Self {
        Self::new_inner(ZeissHub::new(), DEVICE_NAME_Z_STAGE, "HP", "PH", false)
    }

    pub fn new_with_hub(hub: ZeissHub) -> Self {
        Self::new_inner(hub, DEVICE_NAME_FOCUS, "FP", "PF", true)
    }

    pub fn new_z_stage_with_hub(hub: ZeissHub) -> Self {
        Self::new_inner(hub, DEVICE_NAME_Z_STAGE, "HP", "PH", false)
    }

    fn new_inner(
        hub: ZeissHub,
        name: &'static str,
        command_prefix: &'static str,
        response_prefix: &'static str,
        has_load_position: bool,
    ) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "StepSize (um)",
                PropertyValue::Float(1.0 / STEPS_PER_UM),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("StepSize (um)", &["0.025", "0.050"])
            .unwrap();
        props
            .define_property("Focus firmware", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Position", PropertyValue::Float(0.0), false)
            .unwrap();
        if has_load_position {
            props
                .define_property("Load Position", PropertyValue::Integer(0), false)
                .unwrap();
            props
                .set_allowed_values("Load Position", &["0", "1"])
                .unwrap();
        }
        Self {
            props,
            hub,
            name,
            command_prefix,
            response_prefix,
            has_load_position,
            initialized: false,
            pos_um: Cell::new(0.0),
            step_size_um: Cell::new(1.0 / STEPS_PER_UM),
            focus_firmware: String::new(),
            lower_limit: 0.0,
            upper_limit: 1000.0,
        }
    }

    fn send(&self, cmd: &str) -> MmResult<String> {
        self.hub.send(cmd)
    }

    fn get_pos_steps(&self) -> MmResult<i32> {
        let resp = self.send(&format!("{}Zp", self.command_prefix))?;
        let hex = resp
            .strip_prefix(self.response_prefix)
            .ok_or(MmError::SerialInvalidResponse)?;
        decode_pos(hex)
    }

    fn set_pos_steps(&self, steps: i32) -> MmResult<()> {
        let cmd = format!("{}ZT{}", self.command_prefix, encode_pos(steps));
        self.hub.execute(&cmd)
    }

    pub fn set_origin(&mut self) -> MmResult<()> {
        self.hub.execute(&format!("{}ZP0", self.command_prefix))
    }

    fn get_focus_firmware_version(&self) -> MmResult<String> {
        let resp = self.send(&format!("{}Tv0", self.command_prefix))?;
        Ok(resp
            .strip_prefix(self.response_prefix)
            .ok_or(MmError::SerialInvalidResponse)?
            .to_string())
    }

    fn read_limit(&self, command: &str) -> MmResult<f64> {
        let resp = self.send(&format!("{}{}", self.command_prefix, command))?;
        let hex = resp
            .strip_prefix(self.response_prefix)
            .ok_or(MmError::SerialInvalidResponse)?;
        Ok(decode_pos(hex)? as f64 * self.step_size_um.get())
    }

    fn read_load_position(&self) -> MmResult<i64> {
        let resp = self.send(&format!("{}Zw", self.command_prefix))?;
        let state = ZeissHub::parse_prefixed_i64(&resp, self.response_prefix)?;
        Ok(if state == 0 || state == 4 { 1 } else { 0 })
    }
}

impl Default for ZeissFocusStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ZeissFocusStage {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "Z-drive"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.hub.is_connected() {
            return Err(MmError::NotConnected);
        }
        self.focus_firmware = self.get_focus_firmware_version()?;
        if let Some(entry) = self.props.entry_mut("Focus firmware") {
            entry.value = PropertyValue::String(self.focus_firmware.clone());
        }
        self.upper_limit = self.read_limit("Zu")?;
        self.lower_limit = self.read_limit("Zl")?;
        let steps = self.get_pos_steps()?;
        self.pos_um.set(steps as f64 * self.step_size_um.get());
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Position" => Ok(PropertyValue::Float(self.get_position_um()?)),
            "StepSize (um)" => Ok(PropertyValue::Float(self.step_size_um.get())),
            "Load Position" if self.has_load_position => {
                Ok(PropertyValue::Integer(self.read_load_position()?))
            }
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Position" => {
                let z = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_position_um(z)?;
                self.props.set(name, PropertyValue::Float(z))
            }
            "StepSize (um)" => {
                let step = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if step != 0.025 && step != 0.050 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.step_size_um.set(step);
                self.props.set(name, PropertyValue::Float(step))
            }
            "Load Position" if self.has_load_position => {
                let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.hub.execute(&format!(
                    "{}ZW{}",
                    self.command_prefix,
                    if state == 0 { 1 } else { 0 }
                ))?;
                self.props.set(name, PropertyValue::Integer(state))
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
        false
    }
}

impl Stage for ZeissFocusStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = zeiss_focus_um_to_steps(z, self.step_size_um.get());
        self.set_pos_steps(steps)?;
        self.pos_um.set(z);
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        let pos = self.get_pos_steps()? as f64 * self.step_size_um.get();
        self.pos_um.set(pos);
        Ok(pos)
    }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        self.set_position_um(self.get_position_um()? + dz)
    }

    fn home(&mut self) -> MmResult<()> {
        Err(MmError::NotSupported)
    }
    fn stop(&mut self) -> MmResult<()> {
        Ok(())
    }
    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((self.lower_limit, self.upper_limit))
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
    use crate::error::MmResult;
    use crate::transport::MockTransport;
    use crate::transport::Transport;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    fn stage_with(t: MockTransport) -> ZeissFocusStage {
        let hub = ZeissHub::new().with_transport(Box::new(t));
        ZeissFocusStage::new_with_hub(hub)
    }

    struct RecordingTransport {
        expected_sends: VecDeque<String>,
        responses: VecDeque<String>,
        sent: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingTransport {
        fn new(sent: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                expected_sends: VecDeque::new(),
                responses: VecDeque::new(),
                sent,
            }
        }

        fn expect_send(mut self, command: &str) -> Self {
            self.expected_sends.push_back(command.to_string());
            self
        }

        fn response(mut self, response: &str) -> Self {
            self.responses.push_back(response.to_string());
            self
        }
    }

    impl Transport for RecordingTransport {
        fn send(&mut self, cmd: &str) -> MmResult<()> {
            if let Some(expected) = self.expected_sends.pop_front() {
                assert_eq!(cmd, expected);
            }
            self.sent.lock().unwrap().push(cmd.to_string());
            Ok(())
        }

        fn receive_line(&mut self) -> MmResult<String> {
            self.responses.pop_front().ok_or(MmError::SerialTimeout)
        }

        fn purge(&mut self) -> MmResult<()> {
            Ok(())
        }
    }

    #[test]
    fn reports_upstream_device_identity() {
        let s = ZeissFocusStage::new();
        assert_eq!(s.name(), "Focus");
        assert_eq!(s.description(), "Z-drive");
        let z = ZeissFocusStage::new_z_stage();
        assert_eq!(z.name(), "ZStage");
        assert_eq!(z.description(), "Z-drive");
        assert!(!z.has_property("Load Position"));
    }

    #[test]
    fn initialize_reads_position() {
        // FPZp -> PF000190 = 400 steps = 10 um
        let t = MockTransport::new()
            .expect("FPTv0\r", "PFAP2_09")
            .expect("FPZu\r", "PF000000")
            .expect("FPZl\r", "PF000000")
            .expect("FPZp\r", "PF000190")
            .expect("FPZp\r", "PF000190");
        let mut s = stage_with(t);
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 10.0).abs() < 1e-6);
    }

    #[test]
    fn move_absolute() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let t = RecordingTransport::new(sent.clone())
            .expect_send("FPTv0\r")
            .response("PFAP2_09")
            .expect_send("FPZu\r")
            .response("PF000000")
            .expect_send("FPZl\r")
            .response("PF000000")
            .expect_send("FPZp\r")
            .response("PF000000")
            .expect_send("FPZT0003E8\r")
            .expect_send("FPZp\r")
            .response("PF0003E8");
        let hub = ZeissHub::new().with_transport(Box::new(t));
        let mut s = ZeissFocusStage::new_with_hub(hub);
        s.initialize().unwrap();
        s.set_position_um(25.0).unwrap();
        assert!((s.get_position_um().unwrap() - 25.0).abs() < 1e-6);
        assert!(sent.lock().unwrap().contains(&"FPZT0003E8\r".to_string()));
    }

    #[test]
    fn negative_move_uses_upstream_step_casting() {
        // Upstream FocusStage uses C++ `(long)(pos / stepSize + 0.5)`.
        let t = MockTransport::new()
            .expect("FPTv0\r", "PFAP2_09")
            .expect("FPZu\r", "PF000000")
            .expect("FPZl\r", "PF000000")
            .expect("FPZp\r", "PF000000")
            .expect("FPZTFFFE71\r", "");
        let mut s = stage_with(t);
        s.initialize().unwrap();
        s.set_position_um(-10.0).unwrap();
    }

    #[test]
    fn negative_position() {
        // init at -10 um = -400 steps -> hex FFFE70... let roundtrip verify
        use super::super::hub::encode_pos;
        let hex = format!("PH{}", encode_pos(-400));
        let t = MockTransport::new()
            .expect("FPTv0\r", "PFAP2_09")
            .expect("FPZu\r", "PF000000")
            .expect("FPZl\r", "PF000000")
            .expect("FPZp\r", &hex.replacen("PH", "PF", 1))
            .expect("FPZp\r", &hex.replacen("PH", "PF", 1));
        let mut s = stage_with(t);
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - (-10.0)).abs() < 1e-6);
    }

    #[test]
    fn home_is_unsupported() {
        let t = MockTransport::new()
            .expect("FPTv0\r", "PFAP2_09")
            .expect("FPZu\r", "PF000000")
            .expect("FPZl\r", "PF000000")
            .expect("FPZp\r", "PF000000");
        let mut s = stage_with(t);
        s.initialize().unwrap();
        assert_eq!(s.home().unwrap_err(), MmError::NotSupported);
    }

    #[test]
    fn set_origin_sends_upstream_command() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let t = RecordingTransport::new(sent.clone())
            .expect_send("FPTv0\r")
            .response("PFAP2_09")
            .expect_send("FPZu\r")
            .response("PF000000")
            .expect_send("FPZl\r")
            .response("PF000000")
            .expect_send("FPZp\r")
            .response("PF000000")
            .expect_send("FPZP0\r");
        let hub = ZeissHub::new().with_transport(Box::new(t));
        let mut s = ZeissFocusStage::new_with_hub(hub);
        s.initialize().unwrap();
        s.set_origin().unwrap();
        assert_eq!(sent.lock().unwrap().last().unwrap(), "FPZP0\r");
    }

    #[test]
    fn axioskop_z_stage_uses_hp_ph_protocol() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let t = RecordingTransport::new(sent.clone())
            .expect_send("HPTv0\r")
            .response("PHAP2_09")
            .expect_send("HPZu\r")
            .response("PH000000")
            .expect_send("HPZl\r")
            .response("PH000000")
            .expect_send("HPZp\r")
            .response("PH000000")
            .expect_send("HPZT000064\r")
            .expect_send("HPZp\r")
            .response("PH000064");
        let hub = ZeissHub::new().with_transport(Box::new(t));
        let mut s = ZeissFocusStage::new_z_stage_with_hub(hub);
        s.initialize().unwrap();
        s.set_position_um(2.5).unwrap();
        assert!((s.get_position_um().unwrap() - 2.5).abs() < 1e-6);
        assert!(sent.lock().unwrap().contains(&"HPZT000064\r".to_string()));
    }
}
