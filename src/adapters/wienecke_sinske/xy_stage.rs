/// Wienecke & Sinske WSB PiezoDrive CAN XY stage.
///
/// The hardware uses a CAN29 bus protocol internally.  For serial port communication
/// the controller accepts ASCII commands.  Key commands (CR terminated):
///
///   `POS X\r`            → "<x_nm>\r\n"
///   `POS Y\r`            → "<y_nm>\r\n"
///   `MOVE X <nm>\r`      → "OK\r\n" or "ERR <msg>"
///   `MOVE Y <nm>\r`      → "OK\r\n" or "ERR <msg>"
///   `RMOVE X <dnm>\r`    → "OK\r\n" or "ERR <msg>"
///   `RMOVE Y <dnm>\r`    → "OK\r\n" or "ERR <msg>"
///   `STOP\r`             → "OK\r\n"
///
/// Step size: 0.001 µm (1 nm).  Positions in nm on the wire.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};

const NM_PER_UM: f64 = 1000.0;

pub struct WSXYStage {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    x_um: Cell<f64>,
    y_um: Cell<f64>,
    busy_x: Cell<bool>,
    busy_y: Cell<bool>,
}

impl WSXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_property("Velocity (micron/s)", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("Velocity (micron/s)", 0.0, 100000.0)
            .unwrap();
        props
            .define_property(
                "Acceleration (micron/s^2)",
                PropertyValue::Float(0.0),
                false,
            )
            .unwrap();
        props
            .set_property_limits("Acceleration (micron/s^2)", 0.0, 500000.0)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            x_um: Cell::new(0.0),
            y_um: Cell::new(0.0),
            busy_x: Cell::new(false),
            busy_y: Cell::new(false),
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

    fn check_ok(resp: &str) -> MmResult<()> {
        if resp.starts_with("ERR") {
            Err(MmError::LocallyDefined(format!("WS error: {}", resp)))
        } else if resp == "OK" {
            Ok(())
        } else {
            Err(MmError::SerialInvalidResponse)
        }
    }

    fn query_axis(&self, axis: &str) -> MmResult<f64> {
        let resp = self.cmd(&format!("POS {}", axis))?;
        let nm: i64 = resp
            .trim()
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        Ok(nm as f64 / NM_PER_UM)
    }

    fn query_presence(&self, axis: &str) -> MmResult<bool> {
        let resp = self.cmd(&format!("PRESENT {}", axis))?;
        Ok(matches!(resp.as_str(), "1" | "OK" | "PRESENT"))
    }

    fn query_busy_axis(&self, axis: &str) -> MmResult<bool> {
        let resp = self.cmd(&format!("BUSY {}", axis))?;
        Ok(matches!(resp.as_str(), "1" | "BUSY" | "MOVING"))
    }

    fn query_limit_axis(&self, axis: &str, which: &str) -> MmResult<f64> {
        let resp = self.cmd(&format!("LIMIT {} {}", axis, which))?;
        let nm: i64 = resp
            .trim()
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        Ok(nm as f64 / NM_PER_UM)
    }

    fn set_motion_property(&self, command: &str, value_um: f64) -> MmResult<()> {
        let nm = (value_um * NM_PER_UM) as i64;
        let rx = self.cmd(&format!("{} X {}", command, nm))?;
        Self::check_ok(&rx)?;
        let ry = self.cmd(&format!("{} Y {}", command, nm))?;
        Self::check_ok(&ry)
    }
}

impl Default for WSXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for WSXYStage {
    fn name(&self) -> &str {
        "WS-XYStage"
    }
    fn description(&self) -> &str {
        "Wienecke & Sinske WSB PiezoDrive XY stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        if !(self.query_presence("X")? && self.query_presence("Y")?) {
            return Err(MmError::DeviceNotFound("WSB PiezoDrive CAN".into()));
        }
        self.x_um.set(self.query_axis("X")?);
        self.y_um.set(self.query_axis("Y")?);
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
        if name == "Port" && self.initialized {
            return Err(MmError::CanNotSetProperty);
        }
        if name == "Velocity (micron/s)" || name == "Acceleration (micron/s^2)" {
            let value = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            if self.initialized {
                let cmd = if name == "Velocity (micron/s)" {
                    "VEL"
                } else {
                    "ACCEL"
                };
                self.set_motion_property(cmd, value)?;
            }
            self.props.set(name, PropertyValue::Float(value))?;
            return Ok(());
        }
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
        let busy_x = self.query_busy_axis("X").unwrap_or(self.busy_x.get());
        let busy_y = self.query_busy_axis("Y").unwrap_or(self.busy_y.get());
        self.busy_x.set(busy_x);
        self.busy_y.set(busy_y);
        busy_x || busy_y
    }
}

