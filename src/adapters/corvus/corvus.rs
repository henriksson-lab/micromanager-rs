/// ITK Corvus XY-stage controller.
///
/// Protocol: commands end with space `" "` (TX terminator), responses end with `\r\n`.
///   `"0 mode "`           → enter host mode
///   `"version "`          → firmware version string
///   `"1 1 setunit "`      → set axis 1 to µm
///   `"1 2 setunit "`      → set axis 2 to µm
///   `"ge "`               → clear errors
///   `"p "`                → query position → `"X Y\r\n"` (floats, µm)
///   `"X Y move "`         → absolute move (µm)
///   `"dX dY rmove "`      → relative move (µm)
///   `"st "`               → status (bit 0 = busy)
///   `"cal "`              → calibrate/home
///   `"abort "`            → emergency stop
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};
use std::time::Duration;

pub struct CorvusXYStage {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    x_um: Cell<f64>,
    y_um: Cell<f64>,
    speed_mm_s: Cell<f64>,
    accel_m_s2: Cell<f64>,
    joystick_enabled: Cell<bool>,
    range_measured: Cell<bool>,
}

impl CorvusXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Version", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Speed [mm/s]", PropertyValue::Float(40.0), false)
            .unwrap();
        props
            .set_property_limits("Speed [mm/s]", 0.001, 100.0)
            .unwrap();
        props
            .define_property("Acceleration [m/s^2]", PropertyValue::Float(0.2), false)
            .unwrap();
        props
            .set_property_limits("Acceleration [m/s^2]", 0.01, 2.0)
            .unwrap();
        props
            .define_property(
                "Enable joystick?",
                PropertyValue::String("False".to_string()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("Enable joystick?", &["True", "False"])
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            x_um: Cell::new(0.0),
            y_um: Cell::new(0.0),
            speed_mm_s: Cell::new(40.0),
            accel_m_s2: Cell::new(0.2),
            joystick_enabled: Cell::new(false),
            range_measured: Cell::new(false),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(RefCell::new(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_ref() {
            Some(t) => f(t.borrow_mut().as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    /// Send command with trailing space (Corvus TX terminator).
    fn cmd(&self, command: &str) -> MmResult<String> {
        let cmd = format!("{} ", command);
        self.call_transport(|t| {
            let r = t.send_recv(&cmd)?;
            Ok(r.trim().to_string())
        })
    }

    fn parse_xy(resp: &str) -> MmResult<(f64, f64)> {
        let parts: Vec<&str> = resp.trim().split_whitespace().collect();
        if parts.len() < 2 {
            return Err(MmError::LocallyDefined(format!(
                "Cannot parse XY: {}",
                resp
            )));
        }
        Ok((
            parts[0].parse().unwrap_or(0.0),
            parts[1].parse().unwrap_or(0.0),
        ))
    }

    fn select_xy_axes(&mut self) -> MmResult<()> {
        self.cmd("2 setdim")?;
        self.cmd("1 1 setaxis")?;
        self.cmd("1 2 setaxis")?;
        Ok(())
    }

    fn query_xy_position_um(&self) -> MmResult<(f64, f64)> {
        let pos = self.cmd("p")?;
        let (x, y) = Self::parse_xy(&pos)?;
        self.x_um.set(x);
        self.y_um.set(y);
        Ok((x, y))
    }

    fn query_busy(&self) -> MmResult<bool> {
        let resp = self.cmd("st")?;
        let status: i64 = resp
            .trim()
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        Ok(status & 1 != 0)
    }

    fn query_speed_mm_s(&self) -> MmResult<f64> {
        let resp = self.cmd("getvel")?;
        let speed_um_s: f64 = resp
            .trim()
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        let speed = speed_um_s / 1000.0;
        self.speed_mm_s.set(speed);
        Ok(speed)
    }

    fn query_accel_m_s2(&self) -> MmResult<f64> {
        let resp = self.cmd("getaccel")?;
        let accel: f64 = resp
            .trim()
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        self.accel_m_s2.set(accel);
        Ok(accel)
    }

    fn wait_until_not_busy(&self) -> MmResult<()> {
        for _ in 0..400 {
            if !self.query_busy()? {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err(MmError::Err)
    }

    fn query_measured_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        if !self.range_measured.get() {
            return Err(MmError::UnknownPosition);
        }
        self.cmd("2 setdim")?;
        self.call_transport(|t| {
            t.send("getlimit ")?;
            let x_line = t.receive_line()?;
            let y_line = t.receive_line()?;
            let _ack = t.receive_line()?;
            let (x_min, x_max) = Self::parse_limit_line(&x_line)?;
            let (y_min, y_max) = Self::parse_limit_line(&y_line)?;
            Ok((x_min, x_max, y_min, y_max))
        })
    }

    fn parse_limit_line(resp: &str) -> MmResult<(f64, f64)> {
        let parts: Vec<&str> = resp.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(MmError::SerialInvalidResponse);
        }
        let lower = parts[0]
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        let upper = parts[1]
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        Ok((lower, upper))
    }
}

impl Default for CorvusXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CorvusXYStage {
    fn name(&self) -> &str {
        "XY Stage"
    }
    fn description(&self) -> &str {
        "XY Stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let _ = self.cmd("0 mode");
        let ver = self.cmd("version")?;
        self.props
            .entry_mut("Version")
            .map(|e| e.value = PropertyValue::String(ver));
        let _ = self.cmd("1 1 setunit");
        let _ = self.cmd("1 2 setunit");
        let _ = self.cmd("ge");
        self.select_xy_axes()?;
        let pos = self.cmd("p")?;
        let (x, y) = Self::parse_xy(&pos)?;
        self.x_um.set(x);
        self.y_um.set(y);
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.range_measured.set(false);
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Speed [mm/s]" if self.initialized => {
                Ok(PropertyValue::Float(self.query_speed_mm_s()?))
            }
            "Speed [mm/s]" => Ok(PropertyValue::Float(self.speed_mm_s.get())),
            "Acceleration [m/s^2]" if self.initialized => {
                Ok(PropertyValue::Float(self.query_accel_m_s2()?))
            }
            "Acceleration [m/s^2]" => Ok(PropertyValue::Float(self.accel_m_s2.get())),
            "Enable joystick?" => Ok(PropertyValue::String(
                if self.joystick_enabled.get() {
                    "True"
                } else {
                    "False"
                }
                .to_string(),
            )),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Speed [mm/s]" => {
                let speed = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0.001..=100.0).contains(&speed) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.cmd(&format!("{} setvel", speed * 1000.0))?;
                self.speed_mm_s.set(speed);
                self.props.set(name, PropertyValue::Float(speed))
            }
            "Acceleration [m/s^2]" => {
                let accel = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0.01..=2.0).contains(&accel) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.cmd(&format!("{} setaccel", accel))?;
                self.accel_m_s2.set(accel);
                self.props.set(name, PropertyValue::Float(accel))
            }
            "Enable joystick?" => {
                let state = val.as_str();
                let toggle = match state {
                    "True" => 1,
                    "False" => 0,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.cmd(&format!("{} j", toggle))?;
                self.joystick_enabled.set(toggle == 1);
                self.props
                    .set(name, PropertyValue::String(state.to_string()))
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
        self.query_busy().unwrap_or(false)
    }
}

impl XYStage for CorvusXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        self.select_xy_axes()?;
        self.cmd(&format!("{:.4} {:.4} move", x, y))?;
        self.x_um.set(x);
        self.y_um.set(y);
        Ok(())
    }
    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        self.query_xy_position_um()
    }
    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        self.select_xy_axes()?;
        self.cmd(&format!("{:.4} {:.4} rmove", dx, dy))?;
        self.x_um.set(self.x_um.get() + dx);
        self.y_um.set(self.y_um.get() + dy);
        Ok(())
    }
    fn home(&mut self) -> MmResult<()> {
        self.range_measured.set(false);
        self.cmd("cal")?;
        self.wait_until_not_busy()?;
        self.cmd("rm")?;
        self.wait_until_not_busy()?;
        self.range_measured.set(true);
        self.x_um.set(0.0);
        self.y_um.set(0.0);
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd("abort");
        Ok(())
    }
    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        self.query_measured_limits_um()
    }
    fn get_step_size_um(&self) -> (f64, f64) {
        (0.1, 0.1)
    }
    fn set_origin(&mut self) -> MmResult<()> {
        self.select_xy_axes()?;
        self.cmd("0 0 setpos")?;
        self.x_um.set(0.0);
        self.y_um.set(0.0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any("OK") // 0 mode
            .any("Corvus v2.3") // version
            .any("OK") // 1 1 setunit
            .any("OK") // 1 2 setunit
            .any("OK") // ge
            .expect("2 setdim ", "OK")
            .expect("1 1 setaxis ", "OK")
            .expect("1 2 setaxis ", "OK")
            .any("100.0 200.0") // p
    }

    #[test]
    fn initialize() {
        let t = make_transport().expect("p ", "100.0 200.0");
        let mut s = CorvusXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (100.0, 200.0));
    }

    #[test]
    fn move_absolute() {
        let t = make_transport()
            .expect("2 setdim ", "OK")
            .expect("1 1 setaxis ", "OK")
            .expect("1 2 setaxis ", "OK")
            .any("OK")
            .expect("p ", "300.0 400.0");
        let mut s = CorvusXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(300.0, 400.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (300.0, 400.0));
    }

    #[test]
    fn move_relative() {
        let t = make_transport()
            .expect("2 setdim ", "OK")
            .expect("1 1 setaxis ", "OK")
            .expect("1 2 setaxis ", "OK")
            .any("OK")
            .expect("p ", "110.0 220.0");
        let mut s = CorvusXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(10.0, 20.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 110.0).abs() < 1e-9);
        assert!((y - 220.0).abs() < 1e-9);
    }

    #[test]
    fn no_transport_error() {
        assert!(CorvusXYStage::new().initialize().is_err());
    }

    #[test]
    fn limits_are_not_fabricated() {
        let s = CorvusXYStage::new();
        assert_eq!(s.get_limits_um().unwrap_err(), MmError::UnknownPosition);
    }

    #[test]
    fn home_measures_range_and_get_limits_reads_controller_limits() {
        let t = make_transport()
            .expect("cal ", "OK")
            .expect("st ", "0")
            .expect("rm ", "OK")
            .expect("st ", "0")
            .expect("2 setdim ", "OK")
            .expect("getlimit ", "-100.0 25000.0")
            .expect("getlimit ", "-50.0 12000.0")
            .expect("getlimit ", "OK");
        let mut s = CorvusXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_limits_um().unwrap_err(), MmError::UnknownPosition);
        s.home().unwrap();
        assert_eq!(
            s.get_limits_um().unwrap(),
            (-100.0, 25000.0, -50.0, 12000.0)
        );
    }

    #[test]
    fn speed_accel_and_joystick_properties_send_controller_commands() {
        let t = make_transport()
            .expect("42000 setvel ", "OK")
            .expect("0.5 setaccel ", "OK")
            .expect("1 j ", "OK");
        let mut s = CorvusXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("Speed [mm/s]", PropertyValue::Float(42.0))
            .unwrap();
        s.set_property("Acceleration [m/s^2]", PropertyValue::Float(0.5))
            .unwrap();
        s.set_property(
            "Enable joystick?",
            PropertyValue::String("True".to_string()),
        )
        .unwrap();
        assert_eq!(
            s.get_property("Enable joystick?").unwrap(),
            PropertyValue::String("True".to_string())
        );
    }

    #[test]
    fn failed_joystick_write_preserves_cached_state() {
        let t = make_transport();
        let mut s = CorvusXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();

        assert!(s
            .set_property(
                "Enable joystick?",
                PropertyValue::String("True".to_string()),
            )
            .is_err());
        assert_eq!(
            s.get_property("Enable joystick?").unwrap(),
            PropertyValue::String("False".to_string())
        );
    }

    #[test]
    fn acceleration_property_uses_upstream_user_limits() {
        let mut s = CorvusXYStage::new();
        assert_eq!(
            s.set_property("Acceleration [m/s^2]", PropertyValue::Float(2.5))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            s.set_property("Acceleration [m/s^2]", PropertyValue::Float(0.005))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn busy_polls_status_bit() {
        let t = make_transport().expect("st ", "1");
        let mut s = CorvusXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
    }

    #[test]
    fn live_position_speed_and_accel_queries() {
        let t = make_transport()
            .expect("p ", "12.5 34.5")
            .expect("getvel ", "45000")
            .expect("getaccel ", "0.75");
        let mut s = CorvusXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (12.5, 34.5));
        assert_eq!(
            s.get_property("Speed [mm/s]").unwrap(),
            PropertyValue::Float(45.0)
        );
        assert_eq!(
            s.get_property("Acceleration [m/s^2]").unwrap(),
            PropertyValue::Float(0.75)
        );
    }
}
