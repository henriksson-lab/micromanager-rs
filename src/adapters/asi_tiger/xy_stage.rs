/// ASI Tiger Controller — XY stage.
///
/// Protocol (TX `\r`, RX `\r\n`):
///   Init:
///     `0 V\r`        → `:A v<version>\r\n`   firmware version
///     `UM X?\r`      → `:A X=<u>\r\n`        units per mm; converted to units/um
///     `UM Y?\r`      → `:A Y=<u>\r\n`        units per mm; converted to units/um
///     `VB Z=1\r`     → `:A \r\n`             return positions with one decimal
///   Move:
///     `M X=<x> Y=<y>\r` → `:A \r\n`          absolute move (units = 1/10 µm)
///     `R X=<dx> Y=<dy>\r`→ `:A \r\n`         relative move, omitting zero axes
///   Query:
///     `W X\r`        → `:A X=<x>\r\n`         X position
///     `W Y\r`        → `:A Y=<y>\r\n`         Y position
///   Halt:
///     `HALT\r`       → `:A \r\n`              halt stage card
///     `! X Y\r`      → `:A \r\n`              home X and Y
///     `H X=0 Y=0\r`  → `:A \r\n`              set origin
///
/// Position units are queried from the controller's per-axis UM settings.
/// Responses: `:A` = success, `:N-<code>` = error.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

const DEFAULT_UNITS_PER_UM: f64 = 10.0;

pub struct AsiTigerXYStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    unit_mult_x: f64,
    unit_mult_y: f64,
    x_um: f64,
    y_um: f64,
}

