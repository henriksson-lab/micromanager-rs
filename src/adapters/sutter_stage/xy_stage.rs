/// Sutter Instruments MPC-200 XY stage.
///
/// Protocol identical to Ludl MAC series (TX `\r`, RX `\n`, `:A`/`:N` responses):
///   `VER\r`              → `:A <version>`
///   `MOVE X=<n> Y=<n>\r` → `:A`   (steps, 0.1 µm resolution)
///   `WHERE X Y\r`        → `:A <x> <y>`
///   `HOME X Y\r`         → `:A`
///   `HALT\r`             → `:A`
///   `HERE X=0 Y=0\r`    → `:A`
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

const STEPS_PER_UM: f64 = 10.0;

pub struct SutterXYStage {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl SutterXYStage {
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

    fn check_a(resp: &str) -> MmResult<&str> {
        let s = resp.trim();
        if let Some(rest) = s.strip_prefix(":A") {
            Ok(rest.trim())
        } else {
            Err(MmError::LocallyDefined(format!("Sutter error: {}", s)))
        }
    }

    fn parse_xy(resp: &str) -> MmResult<(f64, f64)> {
        let body = Self::check_a(resp)?;
        let parts: Vec<&str> = body.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(MmError::LocallyDefined(format!(
                "Cannot parse WHERE: {}",
                resp
            )));
        }
        let x: i64 = parts[0].parse().map_err(|_| {
            MmError::LocallyDefined(format!("Cannot parse WHERE X value: {}", resp))
        })?;
        let y: i64 = parts[1].parse().map_err(|_| {
            MmError::LocallyDefined(format!("Cannot parse WHERE Y value: {}", resp))
        })?;
        Ok((x as f64 / STEPS_PER_UM, y as f64 / STEPS_PER_UM))
    }

    fn axis_busy(&self, axis: char) -> bool {
        match self.cmd(&format!("STATUS {}", axis)) {
            Ok(resp) => resp.as_bytes().first() == Some(&b'B'),
            Err(_) => false,
        }
    }

    fn set_high_command_level(&self) -> MmResult<()> {
        self.call_transport(|t| t.send_bytes(&[255, 65]))
    }

    fn parse_first_i64(resp: &str, command: &str) -> MmResult<i64> {
        let body = Self::check_a(resp)?;
        body.split_whitespace()
            .next()
            .ok_or_else(|| MmError::LocallyDefined(format!("Cannot parse {}: {}", command, resp)))?
            .parse()
            .map_err(|_| MmError::LocallyDefined(format!("Cannot parse {}: {}", command, resp)))
    }

    fn step_size_um(&self) -> MmResult<f64> {
        self.props
            .get("StepSize")?
            .as_f64()
            .ok_or(MmError::InvalidPropertyValue)
    }

    fn read_scaled_setting(&self, command: &str) -> MmResult<PropertyValue> {
        let resp = self.cmd(&format!("{} X Y", command))?;
        let pulses = Self::parse_first_i64(&resp, command)?;
        Ok(PropertyValue::Float(pulses as f64 * self.step_size_um()?))
    }

    fn read_acceleration(&self) -> MmResult<PropertyValue> {
        let resp = self.cmd("ACCEL X Y")?;
        Ok(PropertyValue::Integer(Self::parse_first_i64(
            &resp, "ACCEL",
        )?))
    }

    fn set_step_size(&mut self, val: PropertyValue) -> MmResult<()> {
        let step_size = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
        if step_size <= 0.0 {
            return Err(MmError::InvalidPropertyValue);
        }
        self.props.set("StepSize", PropertyValue::Float(step_size))
    }

    fn set_scaled_setting(
        &mut self,
        property: &str,
        command: &str,
        val: PropertyValue,
        min_pulses: i64,
        max_pulses: i64,
    ) -> MmResult<()> {
        let requested = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
        let step_size = self.step_size_um()?;
        let pulses = (requested / step_size).trunc() as i64;
        let pulses = pulses.clamp(min_pulses, max_pulses);
        let resp = self.cmd(&format!("{} X={} Y={}", command, pulses, pulses))?;
        Self::check_a(&resp)?;
        self.props
            .set(property, PropertyValue::Float(pulses as f64 * step_size))
    }

    fn set_acceleration(&mut self, val: PropertyValue) -> MmResult<()> {
        let accel = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
        let accel = accel.clamp(1, 255);
        let resp = self.cmd(&format!("ACCEL X={} Y={}", accel, accel))?;
        Self::check_a(&resp)?;
        self.props
            .set("Acceleration", PropertyValue::Integer(accel))
    }
}

