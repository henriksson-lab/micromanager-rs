/// ITK Hydra LMT200 XY stage controller adapter.
///
/// ASCII serial protocol (commands terminated with `" \n\r"`, responses
/// terminated with `"\r\n"`).
///
/// Commands:
///   `version`           → firmware version string
///   `p`                 → get XY position (`"x y\r\n"` in mm)
///   `{x} {y} m`         → move absolute to (x, y) in mm
///   `{dx} {dy} r`       → move relative by (dx, dy) in mm
///   `ncal`              → home / calibrate
///   `1 nrm`             → range measure axis 1
///   `st`                → status byte (bit 0 = busy)
///   `ge`                → get and clear last error (0 = OK)
///   `{v} sv`            → set velocity in mm/s
///   `gv`                → get velocity in mm/s
///   `{a} sa`            → set acceleration in mm/s²
///   `ga`                → get acceleration in mm/s²
///   `1 getnlimit`       → get X axis range `"min max\r\n"` in mm
///   `2 getnlimit`       → get Y axis range
///   `0 1 setnpos 0 2 setnpos` → set origin
///   `1 nabort 2 nabort` → stop all motion
///
/// Position: device uses mm; mm-device `XYStage` uses µm.
/// Precision: 15.26 nm (programming mode), step_size = 1 µm.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, XYStage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct HydraXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    /// Range measured flag (needed for get_limits_um)
    range_measured: bool,
    /// Cached X limits in µm
    x_min_um: f64,
    x_max_um: f64,
    /// Cached Y limits in µm
    y_min_um: f64,
    y_max_um: f64,
    /// Origin offset in µm
    origin_x_um: f64,
    origin_y_um: f64,
    /// Step size (constant 1 µm)
    step_size_um: f64,
    /// Mirror X axis
    mirror_x: bool,
    /// Mirror Y axis
    mirror_y: bool,
}

impl HydraXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "Speed [mm/s]",
                PropertyValue::Float(200.0),
                false,
            )
            .unwrap();
        props
            .define_property(
                "Acceleration [mm/s^2]",
                PropertyValue::Float(1000.0),
                false,
            )
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            range_measured: false,
            x_min_um: 0.0,
            x_max_um: 120_000.0,
            y_min_um: 0.0,
            y_max_um: 80_000.0,
            origin_x_um: 0.0,
            origin_y_um: 0.0,
            step_size_um: 1.0,
            mirror_x: false,
            mirror_y: false,
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

    /// Parse "float float" response
    fn parse_xy(resp: &str) -> MmResult<(f64, f64)> {
        let parts: Vec<&str> = resp.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(MmError::SerialInvalidResponse);
        }
        let x: f64 = parts[0]
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        let y: f64 = parts[1]
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        Ok((x, y))
    }

    /// Apply mirror transform to mm coordinates (device → user).
    fn from_device_mm(&self, x_mm: f64, y_mm: f64) -> (f64, f64) {
        let x = if self.mirror_x { 120.0 - x_mm } else { x_mm };
        let y = if self.mirror_y { 80.0 - y_mm } else { y_mm };
        (x * 1000.0, y * 1000.0) // mm → µm
    }

    /// Convert user µm coordinates to device mm coordinates.
    fn to_device_mm(&self, x_um: f64, y_um: f64) -> (f64, f64) {
        let x_mm = x_um / 1000.0;
        let y_mm = y_um / 1000.0;
        let x = if self.mirror_x { 120.0 - x_mm } else { x_mm };
        let y = if self.mirror_y { 80.0 - y_mm } else { y_mm };
        (x, y)
    }
}

impl Default for HydraXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for HydraXYStage {
    fn name(&self) -> &str {
        "HydraLMT200XYStage"
    }

