/// ChuoSeiki QT 2-axis XY stage.
///
/// Protocol (CR+LF terminated):
///   `?:CHUOSEIKI\r\n`           → "CHUOSEIKI\r\n"  (identity check)
///   `AGO:A<x>B<y>\r\n`          → OK or `!<n>` error
///   `MGO:A<dx>B<dy>\r\n`        → OK or `!<n>` error (relative move)
///   `Q:A0B0\r\n`                → `<+/->XXXXXXXXD,<+/->XXXXXXXXD\r\n`
///                                  (positions + state: D=moving, K=stopped, H=homing)
///   `H:AB\r\n`                  → OK or `!<n>` (home both axes)
///
/// Step size default: 1 µm/step.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

const DEFAULT_STEP_UM: f64 = 1.0;

pub struct ChuoSeikiQTXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
    step_x_um: f64,
    step_y_um: f64,
    speed_high_x: i64,
    speed_low_x: i64,
    accel_time_x: i64,
    speed_high_y: i64,
    speed_low_y: i64,
    accel_time_y: i64,
}

impl ChuoSeikiQTXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("X-Axis StepSize: um", PropertyValue::Float(1.0), false)
            .unwrap();
        props
            .define_property("Y-Axis StepSize: um", PropertyValue::Float(1.0), false)
            .unwrap();
        props
            .define_property("X-Axis HighSpeed: pps", PropertyValue::Float(2000.0), false)
            .unwrap();
        props
            .define_property("X-Axis LowSpeed: pps", PropertyValue::Float(500.0), false)
            .unwrap();
        props
            .define_property(
                "X-Axis AcceleratingTime: msec",
                PropertyValue::Float(100.0),
                false,
            )
            .unwrap();
        props
            .define_property("Y-Axis HighSpeed: pps", PropertyValue::Float(2000.0), false)
            .unwrap();
        props
            .define_property("Y-Axis LowSpeed: pps", PropertyValue::Float(500.0), false)
            .unwrap();
        props
            .define_property(
                "Y-Axis Accelerating Time: msec",
                PropertyValue::Float(100.0),
                false,
            )
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            x_um: 0.0,
            y_um: 0.0,
            step_x_um: DEFAULT_STEP_UM,
            step_y_um: DEFAULT_STEP_UM,
            speed_high_x: 2000,
            speed_low_x: 500,
            accel_time_x: 100,
            speed_high_y: 2000,
            speed_low_y: 500,
            accel_time_y: 100,
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
        let c = format!("{}\r\n", command);
        self.call_transport(|t| {
            let r = t.send_recv(&c)?;
            Ok(r.trim().to_string())
        })
    }

    /// Parse a controller error response; `!n` means error, anything else is ok.
    fn check_response(resp: &str) -> MmResult<()> {
        if resp.starts_with('!') {
            Err(MmError::LocallyDefined(format!(
                "ChuoSeiki QT error: {}",
                resp
            )))
        } else {
            Ok(())
        }
    }

    fn parse_position_response(resp: &str, step_x_um: f64, step_y_um: f64) -> MmResult<(f64, f64)> {
        // Expect at least 21 chars: 9+1 + ',' + 9+1
        if resp.len() < 21 {
            return Err(MmError::LocallyDefined(format!(
                "ChuoSeiki QT: unexpected position response: {}",
                resp
            )));
        }
        let x_steps: i64 = resp[..9].trim().parse().unwrap_or(0);
        let y_steps: i64 = resp[11..20].trim().parse().unwrap_or(0);
        Ok((x_steps as f64 * step_x_um, y_steps as f64 * step_y_um))
    }

    fn set_positive_float_property(&mut self, name: &str, val: PropertyValue) -> MmResult<f64> {
        let value = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
        if value <= 0.0 {
            return Err(MmError::InvalidPropertyValue);
        }
        self.props.set(name, PropertyValue::Float(value))?;
        Ok(value)
    }

    fn set_i64_property(&mut self, name: &str, val: PropertyValue) -> MmResult<i64> {
        let value = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
        self.props.set(name, PropertyValue::Float(value as f64))?;
        Ok(value)
    }
}

