/// Marzhauser TANGO controller XY-stage.
///
/// Protocol (ASCII, `\r` terminated):
///   `?version\r`      → response contains "TANGO"
///   `!autostatus 0\r` → disable autostatus reports
///   `!dim 1 1\r`      → switch to micrometer mode
///   `!moa <x> <y>\r`  → move to absolute position (µm, space-separated)
///   `!mor <dx> <dy>\r`→ move relative (µm)
///   `?pos\r`          → current position: `<x> <y>` in µm
///   `?statusaxis\r`   → motion status; 'M' in response = moving
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct MarzhauserXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl MarzhauserXYStage {
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
        let cmd = format!("{}\r", command);
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }

    fn check_tango_error(&mut self) -> MmResult<()> {
        let resp = self.cmd("?err")?;
        let err = resp
            .trim()
            .parse::<i32>()
            .map_err(|_| MmError::LocallyDefined(format!("Bad ?err response: {}", resp)))?;
        if err != 0 {
            return Err(MmError::LocallyDefined(format!(
                "Marzhauser controller error: {}",
                err
            )));
        }
        Ok(())
    }

    /// Parse `<x> <y>` (space-separated floats).
    fn parse_pos(resp: &str) -> MmResult<(f64, f64)> {
        let parts: Vec<&str> = resp.trim().split_whitespace().collect();
        if parts.len() < 2 {
            return Err(MmError::LocallyDefined(format!(
                "Cannot parse position: {}",
                resp
            )));
        }
        let x = parts[0]
            .parse::<f64>()
            .map_err(|_| MmError::LocallyDefined(format!("Bad X: {}", parts[0])))?;
        let y = parts[1]
            .parse::<f64>()
            .map_err(|_| MmError::LocallyDefined(format!("Bad Y: {}", parts[1])))?;
        Ok((x, y))
    }
}

impl Default for MarzhauserXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for MarzhauserXYStage {
    fn name(&self) -> &str {
        "XYStage"
    }
    fn description(&self) -> &str {
        "Tango XY stage driver adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        let ver = self.cmd("?version")?;
        if !ver.to_lowercase().contains("tango") {
            return Err(MmError::LocallyDefined(format!(
                "Unexpected controller: {}",
                ver
            )));
        }

        let _ = self.cmd("!autostatus 0");
        let _ = self.cmd("!dim 1 1"); // micrometer mode

        let pos = self.cmd("?pos")?;
        let (x, y) = Self::parse_pos(&pos)?;
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
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        self.props.set(name, val)
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

impl XYStage for MarzhauserXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        self.cmd("!dim 1 1")?;
        let resp = self.cmd(&format!("!moa {:.3} {:.3}", x, y))?;
        if resp.contains('E') {
            return Err(MmError::LocallyDefined(format!(
                "Marzhauser error: {}",
                resp
            )));
        }
        self.check_tango_error()?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        Ok((self.x_um, self.y_um))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        self.cmd("!dim 1 1")?;
        let resp = self.cmd(&format!("!mor {:.3} {:.3}", dx, dy))?;
        if resp.contains('E') {
            return Err(MmError::LocallyDefined(format!(
                "Marzhauser error: {}",
                resp
            )));
        }
        self.check_tango_error()?;
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        let resp = self.cmd("!cal")?;
        if resp.contains('E') {
            return Err(MmError::LocallyDefined(format!(
                "Marzhauser error: {}",
                resp
            )));
        }
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd("a x");
        let _ = self.cmd("a y");
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((-100_000.0, 100_000.0, -100_000.0, 100_000.0))
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        (0.1, 0.1)
    }

    fn set_origin(&mut self) -> MmResult<()> {
        let resp = self.cmd("!pos 0 0")?;
        if resp.contains('E') {
            return Err(MmError::LocallyDefined(format!(
                "Marzhauser error: {}",
                resp
            )));
        }
        self.check_tango_error()?;
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
            .expect("?version\r", "TANGO:v1.2")
            .expect("!autostatus 0\r", "OK")
            .expect("!dim 1 1\r", "OK")
            .expect("?pos\r", "100.000 200.000")
    }

    #[test]
    fn initialize() {
        let mut stage = MarzhauserXYStage::new().with_transport(Box::new(make_transport()));
        stage.initialize().unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (100.0, 200.0));
    }

    #[test]
    fn move_absolute() {
        let t = make_transport()
            .expect("!dim 1 1\r", "OK")
            .expect("!moa 300.000 400.000\r", "OK")
            .expect("?err\r", "0");
        let mut stage = MarzhauserXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_xy_position_um(300.0, 400.0).unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (300.0, 400.0));
    }

    #[test]
    fn move_relative() {
        let t = make_transport()
            .expect("!dim 1 1\r", "OK")
            .expect("!mor 10.000 20.000\r", "OK")
            .expect("?err\r", "0");
        let mut stage = MarzhauserXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_relative_xy_position_um(10.0, 20.0).unwrap();
        let (x, y) = stage.get_xy_position_um().unwrap();
        assert!((x - 110.0).abs() < 1e-9);
        assert!((y - 220.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute_queries_tango_error_before_cache_update() {
        let t = make_transport()
            .expect("!dim 1 1\r", "OK")
            .expect("!moa 300.000 400.000\r", "OK")
            .expect("?err\r", "7");
        let mut stage = MarzhauserXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();

        assert!(stage.set_xy_position_um(300.0, 400.0).is_err());
        assert_eq!(stage.get_xy_position_um().unwrap(), (100.0, 200.0));
    }

    #[test]
    fn set_origin_sends_controller_origin_and_checks_error() {
        let t = make_transport()
            .expect("!pos 0 0\r", "OK")
            .expect("?err\r", "0");
        let mut stage = MarzhauserXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();

        stage.set_origin().unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn wrong_controller_rejected() {
        let t = MockTransport::new().expect("?version\r", "Unknown v1.0");
        let mut stage = MarzhauserXYStage::new().with_transport(Box::new(t));
        assert!(stage.initialize().is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(MarzhauserXYStage::new().initialize().is_err());
    }
}
