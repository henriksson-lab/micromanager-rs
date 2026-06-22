/// Prior Scientific ProScan XY stage.
///
/// Protocol (TX `\r`, RX `\r`):
///   `G,x,y\r`      → absolute move (steps); response `R\r` or `E<code>\r`
///   `GR,dx,dy\r`   → relative move (steps); same response
///   `PX\r`         → X position in steps
///   `PY\r`         → Y position in steps
///   `SIS\r`        → home (Set Initial Stage position)
///   `K\r`          → halt
///   `$\r`          → status byte (bit 0 = X busy, bit 1 = Y busy)
///
/// Step size: 0.1 µm / step (10 steps per µm).
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

const DEFAULT_STEPS_PER_UM: f64 = 10.0;

pub struct PriorXYStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    steps_per_um_x: f64,
    steps_per_um_y: f64,
    x_um: f64,
    y_um: f64,
}

impl PriorXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            steps_per_um_x: DEFAULT_STEPS_PER_UM,
            steps_per_um_y: DEFAULT_STEPS_PER_UM,
            x_um: 0.0,
            y_um: 0.0,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = RefCell::new(Some(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.borrow_mut().as_mut() {
            Some(t) => f(t.as_mut()),
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

    fn clear_port(&self) -> MmResult<()> {
        self.call_transport(|t| t.purge())
    }

    fn check_r(resp: &str) -> MmResult<()> {
        let s = resp.trim();
        if s == "R" {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("Prior error: {}", s)))
        }
    }

    fn check_zero(resp: &str, context: &str) -> MmResult<()> {
        let s = resp.trim();
        if s.starts_with('0') {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!(
                "Prior {} error: {}",
                context, s
            )))
        }
    }

    fn query_bounded_i64_property(&self, command: &str) -> MmResult<PropertyValue> {
        self.clear_port()?;
        let value = self
            .cmd(command)?
            .trim()
            .parse::<i64>()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        if !(1..=100).contains(&value) {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(PropertyValue::Integer(value))
    }

    fn ensure_runtime_properties(&mut self) -> MmResult<()> {
        if !self.props.has_property("StepSizeX_um") {
            self.props
                .define_property("StepSizeX_um", PropertyValue::Float(0.1), true)?;
            self.props
                .define_property("StepSizeY_um", PropertyValue::Float(0.1), true)?;
            self.props
                .define_property("MaxSpeed", PropertyValue::Integer(20), false)?;
            self.props.set_property_limits("MaxSpeed", 1.0, 100.0)?;
            self.props
                .define_property("Acceleration", PropertyValue::Integer(20), false)?;
            self.props.set_property_limits("Acceleration", 1.0, 100.0)?;
            self.props
                .define_property("SCurve", PropertyValue::Integer(20), false)?;
            self.props.set_property_limits("SCurve", 1.0, 100.0)?;
        }
        Ok(())
    }

    fn read_xy(&self) -> MmResult<(f64, f64)> {
        let rx = self.cmd("PX")?;
        let ry = self.cmd("PY")?;
        let xs: i64 = rx.trim().parse().unwrap_or(0);
        let ys: i64 = ry.trim().parse().unwrap_or(0);
        Ok((
            xs as f64 / self.steps_per_um_x,
            ys as f64 / self.steps_per_um_y,
        ))
    }

    fn discover_resolution(&mut self) {
        if let Ok(resp) = self.cmd("RES,s") {
            if let Ok(res) = resp.trim().parse::<f64>() {
                if res > 0.0 {
                    self.steps_per_um_x = 1.0 / res;
                    self.steps_per_um_y = 1.0 / res;
                    if let Some(e) = self.props.entry_mut("StepSizeX_um") {
                        e.value = PropertyValue::Float(res);
                    }
                    if let Some(e) = self.props.entry_mut("StepSizeY_um") {
                        e.value = PropertyValue::Float(res);
                    }
                }
            }
        }
    }
}

impl Default for PriorXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for PriorXYStage {
    fn name(&self) -> &str {
        "PriorXYStage"
    }
    fn description(&self) -> &str {
        "Prior Scientific ProScan XY stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        self.clear_port()?;
        self.cmd("COMP 0")?;
        self.ensure_runtime_properties()?;
        self.discover_resolution();
        let (x, y) = self.read_xy()?;
        self.x_um = x;
        self.y_um = y;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if self.initialized {
            match name {
                "MaxSpeed" => return self.query_bounded_i64_property("SMS"),
                "Acceleration" => return self.query_bounded_i64_property("SAS"),
                "SCurve" => return self.query_bounded_i64_property("SCS"),
                _ => {}
            }
        }
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "MaxSpeed" if self.initialized => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.clear_port()?;
                Self::check_zero(&self.cmd(&format!("SMS,{}", v))?, name)?;
                self.props.set(name, PropertyValue::Integer(v))
            }
            "Acceleration" if self.initialized => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.clear_port()?;
                Self::check_zero(&self.cmd(&format!("SAS,{}", v))?, name)?;
                self.props.set(name, PropertyValue::Integer(v))
            }
            "SCurve" if self.initialized => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.clear_port()?;
                Self::check_zero(&self.cmd(&format!("SCS,{}", v))?, name)?;
                self.props.set(name, PropertyValue::Integer(v))
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
        self.cmd("$")
            .ok()
            .and_then(|s| s.trim().parse::<u8>().ok())
            .map(|status| status & 0x03 != 0)
            .unwrap_or(false)
    }
}

