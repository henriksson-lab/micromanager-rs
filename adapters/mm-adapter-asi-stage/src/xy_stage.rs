/// ASI XY-stage (Applied Scientific Instrumentation).
///
/// Protocol (ASCII, `\r` terminated):
///   `M X=<x> Y=<y>\r` → move to absolute position (tenths of microns)
///                        response: `:A\r` or `:N<code>\r`
///   `W X Y\r`          → query position; response `:A X=<x> Y=<y>\r`
///   `R X=<dx> Y=<dy>\r`→ relative move
///   `Z\r`              → home (zero) all axes
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, XYStage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const UNITS_PER_UM: f64 = 10.0;

pub struct AsiXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl AsiXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();

        Self { props, transport: None, initialized: false, x_um: 0.0, y_um: 0.0 }
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
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }

    fn check_response(resp: &str) -> MmResult<()> {
        if resp.starts_with(":N") {
            return Err(MmError::LocallyDefined(format!("ASI error: {}", resp)));
        }
        Ok(())
    }

    /// Parse `:A X=<x> Y=<y>` → (x_um, y_um).
    fn parse_xy(resp: &str) -> MmResult<(f64, f64)> {
        let resp = resp.trim();
        let mut x = None;
        let mut y = None;
        for token in resp.split_whitespace() {
            if let Some(v) = token.strip_prefix("X=") {
                x = v.parse::<f64>().ok();
            }
            if let Some(v) = token.strip_prefix("Y=") {
                y = v.parse::<f64>().ok();
            }
        }
        match (x, y) {
            (Some(xv), Some(yv)) => Ok((xv / UNITS_PER_UM, yv / UNITS_PER_UM)),
            _ => Err(MmError::LocallyDefined(format!("Cannot parse XY: {}", resp))),
        }
    }
}

impl Default for AsiXYStage {
    fn default() -> Self { Self::new() }
}

impl Device for AsiXYStage {
    fn name(&self) -> &str { "ASI-XYStage" }
    fn description(&self) -> &str { "ASI XY-stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let resp = self.cmd("W X Y")?;
        let (x, y) = Self::parse_xy(&resp)?;
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

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::XYStage }
    fn busy(&self) -> bool { false }
}

impl XYStage for AsiXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let xu = (x * UNITS_PER_UM).round() as i64;
        let yu = (y * UNITS_PER_UM).round() as i64;
        let resp = self.cmd(&format!("M X={} Y={}", xu, yu))?;
        Self::check_response(&resp)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> { Ok((self.x_um, self.y_um)) }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let xu = (dx * UNITS_PER_UM).round() as i64;
        let yu = (dy * UNITS_PER_UM).round() as i64;
        let resp = self.cmd(&format!("R X={} Y={}", xu, yu))?;
        Self::check_response(&resp)?;
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        let resp = self.cmd("! X Y")?;
        Self::check_response(&resp)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd("\\");
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((-100_000.0, 100_000.0, -100_000.0, 100_000.0))
    }

    fn get_step_size_um(&self) -> (f64, f64) { (0.1, 0.1) }

    fn set_origin(&mut self) -> MmResult<()> {
        let resp = self.cmd("Z")?;
        Self::check_response(&resp)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn initialize_reads_position() {
        let t = MockTransport::new().expect("W X Y", ":A X=1000 Y=2000");
        let mut stage = AsiXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (100.0, 200.0));
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new()
            .expect("W X Y", ":A X=0 Y=0")
            .expect("M X=1500 Y=2500", ":A");
        let mut stage = AsiXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_xy_position_um(150.0, 250.0).unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (150.0, 250.0));
    }

    #[test]
    fn move_relative() {
        let t = MockTransport::new()
            .expect("W X Y", ":A X=0 Y=0")
            .expect("R X=100 Y=200", ":A");
        let mut stage = AsiXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_relative_xy_position_um(10.0, 20.0).unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (10.0, 20.0));
    }

    #[test]
    fn no_transport_error() {
        assert!(AsiXYStage::new().initialize().is_err());
    }
}
