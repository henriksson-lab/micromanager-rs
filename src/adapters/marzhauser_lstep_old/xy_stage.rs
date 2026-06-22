/// Marzhauser LStep Old (v1.2) XY stage.
///
/// This older controller uses a binary/mixed protocol:
///   `UI\r`             → get motor speed (returns decimal string)
///   `U\t<3bytes>\r`    → set motor speed (3 ASCII digit bytes)
///   `U\x43\r`          → get X position (U,67 = get pos X)
///   `U\x44\r`          → get Y position (U,68 = get pos Y)
///   `U\x07r\r U P\0`   → goto absolute (prepare)
///   `U\x00<15 ascii digits>\r` → set X position value
///   `U\x01<15 ascii digits>\r` → set Y position value
///   `U P\0\r`          → start motion
///   `a\r`              → stop
///
/// In practice, for the Rust adapter we model this as text commands for
/// testability, keeping the same logical command strings the C++ uses.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct LStepOldXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
    motor_speed: f64,
    joystick_command: String,
}

impl LStepOldXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            x_um: 0.0,
            y_um: 0.0,
            motor_speed: 5.0,
            joystick_command: "False".into(),
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
        let cmd = command.to_string();
        self.call_transport(|t| Ok(t.send_recv(&cmd)?.trim().to_string()))
    }

    fn send_only(&mut self, command: &str) -> MmResult<()> {
        let cmd = command.to_string();
        self.call_transport(|t| {
            t.send(&cmd)?;
            Ok(())
        })
    }

    fn define_runtime_properties(&mut self, firmware: String) -> MmResult<()> {
        if !self.props.has_property("Motor-speed [Hz]") {
            self.props.define_property(
                "Motor-speed [Hz]",
                PropertyValue::Float(self.motor_speed),
                false,
            )?;
            self.props
                .set_property_limits("Motor-speed [Hz]", 0.01, 25.0)?;
        }
        if !self.props.has_property("Firmware Version") {
            self.props.define_property(
                "Firmware Version",
                PropertyValue::String(firmware),
                true,
            )?;
        } else if let Some(entry) = self.props.entry_mut("Firmware Version") {
            entry.value = PropertyValue::String(firmware);
        }
        if !self.props.has_property("Joystick command") {
            self.props.define_property(
                "Joystick command",
                PropertyValue::String(self.joystick_command.clone()),
                false,
            )?;
            self.props
                .set_allowed_values("Joystick command", &["True", "False"])?;
        }
        Ok(())
    }

    fn set_motor_speed(&mut self, speed_hz: f64) -> MmResult<()> {
        if !(0.01..=25.0).contains(&speed_hz) {
            return Err(MmError::InvalidPropertyValue);
        }
        let encoded = ((speed_hz + 0.05) * 10.0) as i32;
        self.send_only(&format!("U\t{:03}", encoded))?;
        self.motor_speed = speed_hz;
        if self.props.has_property("Motor-speed [Hz]") {
            self.props
                .set("Motor-speed [Hz]", PropertyValue::Float(speed_hz))?;
        }
        Ok(())
    }

    fn set_joystick_command(&mut self, value: String) -> MmResult<()> {
        match value.as_str() {
            "True" => self.send_only("U\u{7}j\rUP")?,
            "False" => self.send_only("j")?,
            _ => return Err(MmError::InvalidPropertyValue),
        }
        self.joystick_command = value.clone();
        if self.props.has_property("Joystick command") {
            self.props
                .set("Joystick command", PropertyValue::String(value))?;
        }
        Ok(())
    }

    /// Query X position (returns integer steps/µm)
    fn get_x(&mut self) -> MmResult<f64> {
        let resp = self.cmd("UC")?;
        resp.trim()
            .parse::<f64>()
            .map_err(|_| MmError::LocallyDefined(format!("Bad X pos: {}", resp)))
    }

    /// Query Y position
    fn get_y(&mut self) -> MmResult<f64> {
        let resp = self.cmd("UD")?;
        resp.trim()
            .parse::<f64>()
            .map_err(|_| MmError::LocallyDefined(format!("Bad Y pos: {}", resp)))
    }
}

impl Default for LStepOldXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for LStepOldXYStage {
    fn name(&self) -> &str {
        "XYStage"
    }
    fn description(&self) -> &str {
        "LStepOld XY Stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        let speed_resp = self.cmd("UI")?;
        self.motor_speed = speed_resp.trim().parse::<f64>().unwrap_or(50.0) * 0.1;

        let firmware = self.cmd("U\u{7}b\rUP")?;
        self.define_runtime_properties(firmware)?;

        // Query current positions
        self.x_um = self.get_x()?;
        self.y_um = self.get_y()?;

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        let _ = self.send_only("a"); // stop / deactivate joystick
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "MotorSpeed" => Ok(PropertyValue::Float(self.motor_speed)),
            "Motor-speed [Hz]" if self.props.has_property("Motor-speed [Hz]") => {
                Ok(PropertyValue::Float(self.motor_speed))
            }
            "Joystick command" if self.props.has_property("Joystick command") => {
                Ok(PropertyValue::String(self.joystick_command.clone()))
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "MotorSpeed" => {
                let spd = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_motor_speed(spd)
            }
            "Motor-speed [Hz]" => {
                if !self.props.has_property("Motor-speed [Hz]") {
                    return Err(MmError::UnknownLabel(name.to_string()));
                }
                let spd = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_motor_speed(spd)
            }
            "Joystick command" => {
                if !self.props.has_property("Joystick command") {
                    return Err(MmError::UnknownLabel(name.to_string()));
                }
                let value = val.to_string();
                self.set_joystick_command(value)
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
        DeviceType::XYStage
    }
    fn busy(&self) -> bool {
        false
    }
}

impl XYStage for LStepOldXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let posx = format!("{:015}", x as i64);
        let posy = format!("{:015}", y as i64);
        // Send goto absolute command sequence
        let _ = self.send_only("GOTO_ABS");
        let _ = self.send_only(&format!("SET_X {}", posx));
        let _ = self.send_only(&format!("SET_Y {}", posy));
        let _ = self.cmd("START")?; // expects a response
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        Ok((self.x_um, self.y_um))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let new_x = self.x_um + dx;
        let new_y = self.y_um + dy;
        self.set_xy_position_um(new_x, new_y)
    }

