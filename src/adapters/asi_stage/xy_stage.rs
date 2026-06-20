/// ASI XY-stage (Applied Scientific Instrumentation).
///
/// Protocol (ASCII, `\r` terminated):
///   `M X=<x> Y=<y>\r` → move to absolute position (tenths of microns)
///                        response: `:A\r` or `:N<code>\r`
///   `W X Y\r`          → query position; response `:A X=<x> Y=<y>\r`
///   `R X=<dx> Y=<dy>\r`→ relative move
///   `! X Y\r`          → home axes
///   `H X=0 Y=0\r`      → set origin
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

const ASI_SERIAL_UNITS_PER_UM: f64 = 10.0;
const STEP_SIZE_UM: f64 = 0.01;

pub struct AsiXYStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl AsiXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();

        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
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
        let cmd = format!("{}\r", command);
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }

    fn check_response(resp: &str) -> MmResult<()> {
        if resp.starts_with(":N") {
            return Err(MmError::LocallyDefined(format!("ASI error: {}", resp)));
        }
        if !resp.starts_with(":A") {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(())
    }

    fn asi_units(um: f64) -> String {
        format!("{:.6}", um * ASI_SERIAL_UNITS_PER_UM)
    }

    /// Parse `:A X=<x> Y=<y>` or upstream `:A <x> <y>` → (x_um, y_um).
    fn parse_xy(resp: &str) -> MmResult<(f64, f64)> {
        let resp = resp.trim();
        let mut x = None;
        let mut y = None;
        let mut unlabeled = Vec::new();
        for token in resp.split_whitespace() {
            if let Some(v) = token.strip_prefix("X=") {
                x = v.parse::<f64>().ok();
            }
            if let Some(v) = token.strip_prefix("Y=") {
                y = v.parse::<f64>().ok();
            }
            if token != ":A" && !token.contains('=') {
                if let Ok(value) = token.parse::<f64>() {
                    unlabeled.push(value);
                }
            }
        }
        if x.is_none() && y.is_none() && unlabeled.len() >= 2 {
            x = Some(unlabeled[0]);
            y = Some(unlabeled[1]);
        }
        match (x, y) {
            (Some(xv), Some(yv)) => {
                Ok((xv / ASI_SERIAL_UNITS_PER_UM, yv / ASI_SERIAL_UNITS_PER_UM))
            }
            _ => Err(MmError::LocallyDefined(format!(
                "Cannot parse XY: {}",
                resp
            ))),
        }
    }
}

impl Default for AsiXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for AsiXYStage {
    fn name(&self) -> &str {
        "ASI-XYStage"
    }
    fn description(&self) -> &str {
        "ASI XY-stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
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
        self.cmd("/")
            .map(|resp| resp.trim().starts_with(":B"))
            .unwrap_or(false)
    }
}

impl XYStage for AsiXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let resp = self.cmd(&format!(
            "M X={} Y={}",
            Self::asi_units(x),
            Self::asi_units(y)
        ))?;
        Self::check_response(&resp)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        let resp = self.cmd("W X Y")?;
        Self::parse_xy(&resp)
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let command = if dx == 0.0 && dy != 0.0 {
            format!("R Y={}", Self::asi_units(dy))
        } else if dx != 0.0 && dy == 0.0 {
            format!("R X={}", Self::asi_units(dx))
        } else {
            format!("R X={} Y={}", Self::asi_units(dx), Self::asi_units(dy))
        };
        let resp = self.cmd(&command)?;
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
        let resp = self.cmd("HALT")?;
        if resp.starts_with(":N-21") {
            return Ok(());
        }
        Self::check_response(&resp)?;
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Err(MmError::UnsupportedCommand)
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        (STEP_SIZE_UM, STEP_SIZE_UM)
    }

    fn set_origin(&mut self) -> MmResult<()> {
        let resp = self.cmd("H X=0 Y=0")?;
        Self::check_response(&resp)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_reads_position() {
        let t = MockTransport::new()
            .expect("W X Y\r", ":A 1000 2000")
            .expect("W X Y\r", ":A 1000 2000");
        let mut stage = AsiXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (100.0, 200.0));
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new()
            .expect("W X Y\r", ":A X=0 Y=0")
            .expect("M X=1500.000000 Y=2500.000000\r", ":A")
            .expect("W X Y\r", ":A X=1500 Y=2500");
        let mut stage = AsiXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_xy_position_um(150.0, 250.0).unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (150.0, 250.0));
    }

    #[test]
    fn move_relative() {
        let t = MockTransport::new()
            .expect("W X Y\r", ":A X=0 Y=0")
            .expect("R X=100.000000 Y=200.000000\r", ":A")
            .expect("W X Y\r", ":A X=100 Y=200");
        let mut stage = AsiXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_relative_xy_position_um(10.0, 20.0).unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (10.0, 20.0));
    }

    #[test]
    fn relative_move_omits_zero_axis_like_upstream() {
        let t = MockTransport::new()
            .expect("W X Y\r", ":A X=0 Y=0")
            .expect("R Y=200.000000\r", ":A")
            .expect("W X Y\r", ":A X=0 Y=200");
        let mut stage = AsiXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_relative_xy_position_um(0.0, 20.0).unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (0.0, 20.0));
    }

    #[test]
    fn stop_and_origin_use_upstream_commands() {
        let t = MockTransport::new()
            .expect("W X Y\r", ":A X=0 Y=0")
            .expect("HALT\r", ":A")
            .expect("H X=0 Y=0\r", ":A");
        let mut stage = AsiXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.stop().unwrap();
        stage.set_origin().unwrap();
    }

    #[test]
    fn limits_are_unsupported_like_upstream() {
        let stage = AsiXYStage::new();
        assert_eq!(
            stage.get_limits_um().unwrap_err(),
            MmError::UnsupportedCommand
        );
        assert_eq!(stage.get_step_size_um(), (0.01, 0.01));
    }

    #[test]
    fn no_transport_error() {
        assert!(AsiXYStage::new().initialize().is_err());
    }
}
