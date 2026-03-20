/// ASI Tiger Controller — XY stage.
///
/// Protocol (TX `\r`, RX `\r\n`):
///   Init:
///     `0 V\r`        → `:A v<version>\r\n`   firmware version
///     `VB F=0\r`     → `:A \r\n`             set reply syntax (required)
///   Move:
///     `M X=<x> Y=<y>\r` → `:A \r\n`          absolute move (units = 1/10 µm)
///     `R X=<dx> Y=<dy>\r`→ `:A \r\n`         relative move
///   Query:
///     `W X\r`        → `:A X=<x>\r\n`         X position
///     `W Y\r`        → `:A Y=<y>\r\n`         Y position
///   Halt:
///     `/ \r`         → `:A \r\n`              halt all axes
///   Home (firmware 2.7+):
///     `HM X+ Y+\r`   → `:A \r\n`              home X and Y
///
/// Position units: 10 units per µm (= tenths of µm). Same as ASI MS-series.
/// Responses: `:A` = success, `:N-<code>` = error.
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, XYStage};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

const UNITS_PER_UM: f64 = 10.0;

pub struct AsiTigerXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl AsiTigerXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("FirmwareVersion", PropertyValue::String(String::new()), true).unwrap();
        Self { props, transport: None, initialized: false, x_um: 0.0, y_um: 0.0 }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where F: FnOnce(&mut dyn Transport) -> MmResult<R> {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    fn cmd_ok(&mut self, command: &str) -> MmResult<String> {
        let resp = self.cmd(command)?;
        if resp.starts_with(":N") {
            Err(MmError::LocallyDefined(format!("ASI Tiger error: {}", resp)))
        } else {
            Ok(resp)
        }
    }

    fn parse_axis_value(resp: &str, axis: char) -> Option<f64> {
        // ":A X=-12345.67" or ":A X=-12345"
        let key = format!("{}=", axis);
        resp.split_whitespace()
            .find(|s| s.starts_with(&key))
            .and_then(|s| s[key.len()..].parse::<f64>().ok())
    }
}

impl Default for AsiTigerXYStage { fn default() -> Self { Self::new() } }

impl Device for AsiTigerXYStage {
    fn name(&self) -> &str { "AsiTigerXYStage" }
    fn description(&self) -> &str { "ASI Tiger XY Stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Get firmware version
        let ver_resp = self.cmd_ok("0 V")?;
        let ver = ver_resp.trim_start_matches(":A").trim().trim_start_matches('v').to_string();
        self.props.entry_mut("FirmwareVersion").map(|e| e.value = PropertyValue::String(ver));
        // Set reply syntax to simple mode
        self.cmd_ok("VB F=0")?;
        // Query current positions
        let rx = self.cmd_ok("W X")?;
        let ry = self.cmd_ok("W Y")?;
        self.x_um = Self::parse_axis_value(&rx, 'X').map(|v| v / UNITS_PER_UM).unwrap_or(0.0);
        self.y_um = Self::parse_axis_value(&ry, 'Y').map(|v| v / UNITS_PER_UM).unwrap_or(0.0);
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> { self.props.set(name, val) }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::XYStage }
    fn busy(&self) -> bool { false }
}

impl XYStage for AsiTigerXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let cmd = format!("M X={:.0} Y={:.0}", x * UNITS_PER_UM, y * UNITS_PER_UM);
        self.cmd_ok(&cmd)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> { Ok((self.x_um, self.y_um)) }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let cmd = format!("R X={:.0} Y={:.0}", dx * UNITS_PER_UM, dy * UNITS_PER_UM);
        self.cmd_ok(&cmd)?;
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        self.cmd_ok("HM X+ Y+")?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> { self.cmd_ok("/ ")?; Ok(()) }

    fn set_origin(&mut self) -> MmResult<()> { self.cmd_ok("Z")?; Ok(()) }

    fn get_step_size_um(&self) -> (f64, f64) { (1.0 / UNITS_PER_UM, 1.0 / UNITS_PER_UM) }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((-50000.0, 50000.0, -50000.0, 50000.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect("0 V\r", ":A v3.01")
            .expect("VB F=0\r", ":A")
            .expect("W X\r", ":A X=0")
            .expect("W Y\r", ":A Y=0")
    }

    #[test]
    fn initialize() {
        let mut s = AsiTigerXYStage::new().with_transport(Box::new(make_init_transport()));
        s.initialize().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn move_absolute() {
        let t = make_init_transport()
            .expect("M X=1000 Y=2000\r", ":A");
        let mut s = AsiTigerXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(100.0, 200.0).unwrap(); // 100µm → 1000, 200µm → 2000
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 100.0).abs() < 0.01);
        assert!((y - 200.0).abs() < 0.01);
    }

    #[test]
    fn move_relative() {
        let t = make_init_transport()
            .expect("R X=500 Y=-500\r", ":A");
        let mut s = AsiTigerXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(50.0, -50.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 50.0).abs() < 0.01);
        assert!((y + 50.0).abs() < 0.01);
    }

    #[test]
    fn home() {
        let t = make_init_transport()
            .expect("HM X+ Y+\r", ":A");
        let mut s = AsiTigerXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn no_transport_error() { assert!(AsiTigerXYStage::new().initialize().is_err()); }
}
