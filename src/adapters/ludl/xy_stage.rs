/// Ludl Electronic Products MAC5000/MAC6000 XY stage.
///
/// Protocol (TX `\r`, RX `\n`):
///   `MOVE X=<n> Y=<n>\r` → `:A` (steps, default 0.1 µm resolution)
///   `MOVREL X=<n> Y=<n>\r` → `:A`
///   `WHERE X Y\r`      → `:A <x> <y>`
///   `HOME X Y\r`       → `:A`
///   `HALT\r`           → `:A`
///   `HERE X=0 Y=0\r`   → `:A`  (set origin)
///
/// Step size: 0.1 µm / step.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};

const STEP_SIZE_UM: f64 = 0.1;

pub struct LudlXYStage {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    x_um: Cell<f64>,
    y_um: Cell<f64>,
    step_size_um: f64,
    step_size_x_um: f64,
    step_size_y_um: f64,
    speed_um_s: f64,
    start_speed_um_s: f64,
    accel: f64,
}

impl LudlXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("StepSize", PropertyValue::Float(1.0), false)
            .unwrap();
        props
            .define_property("StepSize-X", PropertyValue::Float(0.1), false)
            .unwrap();
        props
            .define_property("StepSize-Y", PropertyValue::Float(0.1), false)
            .unwrap();
        props
            .define_property("Speed", PropertyValue::Float(2500.0), false)
            .unwrap();
        props
            .define_property("StartSpeed", PropertyValue::Float(500.0), false)
            .unwrap();
        props
            .define_property("Acceleration", PropertyValue::Float(100.0), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            x_um: Cell::new(0.0),
            y_um: Cell::new(0.0),
            step_size_um: 1.0,
            step_size_x_um: 0.1,
            step_size_y_um: 0.1,
            speed_um_s: 2500.0,
            start_speed_um_s: 500.0,
            accel: 100.0,
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

    fn cmd(&self, command: &str) -> MmResult<String> {
        let c = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&c)?;
            Ok(r.trim().to_string())
        })
    }

    /// Check `:A ...` response; strip prefix and return remainder.
    fn check_a(resp: &str) -> MmResult<&str> {
        let s = resp.trim();
        if let Some(rest) = s.strip_prefix(":A") {
            Ok(rest.trim())
        } else {
            Err(MmError::LocallyDefined(format!("Ludl error: {}", s)))
        }
    }

    /// Parse `:A <x> <y>` → (x_um, y_um)
    fn parse_xy(resp: &str) -> MmResult<(f64, f64)> {
        let body = Self::check_a(resp)?;
        let parts: Vec<&str> = body.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(MmError::LocallyDefined(format!(
                "Cannot parse WHERE response: {}",
                resp
            )));
        }
        let x: i64 = parts[0].parse().unwrap_or(0);
        let y: i64 = parts[1].parse().unwrap_or(0);
        Ok((x as f64 * STEP_SIZE_UM, y as f64 * STEP_SIZE_UM))
    }

    fn query_position_um(&self) -> MmResult<(f64, f64)> {
        self.call_transport(|t| t.purge())?;
        let pos = self.cmd("WHERE X Y")?;
        let (x, y) = Self::parse_xy(&pos)?;
        self.x_um.set(x);
        self.y_um.set(y);
        Ok((x, y))
    }

    fn poll_module_busy(&self) -> bool {
        self.call_transport(|t| {
            if t.purge().is_err() || t.send("STATUS S\r").is_err() {
                return Ok(false);
            }
            Ok(match t.receive_line() {
                Ok(resp) => match resp.trim().as_bytes().first().copied() {
                    Some(b'N') => false,
                    Some(b'B') => true,
                    _ => true,
                },
                Err(_) => true,
            })
        })
        .unwrap_or(false)
    }
}

impl Default for LudlXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for LudlXYStage {
    fn name(&self) -> &str {
        "LudlXYStage"
    }
    fn description(&self) -> &str {
        "Ludl MAC5000/MAC6000 XY stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "StepSize" => Ok(PropertyValue::Float(self.step_size_um)),
            "StepSize-X" => Ok(PropertyValue::Float(self.step_size_x_um)),
            "StepSize-Y" => Ok(PropertyValue::Float(self.step_size_y_um)),
            "Speed" => Ok(PropertyValue::Float(self.speed_um_s)),
            "StartSpeed" => Ok(PropertyValue::Float(self.start_speed_um_s)),
            "Acceleration" => Ok(PropertyValue::Float(self.accel)),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "StepSize" => {
                let step = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if step <= 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.step_size_um = step;
                self.step_size_x_um = step;
                self.step_size_y_um = step;
                self.props.set("StepSize-X", PropertyValue::Float(step))?;
                self.props.set("StepSize-Y", PropertyValue::Float(step))?;
                self.props.set(name, PropertyValue::Float(step))
            }
            "StepSize-X" => {
                let step = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if step <= 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.step_size_x_um = step;
                self.props.set(name, PropertyValue::Float(step))
            }
            "StepSize-Y" => {
                let step = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if step <= 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.step_size_y_um = step;
                self.props.set(name, PropertyValue::Float(step))
            }
            "Speed" => {
                self.speed_um_s = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.speed_um_s))
            }
            "StartSpeed" => {
                self.start_speed_um_s = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Float(self.start_speed_um_s))
            }
            "Acceleration" => {
                self.accel = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.accel))
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
        self.poll_module_busy()
    }
}