impl XYStage for PriorXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let xs = (x * self.steps_per_um_x).round() as i64;
        let ys = (y * self.steps_per_um_y).round() as i64;
        self.clear_port()?;
        let r = self.cmd(&format!("G,{},{}", xs, ys))?;
        Self::check_r(&r)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }
    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        self.read_xy()
    }
    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let dxs = (dx * self.steps_per_um_x).round() as i64;
        let dys = (dy * self.steps_per_um_y).round() as i64;
        self.clear_port()?;
        let r = self.cmd(&format!("GR,{},{}", dxs, dys))?;
        Self::check_r(&r)?;
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }
    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("SIS")?;
        Self::check_r(&r)?;
        let r = self.cmd("SIS")?;
        Self::check_r(&r)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> {
        let r = self.cmd("K")?;
        Self::check_r(&r)
    }
    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Err(MmError::UnsupportedCommand)
    }
    fn get_step_size_um(&self) -> (f64, f64) {
        (1.0 / self.steps_per_um_x, 1.0 / self.steps_per_um_y)
    }
    fn set_origin(&mut self) -> MmResult<()> {
        let r = self.cmd("PS,0,0")?;
        if !r.starts_with('0') {
            return Err(MmError::LocallyDefined(format!(
                "Prior set-origin error: {}",
                r
            )));
        }
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
            .expect("COMP 0\r", "0")
            .expect("RES,s\r", "0.1")
            .expect("PX\r", "1000") // PX -> 100 µm
            .expect("PY\r", "2000") // PY -> 200 µm
    }

    #[test]
    fn initialize() {
        let t = make_transport()
            .expect("PX\r", "1000")
            .expect("PY\r", "2000");
        let mut s = PriorXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 100.0).abs() < 1e-9);
        assert!((y - 200.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport()
            .expect("G,5000,6000\r", "R")
            .expect("PX\r", "5000")
            .expect("PY\r", "6000");
        let mut s = PriorXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(500.0, 600.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (500.0, 600.0));
    }

    #[test]
    fn move_relative() {
        let t = make_transport()
            .expect("GR,500,750\r", "R")
            .expect("PX\r", "1500")
            .expect("PY\r", "2750");
        let mut s = PriorXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(50.0, 75.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 150.0).abs() < 1e-9);
        assert!((y - 275.0).abs() < 1e-9);
    }

    #[test]
    fn error_response_fails() {
        let t = make_transport().any("E8");
        let mut s = PriorXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_xy_position_um(9999.0, 0.0).is_err());
    }

    #[test]
    fn home() {
        let t = make_transport().expect("SIS\r", "R").expect("SIS\r", "R");
        let t = t.expect("PX\r", "0").expect("PY\r", "0");
        let mut s = PriorXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn stop_checks_ack_and_origin_sends_command() {
        let t = make_transport()
            .expect("K\r", "R")
            .expect("PS,0,0\r", "0")
            .expect("PX\r", "0")
            .expect("PY\r", "0");
        let mut s = PriorXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.stop().unwrap();
        s.set_origin().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn limits_are_unsupported_like_upstream() {
        let s = PriorXYStage::new();
        assert_eq!(s.get_limits_um().unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn initialize_creates_runtime_properties_like_upstream() {
        let mut s = PriorXYStage::new().with_transport(Box::new(make_transport()));
        assert!(!s.has_property("Version"));
        assert!(!s.has_property("StepSizeX_um"));
        assert!(!s.has_property("StepSizeY_um"));
        assert!(!s.has_property("MaxSpeed"));
        assert!(!s.has_property("Acceleration"));
        assert!(!s.has_property("SCurve"));

        s.initialize().unwrap();

        assert!(!s.has_property("Version"));
        assert!(s.has_property("StepSizeX_um"));
        assert!(s.has_property("StepSizeY_um"));
        assert!(s.has_property("MaxSpeed"));
        assert!(s.has_property("Acceleration"));
        assert!(s.has_property("SCurve"));
    }

    #[test]
    fn motion_properties_use_zero_ack_and_live_reads() {
        let t = make_transport()
            .expect("SMS,42\r", "0")
            .expect("SMS\r", "42")
            .expect("SAS,43\r", "0")
            .expect("SAS\r", "43")
            .expect("SCS,44\r", "0")
            .expect("SCS\r", "44");
        let mut s = PriorXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("MaxSpeed", PropertyValue::Integer(42))
            .unwrap();
        assert_eq!(
            s.get_property("MaxSpeed").unwrap(),
            PropertyValue::Integer(42)
        );
        s.set_property("Acceleration", PropertyValue::Integer(43))
            .unwrap();
        assert_eq!(
            s.get_property("Acceleration").unwrap(),
            PropertyValue::Integer(43)
        );
        s.set_property("SCurve", PropertyValue::Integer(44))
            .unwrap();
        assert_eq!(
            s.get_property("SCurve").unwrap(),
            PropertyValue::Integer(44)
        );
    }

    #[test]
    fn failed_motion_property_write_preserves_cached_value() {
        let t = make_transport().expect("SMS,42\r", "E8");
        let mut s = PriorXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();

        assert!(s
            .set_property("MaxSpeed", PropertyValue::Integer(42))
            .is_err());
        s.shutdown().unwrap();
        assert_eq!(
            s.get_property("MaxSpeed").unwrap(),
            PropertyValue::Integer(20)
        );
    }

    #[test]
    fn no_transport_error() {
        assert!(PriorXYStage::new().initialize().is_err());
    }
}
