/// Conix Research XYZ controller XY stage.
///
/// Protocol (TX `\r`, RX `\r`):
///   `COMUNITS UM\r`  → `:A`           (set units to microns)
///   `W X Y\r`        → `:A <x> <y>`  (positions in µm)
///   `M X<x> Y<y>\r`  → `:A`          (move to absolute position in µm)
///   `!\r`            → `:A`           (home)
///   `\\r`            → `:A`           (halt/stop; backslash + CR)
///   `H\r`            → `:A`           (set origin HERE)
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

fn check_a(resp: &str) -> MmResult<&str> {
    let s = resp.trim();
    if let Some(rest) = s.strip_prefix(":A") {
        Ok(rest.trim())
    } else {
        Err(MmError::LocallyDefined(format!("Conix error: {}", s)))
    }
}

pub struct ConixXYStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl ConixXYStage {
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
        *self.transport.get_mut() = Some(t);
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        let mut transport = self.transport.borrow_mut();
        match transport.as_mut() {
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

    fn query_xy_position_um(&self) -> MmResult<(f64, f64)> {
        let pos = self.cmd("W X Y")?;
        Self::parse_xy(&pos)
    }

    fn poll_busy(&self) -> MmResult<bool> {
        let r = self.cmd("/")?;
        Ok(r.trim_start().starts_with('B'))
    }

    fn parse_xy(resp: &str) -> MmResult<(f64, f64)> {
        let body = check_a(resp)?;
        let parts: Vec<&str> = body.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(MmError::LocallyDefined(format!(
                "Cannot parse W X Y: {}",
                resp
            )));
        }
        let x: f64 = parts[0]
            .parse()
            .map_err(|_| MmError::LocallyDefined(format!("Cannot parse W X Y: {}", resp)))?;
        let y: f64 = parts[1]
            .parse()
            .map_err(|_| MmError::LocallyDefined(format!("Cannot parse W X Y: {}", resp)))?;
        Ok((x, y))
    }
}

impl Default for ConixXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ConixXYStage {
    fn name(&self) -> &str {
        "ConixXYStage"
    }
    fn description(&self) -> &str {
        "Conix XY stage driver"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        let r = self.cmd("COMUNITS UM")?;
        check_a(&r)?;
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
        self.poll_busy().unwrap_or(false)
    }
}

impl XYStage for ConixXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let r = self.cmd(&format!("M X{} Y{}", x, y))?;
        check_a(&r)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }
    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        self.query_xy_position_um()
    }
    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        self.set_xy_position_um(self.x_um + dx, self.y_um + dy)
    }
    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("!")?;
        check_a(&r)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> {
        // backslash = HALT command; upstream accepts :N-21 while moving.
        let r = self.cmd("\\")?;
        if r.trim() != ":N-21" {
            check_a(&r)?;
        }
        Ok(())
    }
    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((0.0, 20_000.0, 0.0, 20_000.0))
    }
    fn get_step_size_um(&self) -> (f64, f64) {
        (0.015, 0.015)
    }
    fn set_origin(&mut self) -> MmResult<()> {
        let r = self.cmd("H")?;
        check_a(&r)?;
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
        MockTransport::new().expect("COMUNITS UM\r", ":A")
    }

    #[test]
    fn initialize() {
        let t = make_transport().expect("W X Y\r", ":A 100.5 200.3");
        let mut s = ConixXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 100.5).abs() < 1e-6);
        assert!((y - 200.3).abs() < 1e-6);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any(":A").expect("W X Y\r", ":A 300 400");
        let mut s = ConixXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(300.0, 400.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (300.0, 400.0));
    }

    #[test]
    fn home() {
        let t = make_transport().any(":A");
        let mut s = ConixXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert_eq!((s.x_um, s.y_um), (0.0, 0.0));
    }

    #[test]
    fn stop_accepts_halt_while_moving_response() {
        let t = make_transport().expect("\\\r", ":N-21");
        let mut s = ConixXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.stop().unwrap();
    }

    #[test]
    fn error_fails() {
        let t = make_transport().any(":N-21");
        let mut s = ConixXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_xy_position_um(99999.0, 0.0).is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(ConixXYStage::new().initialize().is_err());
    }

    #[test]
    fn busy_polls_status_shortcut() {
        let t = make_transport().expect("/\r", "B");
        let mut s = ConixXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
    }

    #[test]
    fn get_position_queries_live_position() {
        let t = make_transport().expect("W X Y\r", ":A 7 8");
        let mut s = ConixXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (7.0, 8.0));
    }

    #[test]
    fn malformed_position_reply_errors_instead_of_zeroing_axis() {
        let t = make_transport().expect("W X Y\r", ":A abc 8");
        let mut s = ConixXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.get_xy_position_um().is_err());
    }

    #[test]
    fn initialized_port_change_is_rejected() {
        let t = make_transport();
        let mut s = ConixXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s
            .set_property("Port", PropertyValue::String("COM2".to_string()))
            .is_err());
        assert_eq!(
            s.get_property("Port").unwrap(),
            PropertyValue::String("Undefined".to_string())
        );
    }
}