impl XYStage for LudlXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let xs = (x / self.step_size_x_um).round() as i64;
        let ys = (y / self.step_size_y_um).round() as i64;
        let r = self.cmd(&format!("MOVE X={} Y={}", xs, ys))?;
        Self::check_a(&r)?;
        self.x_um.set(x);
        self.y_um.set(y);
        Ok(())
    }
    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        if self.initialized && self.transport.is_some() {
            self.query_position_um()
        } else {
            Ok((self.x_um.get(), self.y_um.get()))
        }
    }
    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let xs = (dx / self.step_size_x_um).round() as i64;
        let ys = (dy / self.step_size_y_um).round() as i64;
        let r = self.cmd(&format!("MOVREL X={} Y={}", xs, ys))?;
        Self::check_a(&r)?;
        self.x_um.set(self.x_um.get() + dx);
        self.y_um.set(self.y_um.get() + dy);
        Ok(())
    }
    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("HOME X Y")?;
        Self::check_a(&r)?;
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd("HALT");
        Ok(())
    }
    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Err(MmError::UnsupportedCommand)
    }
    fn get_step_size_um(&self) -> (f64, f64) {
        (self.step_size_x_um, self.step_size_y_um)
    }
    fn set_origin(&mut self) -> MmResult<()> {
        let r = self.cmd("HERE X=0 Y=0")?;
        Self::check_a(&r)?;
        self.query_position_um()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
    }

    #[test]
    fn initialize() {
        let t = make_transport().expect("WHERE X Y\r", ":A 0 0");
        let mut s = LudlXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
        assert!(s.has_property("StepSize"));
        assert!(s.has_property("StepSize-X"));
        assert!(s.has_property("StepSize-Y"));
        assert!(s.has_property("Speed"));
        assert!(s.has_property("StartSpeed"));
        assert!(s.has_property("Acceleration"));
        assert!(!s.has_property("Version"));
    }

    #[test]
    fn move_absolute() {
        let t = make_transport()
            .expect("MOVE X=3000 Y=4000\r", ":A")
            .expect("WHERE X Y\r", ":A 3000 4000");
        let mut s = LudlXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(300.0, 400.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (300.0, 400.0));
    }

    #[test]
    fn move_relative_uses_upstream_movrel() {
        let t = make_transport()
            .expect("MOVREL X=20 Y=-30\r", ":A")
            .expect("WHERE X Y\r", ":A 20 -30");
        let mut s = LudlXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(2.0, -3.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (2.0, -3.0));
    }

    #[test]
    fn error_response_fails() {
        let t = make_transport().any(":N 21");
        let mut s = LudlXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_xy_position_um(99999.0, 0.0).is_err());
    }

    #[test]
    fn parse_xy_ok() {
        let (x, y) = LudlXYStage::parse_xy(":A 1000 -500").unwrap();
        assert!((x - 100.0).abs() < 1e-9);
        assert!((y - (-50.0)).abs() < 1e-9);
    }

    #[test]
    fn step_size_and_limit_behavior_match_upstream_surface() {
        let mut s = LudlXYStage::new();
        s.set_property("StepSize", PropertyValue::Float(0.2))
            .unwrap();
        assert_eq!(s.get_step_size_um(), (0.2, 0.2));
        assert_eq!(s.get_limits_um().unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn set_origin_queries_current_stage_position_like_upstream() {
        let t = make_transport()
            .expect("HERE X=0 Y=0\r", ":A")
            .expect("WHERE X Y\r", ":A 15 -25")
            .expect("WHERE X Y\r", ":A 15 -25");
        let mut s = LudlXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_origin().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (1.5, -2.5));
    }

    #[test]
    fn no_transport_error() {
        assert!(LudlXYStage::new().initialize().is_err());
    }

    #[test]
    fn busy_polls_module_status_like_upstream() {
        let t = make_transport()
            .expect("STATUS S\r", "B")
            .expect("STATUS S\r", "N")
            .expect("STATUS S\r", "?");
        let mut s = LudlXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
        assert!(!s.busy());
        assert!(s.busy());
    }
}