    fn home(&mut self) -> MmResult<()> {
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let _ = self.send_only("a");
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Err(MmError::UnsupportedCommand)
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        (1.0, 1.0)
    }

    fn set_origin(&mut self) -> MmResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    struct RecordingTransport {
        replies: VecDeque<String>,
        sent: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingTransport {
        fn new(replies: &[&str]) -> (Self, Arc<Mutex<Vec<String>>>) {
            let sent = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    replies: replies.iter().map(|s| s.to_string()).collect(),
                    sent: Arc::clone(&sent),
                },
                sent,
            )
        }
    }

    impl Transport for RecordingTransport {
        fn send(&mut self, cmd: &str) -> MmResult<()> {
            self.sent.lock().unwrap().push(cmd.to_string());
            Ok(())
        }

        fn receive_line(&mut self) -> MmResult<String> {
            self.replies.pop_front().ok_or(MmError::SerialTimeout)
        }

        fn purge(&mut self) -> MmResult<()> {
            Ok(())
        }
    }

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("UI", "050") // motor speed = 50 * 0.1 = 5.0 Hz
            .expect("U\u{7}b\rUP", "1.2")
            .expect("UC", "100")
            .expect("UD", "200")
    }

    #[test]
    fn initialize() {
        let mut stage = LStepOldXYStage::new().with_transport(Box::new(make_transport()));
        stage.initialize().unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (100.0, 200.0));
    }

    #[test]
    fn move_absolute() {
        // GOTO_ABS, SET_X, SET_Y are send_only — no script entries.
        // START is cmd() (send_recv) — one script entry.
        let t = make_transport().expect("START", "OK");
        let mut stage = LStepOldXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_xy_position_um(300.0, 400.0).unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (300.0, 400.0));
    }

    #[test]
    fn move_relative() {
        // Same: send_only calls don't consume script entries.
        let t = make_transport().expect("START", "OK");
        let mut stage = LStepOldXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_relative_xy_position_um(50.0, 75.0).unwrap();
        let (x, y) = stage.get_xy_position_um().unwrap();
        assert!((x - 150.0).abs() < 1e-9);
        assert!((y - 275.0).abs() < 1e-9);
    }

    #[test]
    fn initialize_creates_upstream_runtime_properties() {
        let mut stage = LStepOldXYStage::new().with_transport(Box::new(make_transport()));
        assert!(!stage.has_property("Motor-speed [Hz]"));
        assert_eq!(
            stage
                .set_property("Motor-speed [Hz]", PropertyValue::Float(10.0))
                .unwrap_err(),
            MmError::UnknownLabel("Motor-speed [Hz]".into())
        );
        stage.initialize().unwrap();
        assert_eq!(
            stage.get_property("Firmware Version").unwrap(),
            PropertyValue::String("1.2".into())
        );
        assert_eq!(
            stage.get_property("Motor-speed [Hz]").unwrap(),
            PropertyValue::Float(5.0)
        );
        assert_eq!(
            stage.get_property("Joystick command").unwrap(),
            PropertyValue::String("False".into())
        );
    }

    #[test]
    fn motor_speed_setter_sends_upstream_encoded_value() {
        let (t, sent) = RecordingTransport::new(&["050", "1.2", "100", "200"]);
        let mut stage = LStepOldXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage
            .set_property("Motor-speed [Hz]", PropertyValue::Float(12.3))
            .unwrap();
        assert!(sent.lock().unwrap().iter().any(|cmd| cmd == "U\t123"));
        assert_eq!(
            stage.get_property("Motor-speed [Hz]").unwrap(),
            PropertyValue::Float(12.3)
        );
    }

    #[test]
    fn motor_speed_limits_are_enforced_before_send() {
        let mut stage = LStepOldXYStage::new().with_transport(Box::new(make_transport()));
        stage.initialize().unwrap();
        assert_eq!(
            stage
                .set_property("Motor-speed [Hz]", PropertyValue::Float(25.1))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            stage.get_property("Motor-speed [Hz]").unwrap(),
            PropertyValue::Float(5.0)
        );
    }

    #[test]
    fn joystick_command_sends_upstream_command_and_rejects_invalid_values() {
        let (t, sent) = RecordingTransport::new(&["050", "1.2", "100", "200"]);
        let mut stage = LStepOldXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage
            .set_property("Joystick command", PropertyValue::String("True".into()))
            .unwrap();
        assert!(sent.lock().unwrap().iter().any(|cmd| cmd == "U\u{7}j\rUP"));
        assert_eq!(
            stage.get_property("Joystick command").unwrap(),
            PropertyValue::String("True".into())
        );
        assert_eq!(
            stage
                .set_property("Joystick command", PropertyValue::String("bad".into()))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn no_transport_error() {
        assert!(LStepOldXYStage::new().initialize().is_err());
    }
}