impl XYStage for WSXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let xnm = (x * NM_PER_UM).round() as i64;
        let ynm = (y * NM_PER_UM).round() as i64;
        let rx = self.cmd(&format!("MOVE X {}", xnm))?;
        Self::check_ok(&rx)?;
        let ry = self.cmd(&format!("MOVE Y {}", ynm))?;
        Self::check_ok(&ry)?;
        self.x_um.set(x);
        self.y_um.set(y);
        self.busy_x.set(true);
        self.busy_y.set(true);
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        let x = self.query_axis("X")?;
        let y = self.query_axis("Y")?;
        self.x_um.set(x);
        self.y_um.set(y);
        Ok((x, y))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let dxnm = (dx * NM_PER_UM).round() as i64;
        let dynm = (dy * NM_PER_UM).round() as i64;
        let rx = self.cmd(&format!("RMOVE X {}", dxnm))?;
        Self::check_ok(&rx)?;
        let ry = self.cmd(&format!("RMOVE Y {}", dynm))?;
        Self::check_ok(&ry)?;
        self.x_um.set(self.x_um.get() + dx);
        self.y_um.set(self.y_um.get() + dy);
        self.busy_x.set(true);
        self.busy_y.set(true);
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("HOME LOWER")?;
        Self::check_ok(&r)?;
        self.busy_x.set(true);
        self.busy_y.set(true);
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let rx = self.cmd("STOP X")?;
        Self::check_ok(&rx)?;
        let ry = self.cmd("STOP Y")?;
        Self::check_ok(&ry)?;
        self.busy_x.set(false);
        self.busy_y.set(false);
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((
            self.query_limit_axis("X", "LOWER")?,
            self.query_limit_axis("X", "UPPER")?,
            self.query_limit_axis("Y", "LOWER")?,
            self.query_limit_axis("Y", "UPPER")?,
        ))
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        (0.001, 0.001)
    }

    fn set_origin(&mut self) -> MmResult<()> {
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
            .any("1") // PRESENT X
            .any("1") // PRESENT Y
            .any("100000") // POS X → 100 µm
            .any("200000") // POS Y → 200 µm
    }

    #[test]
    fn initialize() {
        let t = make_transport().any("100000").any("200000");
        let mut s = WSXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 100.0).abs() < 1e-9);
        assert!((y - 200.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport()
            .any("OK")
            .any("OK")
            .any("50000")
            .any("75000");
        let mut s = WSXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(50.0, 75.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (50.0, 75.0));
    }

    #[test]
    fn move_relative() {
        let t = make_transport()
            .any("OK")
            .any("OK")
            .any("110000")
            .any("205000");
        let mut s = WSXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(10.0, 5.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 110.0).abs() < 1e-9);
        assert!((y - 205.0).abs() < 1e-9);
    }

    #[test]
    fn busy_polls_axes() {
        let t = make_transport().any("1").any("0");
        let mut s = WSXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
    }

    #[test]
    fn limits_are_live_hardware_stops() {
        let t = make_transport()
            .any("-1000")
            .any("2000")
            .any("-3000")
            .any("4000");
        let mut s = WSXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_limits_um().unwrap(), (-1.0, 2.0, -3.0, 4.0));
    }

    #[test]
    fn initialized_port_change_is_forbidden() {
        let mut s = WSXYStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        assert_eq!(
            s.set_property("Port", PropertyValue::String("COM2".into()))
                .unwrap_err(),
            MmError::CanNotSetProperty
        );
    }

    #[test]
    fn error_response_fails() {
        let t = make_transport().any("ERR: limit");
        let mut s = WSXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_xy_position_um(999_999.0, 0.0).is_err());
    }

    #[test]
    fn malformed_move_ack_does_not_update_cache() {
        let t = make_transport().any("DONE");
        let mut s = WSXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.set_xy_position_um(50.0, 75.0).unwrap_err(),
            MmError::SerialInvalidResponse
        );
        assert_eq!(s.x_um.get(), 100.0);
        assert_eq!(s.y_um.get(), 200.0);
    }

    #[test]
    fn failed_initialized_motion_property_write_does_not_update_cache() {
        let t = make_transport().any("OK").any("ERR: rejected");
        let mut s = WSXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s
            .set_property("Velocity (micron/s)", PropertyValue::Float(25.0))
            .is_err());
        assert_eq!(
            s.get_property("Velocity (micron/s)").unwrap(),
            PropertyValue::Float(0.0)
        );
    }

    #[test]
    fn no_transport_error() {
        assert!(WSXYStage::new().initialize().is_err());
    }
}