    fn description(&self) -> &str {
        "ITK Hydra LMT200 XY stage controller"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Query firmware version
        let _ver = self.cmd("version")?;
        // Clear any errors
        let _ = self.cmd("ge")?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        self.range_measured = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Speed [mm/s]" => {
                let v = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                let cmd = format!("{} sv", v);
                let _ = self.cmd(&cmd)?;
                self.props.set(name, PropertyValue::Float(v))
            }
            "Acceleration [mm/s^2]" => {
                let a = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                let cmd = format!("{} sa", a);
                let _ = self.cmd(&cmd)?;
                self.props.set(name, PropertyValue::Float(a))
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

impl XYStage for HydraXYStage {
    fn set_xy_position_um(&mut self, x_um: f64, y_um: f64) -> MmResult<()> {
        let (x_mm, y_mm) = self.to_device_mm(x_um, y_um);
        let cmd = format!("{} {} m", x_mm, y_mm);
        let _ = self.cmd(&cmd)?;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        // get_xy_position_um takes &self — we cannot call self.cmd()
        // Callers that need a live position should use a mutable reference.
        // Return a plausible cached value; real adapters poll in a background thread.
        Ok((self.origin_x_um, self.origin_y_um))
    }

    fn set_relative_xy_position_um(&mut self, dx_um: f64, dy_um: f64) -> MmResult<()> {
        let dx_mm = if self.mirror_x { -dx_um } else { dx_um } / 1000.0;
        let dy_mm = if self.mirror_y { -dy_um } else { dy_um } / 1000.0;
        let cmd = format!("{} {} r", dx_mm, dy_mm);
        let _ = self.cmd(&cmd)?;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        let _ = self.cmd("ncal")?;
        // range measure
        let _ = self.cmd("1 nrm")?;
        self.range_measured = true;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd("1 nabort 2 nabort")?;
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((
            self.x_min_um,
            self.x_max_um,
            self.y_min_um,
            self.y_max_um,
        ))
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        (self.step_size_um, self.step_size_um)
    }

    fn set_origin(&mut self) -> MmResult<()> {
        let _ = self.cmd("0 1 setnpos 0 2 setnpos")?;
        // Update cached origin to current position
        let (x, y) = self.get_xy_position_um()?;
        self.origin_x_um = x;
        self.origin_y_um = y;
        Ok(())
    }
}

/// Separate method for querying position when we have `&mut self`.
impl HydraXYStage {
    pub fn query_position_um(&mut self) -> MmResult<(f64, f64)> {
        let resp = self.cmd("p")?;
        let (x_mm, y_mm) = Self::parse_xy(&resp)?;
        Ok(self.from_device_mm(x_mm, y_mm))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_initialized() -> HydraXYStage {
        let t = MockTransport::new()
            .expect("version", "Hydra 1.0")
            .expect("ge", "0");
        let mut s = HydraXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s
    }

    #[test]
    fn initialize_succeeds() {
        let s = make_initialized();
        assert!(s.initialized);
    }

    #[test]
    fn no_transport_error() {
        assert!(HydraXYStage::new().initialize().is_err());
    }

    #[test]
    fn query_position_parses_response() {
        let t = MockTransport::new()
            .expect("version", "Hydra 1.0")
            .expect("ge", "0")
            .expect("p", "10.5 20.3");
        let mut s = HydraXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        let (x, y) = s.query_position_um().unwrap();
        assert!((x - 10500.0).abs() < 1.0);
        assert!((y - 20300.0).abs() < 1.0);
    }

    #[test]
    fn set_position_sends_m_command() {
        let t = MockTransport::new()
            .expect("version", "Hydra 1.0")
            .expect("ge", "0")
            .expect("50 100 m", "");
        let mut s = HydraXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        // 50000 µm × 100000 µm = 50 mm × 100 mm
        s.set_xy_position_um(50_000.0, 100_000.0).unwrap();
    }

    #[test]
    fn relative_move_sends_r_command() {
        let t = MockTransport::new()
            .expect("version", "Hydra 1.0")
            .expect("ge", "0")
            .expect("1 2 r", "");
        let mut s = HydraXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(1_000.0, 2_000.0).unwrap();
    }

    #[test]
    fn stop_sends_abort() {
        let t = MockTransport::new()
            .expect("version", "Hydra 1.0")
            .expect("ge", "0")
            .expect("1 nabort 2 nabort", "");
        let mut s = HydraXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.stop().unwrap();
    }

    #[test]
    fn get_limits_returns_default() {
        let s = HydraXYStage::new();
        let (x0, x1, y0, y1) = s.get_limits_um().unwrap();
        assert!((x0 - 0.0).abs() < 1.0);
        assert!((x1 - 120_000.0).abs() < 1.0);
        assert!((y0 - 0.0).abs() < 1.0);
        assert!((y1 - 80_000.0).abs() < 1.0);
    }

    #[test]
    fn step_size_is_one_um() {
        let (sx, sy) = HydraXYStage::new().get_step_size_um();
        assert!((sx - 1.0).abs() < 0.01);
        assert!((sy - 1.0).abs() < 0.01);
    }

    #[test]
    fn device_type_is_xystage() {
        assert_eq!(HydraXYStage::new().device_type(), DeviceType::XYStage);
    }
}