impl Default for ChuoSeikiQTXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ChuoSeikiQTXYStage {
    fn name(&self) -> &str {
        "ChuoSeiki_QT 2-Axis"
    }
    fn description(&self) -> &str {
        "ChuoSeiki 2-stage driver adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Identity check
        let resp = self.cmd("?:CHUOSEIKI")?;
        if !resp.starts_with("CHUOSEIKI") {
            return Err(MmError::LocallyDefined(format!(
                "ChuoSeiki QT: unexpected identity: {}",
                resp
            )));
        }
        // Enable feedback after control commands
        let _ = self.cmd("X:1");
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "X-Axis StepSize: um" => Ok(PropertyValue::Float(self.step_x_um)),
            "Y-Axis StepSize: um" => Ok(PropertyValue::Float(self.step_y_um)),
            "X-Axis HighSpeed: pps" => Ok(PropertyValue::Float(self.speed_high_x as f64)),
            "X-Axis LowSpeed: pps" => Ok(PropertyValue::Float(self.speed_low_x as f64)),
            "X-Axis AcceleratingTime: msec" => Ok(PropertyValue::Float(self.accel_time_x as f64)),
            "Y-Axis HighSpeed: pps" => Ok(PropertyValue::Float(self.speed_high_y as f64)),
            "Y-Axis LowSpeed: pps" => Ok(PropertyValue::Float(self.speed_low_y as f64)),
            "Y-Axis Accelerating Time: msec" => Ok(PropertyValue::Float(self.accel_time_y as f64)),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "X-Axis StepSize: um" => {
                self.step_x_um = self.set_positive_float_property(name, val)?;
                Ok(())
            }
            "Y-Axis StepSize: um" => {
                self.step_y_um = self.set_positive_float_property(name, val)?;
                Ok(())
            }
            "X-Axis HighSpeed: pps" => {
                self.speed_high_x = self.set_i64_property(name, val)?;
                Ok(())
            }
            "X-Axis LowSpeed: pps" => {
                self.speed_low_x = self.set_i64_property(name, val)?;
                Ok(())
            }
            "X-Axis AcceleratingTime: msec" => {
                self.accel_time_x = self.set_i64_property(name, val)?;
                Ok(())
            }
            "Y-Axis HighSpeed: pps" => {
                self.speed_high_y = self.set_i64_property(name, val)?;
                Ok(())
            }
            "Y-Axis LowSpeed: pps" => {
                self.speed_low_y = self.set_i64_property(name, val)?;
                Ok(())
            }
            "Y-Axis Accelerating Time: msec" => {
                self.accel_time_y = self.set_i64_property(name, val)?;
                Ok(())
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

impl XYStage for ChuoSeikiQTXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let xs = (x / self.step_x_um) as i64;
        let ys = (y / self.step_y_um) as i64;
        let r = self.cmd(&format!("AGO:A{}B{}", xs, ys))?;
        Self::check_response(&r)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        Ok((self.x_um, self.y_um))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let dxs = (dx / self.step_x_um) as i64;
        let dys = (dy / self.step_y_um) as i64;
        let r = self.cmd(&format!("MGO:A{}B{}", dxs, dys))?;
        Self::check_response(&r)?;
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("H:AB")?;
        Self::check_response(&r)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let r = self.cmd("L:")?;
        Self::check_response(&r)?;
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Err(MmError::UnsupportedCommand)
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        (self.step_x_um, self.step_y_um)
    }

    fn set_origin(&mut self) -> MmResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any("CHUOSEIKI") // identity
            .any("OK") // X:1
    }

    #[test]
    fn initialize() {
        let mut s = ChuoSeikiQTXYStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert_eq!((x, y), (0.0, 0.0));
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any("OK");
        let mut s = ChuoSeikiQTXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(500.0, 300.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (500.0, 300.0));
    }

    #[test]
    fn move_relative() {
        let t = make_transport().any("OK");
        let mut s = ChuoSeikiQTXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(50.0, 25.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 50.0).abs() < 1e-9);
        assert!((y - 25.0).abs() < 1e-9);
    }

    #[test]
    fn error_response_fails() {
        let t = make_transport().any("!6"); // limit detected
        let mut s = ChuoSeikiQTXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_xy_position_um(999_999.0, 0.0).is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(ChuoSeikiQTXYStage::new().initialize().is_err());
    }

    #[test]
    fn bad_identity_fails() {
        let t = MockTransport::new().any("UNKNOWN");
        let mut s = ChuoSeikiQTXYStage::new().with_transport(Box::new(t));
        assert!(s.initialize().is_err());
    }

    #[test]
    fn step_size() {
        let s = ChuoSeikiQTXYStage::new();
        assert_eq!(s.get_step_size_um(), (1.0, 1.0));
    }

    #[test]
    fn parser_uses_axis_step_sizes() {
        let (x, y) =
            ChuoSeikiQTXYStage::parse_position_response("+00000100K,+00000200K", 0.5, 2.0).unwrap();
        assert_eq!((x, y), (50.0, 400.0));
    }

    #[test]
    fn limits_are_unsupported() {
        let s = ChuoSeikiQTXYStage::new();
        assert_eq!(s.get_limits_um().unwrap_err(), MmError::UnsupportedCommand);
    }
}