impl Default for SutterXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for SutterXYStage {
    fn name(&self) -> &str {
        "XYStage"
    }
    fn description(&self) -> &str {
        "SutterStage XY stage driver adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.set_high_command_level()?;
        if !self.props.has_property("StepSize") {
            self.props
                .define_property("StepSize", PropertyValue::Float(1.0), false)?;
        }
        if !self.props.has_property("Speed") {
            self.props
                .define_property("Speed", PropertyValue::Float(2500.0), false)?;
        }
        if !self.props.has_property("StartSpeed") {
            self.props
                .define_property("StartSpeed", PropertyValue::Float(500.0), false)?;
        }
        if !self.props.has_property("Acceleration") {
            self.props
                .define_property("Acceleration", PropertyValue::Float(100.0), false)?;
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
            "Speed" => self.read_scaled_setting("SPEED"),
            "StartSpeed" => self.read_scaled_setting("STSPEED"),
            "Acceleration" => self.read_acceleration(),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "StepSize" => self.set_step_size(val),
            "Speed" => self.set_scaled_setting("Speed", "SPEED", val, 85, 276480),
            "StartSpeed" => self.set_scaled_setting("StartSpeed", "STSPEED", val, 1000, 276480),
            "Acceleration" => self.set_acceleration(val),
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
        self.axis_busy('X') || self.axis_busy('Y')
    }
}

impl XYStage for SutterXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let xs = (x * STEPS_PER_UM).round() as i64;
        let ys = (y * STEPS_PER_UM).round() as i64;
        let r = self.cmd(&format!("MOVE X={} Y={}", xs, ys))?;
        Self::check_a(&r)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }
    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        let pos = self.cmd("WHERE X Y")?;
        Self::parse_xy(&pos)
    }
    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let xs = (dx * STEPS_PER_UM).round() as i64;
        let ys = (dy * STEPS_PER_UM).round() as i64;
        let r = self.cmd(&format!("MOVREL X={} Y={}", xs, ys))?;
        Self::check_a(&r)?;
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }
    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("HOME X Y")?;
        Self::check_a(&r)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
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
        (0.1, 0.1)
    }
    fn set_origin(&mut self) -> MmResult<()> {
        let _ = self.cmd("HERE X=0 Y=0");
        self.x_um = 0.0;
        self.y_um = 0.0;
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
        let t = make_transport().expect("WHERE X Y\r", ":A 500 1000");
        let mut s = SutterXYStage::new().with_transport(Box::new(t));
        assert!(!s.has_property("StepSize"));
        assert!(!s.has_property("Speed"));
        assert!(!s.has_property("StartSpeed"));
        assert!(!s.has_property("Acceleration"));
        assert!(!s.has_property("Version"));
        s.initialize().unwrap();
        assert!(s.has_property("StepSize"));
        assert!(s.has_property("Speed"));
        assert!(s.has_property("StartSpeed"));
        assert!(s.has_property("Acceleration"));
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 50.0).abs() < 1e-9);
        assert!((y - 100.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport()
            .any(":A")
            .expect("WHERE X Y\r", ":A 2000 3000");
        let mut s = SutterXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(200.0, 300.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (200.0, 300.0));
    }

    #[test]
    fn no_transport_error() {
        assert!(SutterXYStage::new().initialize().is_err());
    }

    #[test]
    fn limits_are_unsupported() {
        assert_eq!(
            SutterXYStage::new().get_limits_um(),
            Err(MmError::UnsupportedCommand)
        );
    }

    #[test]
    fn relative_move_uses_movrel() {
        let t = make_transport()
            .expect("MOVREL X=20 Y=-30\r", ":A")
            .expect("WHERE X Y\r", ":A 520 970");
        let mut s = SutterXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(2.0, -3.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (52.0, 97.0));
    }

    #[test]
    fn busy_polls_status_for_both_axes() {
        let t = make_transport()
            .expect("STATUS X\r", "N")
            .expect("STATUS Y\r", "B");
        let mut s = SutterXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
    }

    #[test]
    fn malformed_where_errors_instead_of_zeroing_position() {
        let t = make_transport().expect("WHERE X Y\r", ":A bad 1000");
        let mut s = SutterXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.get_xy_position_um().is_err());
    }

    #[test]
    fn speed_properties_use_upstream_controller_commands() {
        let t = make_transport()
            .expect("SPEED X=2500 Y=2500\r", ":A")
            .expect("SPEED X Y\r", ":A 1200 1200")
            .expect("STSPEED X=1000 Y=1000\r", ":A")
            .expect("STSPEED X Y\r", ":A 1000 1000")
            .expect("ACCEL X=255 Y=255\r", ":A")
            .expect("ACCEL X Y\r", ":A 42 42");
        let mut s = SutterXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("StepSize", PropertyValue::Float(0.2))
            .unwrap();
        s.set_property("Speed", PropertyValue::Float(500.0))
            .unwrap();
        assert_eq!(
            s.get_property("Speed").unwrap(),
            PropertyValue::Float(240.0)
        );
        s.set_property("StartSpeed", PropertyValue::Float(10.0))
            .unwrap();
        assert_eq!(
            s.get_property("StartSpeed").unwrap(),
            PropertyValue::Float(200.0)
        );
        s.set_property("Acceleration", PropertyValue::Integer(999))
            .unwrap();
        assert_eq!(
            s.get_property("Acceleration").unwrap(),
            PropertyValue::Integer(42)
        );
    }

    #[test]
    fn invalid_step_size_is_rejected_without_cache_drift() {
        let mut s = SutterXYStage::new();
        s.initialize().err();
        s.props
            .define_property("StepSize", PropertyValue::Float(1.0), false)
            .unwrap();
        assert_eq!(
            s.set_property("StepSize", PropertyValue::Float(0.0)),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            s.get_property("StepSize").unwrap(),
            PropertyValue::Float(1.0)
        );
    }
}