impl AsiTigerXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_property(
                "FirmwareVersion",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("FirmwareBuild", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("HexAddress", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("AxisDirectionX", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .set_allowed_values("AxisDirectionX", &["1", "-1"])
            .unwrap();
        props
            .define_property("AxisDirectionY", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .set_allowed_values("AxisDirectionY", &["1", "-1"])
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            unit_mult_x: DEFAULT_UNITS_PER_UM,
            unit_mult_y: DEFAULT_UNITS_PER_UM,
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
        let full = format!("{}\r", command);
        self.call_transport(|t| {
            t.purge()?;
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    fn cmd_ok(&self, command: &str) -> MmResult<String> {
        let resp = self.cmd(command)?;
        if resp.starts_with(":N") {
            Err(MmError::LocallyDefined(format!(
                "ASI Tiger error: {}",
                resp
            )))
        } else if !resp.starts_with(":A") {
            Err(MmError::SerialInvalidResponse)
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

    fn parse_required_axis_value(resp: &str, axis: char) -> MmResult<f64> {
        Self::parse_axis_value(resp, axis).ok_or_else(|| {
            MmError::LocallyDefined(format!("ASI Tiger response lacks {} value: {}", axis, resp))
        })
    }

    fn format_units(value: f64) -> String {
        format!("{}", value)
    }
}

impl Default for AsiTigerXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for AsiTigerXYStage {
    fn name(&self) -> &str {
        "AsiTigerXYStage"
    }
    fn description(&self) -> &str {
        "ASI Tiger XY Stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        // Get firmware version
        let ver_resp = self.cmd_ok("0 V")?;
        let ver = ver_resp
            .trim_start_matches(":A")
            .trim()
            .trim_start_matches('v')
            .to_string();
        self.props
            .entry_mut("FirmwareVersion")
            .map(|e| e.value = PropertyValue::String(ver));
        let unit_x = self.cmd_ok("UM X?")?;
        self.unit_mult_x = Self::parse_axis_value(&unit_x, 'X').ok_or_else(|| {
            MmError::LocallyDefined(format!("ASI Tiger response lacks X UM: {}", unit_x))
        })? / 1000.0;
        let unit_y = self.cmd_ok("UM Y?")?;
        self.unit_mult_y = Self::parse_axis_value(&unit_y, 'Y').ok_or_else(|| {
            MmError::LocallyDefined(format!("ASI Tiger response lacks Y UM: {}", unit_y))
        })? / 1000.0;
        // Set controller card to return positions with one decimal place.
        self.cmd_ok("VB Z=1")?;
        // Query current positions
        let rx = self.cmd_ok("W X")?;
        let ry = self.cmd_ok("W Y")?;
        self.x_um = Self::parse_axis_value(&rx, 'X')
            .map(|v| v / self.unit_mult_x)
            .unwrap_or(0.0);
        self.y_um = Self::parse_axis_value(&ry, 'Y')
            .map(|v| v / self.unit_mult_y)
            .unwrap_or(0.0);
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
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
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
        match self.cmd_ok("RS X?") {
            Ok(resp) if resp.contains('B') => true,
            Ok(_) => self
                .cmd_ok("RS Y?")
                .map(|resp| resp.contains('B'))
                .unwrap_or(false),
            Err(_) => false,
        }
    }
}

impl XYStage for AsiTigerXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let cmd = format!(
            "M X={} Y={}",
            Self::format_units(x * self.unit_mult_x),
            Self::format_units(y * self.unit_mult_y)
        );
        self.cmd_ok(&cmd)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        let rx = self.cmd_ok("W X")?;
        let ry = self.cmd_ok("W Y")?;
        Ok((
            Self::parse_axis_value(&rx, 'X')
                .map(|v| v / self.unit_mult_x)
                .unwrap_or(self.x_um),
            Self::parse_axis_value(&ry, 'Y')
                .map(|v| v / self.unit_mult_y)
                .unwrap_or(self.y_um),
        ))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let cmd = if dx == 0.0 && dy != 0.0 {
            format!("R Y={}", Self::format_units(dy * self.unit_mult_y))
        } else if dx != 0.0 && dy == 0.0 {
            format!("R X={}", Self::format_units(dx * self.unit_mult_x))
        } else {
            format!(
                "R X={} Y={}",
                Self::format_units(dx * self.unit_mult_x),
                Self::format_units(dy * self.unit_mult_y)
            )
        };
        self.cmd_ok(&cmd)?;
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        self.cmd_ok("! X Y")?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        self.cmd_ok("HALT")?;
        Ok(())
    }

    fn set_origin(&mut self) -> MmResult<()> {
        self.cmd_ok("H X=0 Y=0")?;
        Ok(())
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        (0.001, 0.001)
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        let x_min = Self::parse_required_axis_value(&self.cmd_ok("SL X?")?, 'X')? * 1000.0;
        let x_max = Self::parse_required_axis_value(&self.cmd_ok("SU X?")?, 'X')? * 1000.0;
        let y_min = Self::parse_required_axis_value(&self.cmd_ok("SL Y?")?, 'Y')? * 1000.0;
        let y_max = Self::parse_required_axis_value(&self.cmd_ok("SU Y?")?, 'Y')? * 1000.0;
        Ok((x_min, x_max, y_min, y_max))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect("0 V\r", ":A v3.01")
            .expect("UM X?\r", ":A X=10000")
            .expect("UM Y?\r", ":A Y=10000")
            .expect("VB Z=1\r", ":A")
            .expect("W X\r", ":A X=0")
            .expect("W Y\r", ":A Y=0")
    }

    #[test]
    fn initialize() {
        let t = make_init_transport()
            .expect("W X\r", ":A X=0")
            .expect("W Y\r", ":A Y=0");
        let mut s = AsiTigerXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn move_absolute() {
        let t = make_init_transport()
            .expect("M X=1000 Y=2000\r", ":A")
            .expect("W X\r", ":A X=1000")
            .expect("W Y\r", ":A Y=2000");
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
            .expect("R X=500 Y=-500\r", ":A")
            .expect("W X\r", ":A X=500")
            .expect("W Y\r", ":A Y=-500");
        let mut s = AsiTigerXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(50.0, -50.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 50.0).abs() < 0.01);
        assert!((y + 50.0).abs() < 0.01);
    }

    #[test]
    fn relative_move_omits_zero_axes() {
        let t = make_init_transport()
            .expect("R Y=250\r", ":A")
            .expect("R X=-125\r", ":A")
            .expect("W X\r", ":A X=-125")
            .expect("W Y\r", ":A Y=250");
        let mut s = AsiTigerXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(0.0, 25.0).unwrap();
        s.set_relative_xy_position_um(-12.5, 0.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x + 12.5).abs() < 0.01);
        assert!((y - 25.0).abs() < 0.01);
    }

    #[test]
    fn home() {
        let t = make_init_transport()
            .expect("! X Y\r", ":A")
            .expect("W X\r", ":A X=0")
            .expect("W Y\r", ":A Y=0");
        let mut s = AsiTigerXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn stop_and_set_origin_use_tiger_stage_commands() {
        let t = make_init_transport()
            .expect("HALT\r", ":A")
            .expect("H X=0 Y=0\r", ":A");
        let mut s = AsiTigerXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.stop().unwrap();
        s.set_origin().unwrap();
    }

    #[test]
    fn upstream_step_size_and_limits_behavior() {
        let t = MockTransport::new()
            .expect("SL X?\r", ":A X=-1.5")
            .expect("SU X?\r", ":A X=2.5")
            .expect("SL Y?\r", ":A Y=-3")
            .expect("SU Y?\r", ":A Y=4");
        let s = AsiTigerXYStage::new().with_transport(Box::new(t));
        assert_eq!(s.get_step_size_um(), (0.001, 0.001));
        assert_eq!(
            s.get_limits_um().unwrap(),
            (-1500.0, 2500.0, -3000.0, 4000.0)
        );
    }

    #[test]
    fn uses_controller_unit_multipliers() {
        let t = MockTransport::new()
            .expect("0 V\r", ":A v3.01")
            .expect("UM X?\r", ":A X=20000")
            .expect("UM Y?\r", ":A Y=5000")
            .expect("VB Z=1\r", ":A")
            .expect("W X\r", ":A X=400")
            .expect("W Y\r", ":A Y=250")
            .expect("W X\r", ":A X=400")
            .expect("W Y\r", ":A Y=250")
            .expect("M X=100 Y=25\r", ":A")
            .expect("R X=20 Y=-5\r", ":A");
        let mut s = AsiTigerXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (20.0, 50.0));
        s.set_xy_position_um(5.0, 5.0).unwrap();
        s.set_relative_xy_position_um(1.0, -1.0).unwrap();
    }

    #[test]
    fn no_transport_error() {
        assert!(AsiTigerXYStage::new().initialize().is_err());
    }

    #[test]
    fn rejects_non_ack_response() {
        let t = MockTransport::new().expect("0 V\r", "garbled");
        let mut s = AsiTigerXYStage::new().with_transport(Box::new(t));
        assert!(matches!(
            s.initialize(),
            Err(MmError::SerialInvalidResponse)
        ));
    }

    #[test]
    fn busy_uses_per_axis_status_queries() {
        let idle = MockTransport::new()
            .expect("RS X?\r", ":A N")
            .expect("RS Y?\r", ":A N");
        let s = AsiTigerXYStage::new().with_transport(Box::new(idle));
        assert!(!s.busy());

        let x_busy = MockTransport::new().expect("RS X?\r", ":A B");
        let s = AsiTigerXYStage::new().with_transport(Box::new(x_busy));
        assert!(s.busy());

        let y_busy = MockTransport::new()
            .expect("RS X?\r", ":A N")
            .expect("RS Y?\r", ":A B");
        let s = AsiTigerXYStage::new().with_transport(Box::new(y_busy));
        assert!(s.busy());
    }
}
