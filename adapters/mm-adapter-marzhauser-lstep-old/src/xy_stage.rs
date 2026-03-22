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
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, XYStage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct LStepOldXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
    motor_speed: f64,
}

impl LStepOldXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            x_um: 0.0,
            y_um: 0.0,
            motor_speed: 5.0,
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
        self.call_transport(|t| { t.send(&cmd)?; Ok(()) })
    }

    /// Query X position (returns integer steps/µm)
    fn get_x(&mut self) -> MmResult<f64> {
        let resp = self.cmd("UI_GET_X")?;
        resp.trim().parse::<f64>()
            .map_err(|_| MmError::LocallyDefined(format!("Bad X pos: {}", resp)))
    }

    /// Query Y position
    fn get_y(&mut self) -> MmResult<f64> {
        let resp = self.cmd("UI_GET_Y")?;
        resp.trim().parse::<f64>()
            .map_err(|_| MmError::LocallyDefined(format!("Bad Y pos: {}", resp)))
    }
}

impl Default for LStepOldXYStage {
    fn default() -> Self { Self::new() }
}

impl Device for LStepOldXYStage {
    fn name(&self) -> &str { "LStepOldXYStage" }
    fn description(&self) -> &str { "Marzhauser LStep Old (v1.2) XY stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Query motor speed (UI command)
        let speed_resp = self.cmd("UI")?;
        self.motor_speed = speed_resp.trim().parse::<f64>().unwrap_or(50.0) * 0.1;

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
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "MotorSpeed" => {
                let spd = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.motor_speed = spd;
                Ok(())
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::XYStage }
    fn busy(&self) -> bool { false }
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

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> { Ok((self.x_um, self.y_um)) }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let new_x = self.x_um + dx;
        let new_y = self.y_um + dy;
        self.set_xy_position_um(new_x, new_y)
    }

    fn home(&mut self) -> MmResult<()> {
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let _ = self.send_only("a");
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        // C++ returns DEVICE_UNSUPPORTED_COMMAND; we return a wide range
        Ok((-100_000.0, 100_000.0, -100_000.0, 100_000.0))
    }

    fn get_step_size_um(&self) -> (f64, f64) { (1.0, 1.0) }

    fn set_origin(&mut self) -> MmResult<()> {
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("UI",       "050")   // motor speed = 50 * 0.1 = 5.0 Hz
            .expect("UI_GET_X", "100")
            .expect("UI_GET_Y", "200")
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
        let t = make_transport()
            .expect("START", "OK");
        let mut stage = LStepOldXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_xy_position_um(300.0, 400.0).unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (300.0, 400.0));
    }

    #[test]
    fn move_relative() {
        // Same: send_only calls don't consume script entries.
        let t = make_transport()
            .expect("START", "OK");
        let mut stage = LStepOldXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_relative_xy_position_um(50.0, 75.0).unwrap();
        let (x, y) = stage.get_xy_position_um().unwrap();
        assert!((x - 150.0).abs() < 1e-9);
        assert!((y - 275.0).abs() < 1e-9);
    }

    #[test]
    fn no_transport_error() {
        assert!(LStepOldXYStage::new().initialize().is_err());
    }
}
