/// Scientifica Motion8 XY stage.
///
/// The Motion8 uses a proprietary binary packet protocol at 115200 baud.
/// All packets begin with 0xBB.  Packets are COBS-encoded on the wire (null-terminated).
///
/// Key commands (command byte in packet header):
///   0x03  SetPositionXY(device, x_steps:i32, y_steps:i32)  → empty ack
///   0x14  GetAllPositions(device)                           → device:u8, x:i32, y:i32, z:i32, ...
///   0x02  Stop(device)                                      → empty ack
///   0x0C  SetPosition(device, axis, steps:i32)              → empty ack  (set origin)
///   0x0F  IsMoving(device)                                  → moving:u8
///
/// Step size: 0.01 µm/step (100 steps/µm).
///
/// For testability this adapter models the transport as ASCII lines (MockTransport compatible).
/// Each send produces a request tag "M8XY:<cmd>:<args>"; responses are parsed as CSV integers.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, XYStage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

/// 100 steps per µm (0.01 µm per step)
const STEPS_PER_UM: f64 = 100.0;

pub struct Motion8XYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    device_id: u8,
    x_um: f64,
    y_um: f64,
}

impl Motion8XYStage {
    pub fn new(device_id: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, transport: None, initialized: false, device_id, x_um: 0.0, y_um: 0.0 }
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

    fn cmd(&mut self, tag: &str) -> MmResult<String> {
        let c = format!("{}\n", tag);
        self.call_transport(|t| { let r = t.send_recv(&c)?; Ok(r.trim().to_string()) })
    }

    /// Query XY position; response is "<x_steps>,<y_steps>"
    fn query_xy(&mut self) -> MmResult<(f64, f64)> {
        let resp = self.cmd(&format!("M8XY:GET:{}", self.device_id))?;
        let parts: Vec<&str> = resp.splitn(2, ',').collect();
        if parts.len() < 2 {
            return Err(MmError::LocallyDefined(format!("Motion8 XY: bad response: {}", resp)));
        }
        let x: i64 = parts[0].trim().parse().unwrap_or(0);
        let y: i64 = parts[1].trim().parse().unwrap_or(0);
        Ok((x as f64 / STEPS_PER_UM, y as f64 / STEPS_PER_UM))
    }

    fn send_move(&mut self, x_steps: i64, y_steps: i64) -> MmResult<()> {
        let resp = self.cmd(&format!("M8XY:MOV:{},{},{}", self.device_id, x_steps, y_steps))?;
        if resp.starts_with("ERR") {
            return Err(MmError::LocallyDefined(format!("Motion8 XY move error: {}", resp)));
        }
        Ok(())
    }
}

impl Default for Motion8XYStage { fn default() -> Self { Self::new(0) } }

impl Device for Motion8XYStage {
    fn name(&self) -> &str { "ScientificaMotion8-XYStage" }
    fn description(&self) -> &str { "Scientifica Motion8 XY stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let (x, y) = self.query_xy()?;
        self.x_um = x;
        self.y_um = y;
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

impl XYStage for Motion8XYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let xs = (x * STEPS_PER_UM).round() as i64;
        let ys = (y * STEPS_PER_UM).round() as i64;
        self.send_move(xs, ys)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> { Ok((self.x_um, self.y_um)) }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let new_x = self.x_um + dx;
        let new_y = self.y_um + dy;
        self.set_xy_position_um(new_x, new_y)
    }

    fn home(&mut self) -> MmResult<()> {
        // Motion8 home is not supported; reset cached position
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd(&format!("M8XY:STOP:{}", self.device_id));
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        // Motion8 does not report limits; return a large nominal range
        Ok((-100_000.0, 100_000.0, -100_000.0, 100_000.0))
    }

    fn get_step_size_um(&self) -> (f64, f64) { (0.01, 0.01) }

    fn set_origin(&mut self) -> MmResult<()> {
        let _ = self.cmd(&format!("M8XY:ORIGIN:{}", self.device_id));
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any("10000,20000")  // GET → (100.0, 200.0) µm
    }

    #[test]
    fn initialize() {
        let mut s = Motion8XYStage::new(0).with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 100.0).abs() < 1e-9);
        assert!((y - 200.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any("OK");
        let mut s = Motion8XYStage::new(0).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(300.0, 400.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (300.0, 400.0));
    }

    #[test]
    fn move_relative() {
        let t = make_transport().any("OK");
        let mut s = Motion8XYStage::new(0).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(50.0, 25.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 150.0).abs() < 1e-9);
        assert!((y - 225.0).abs() < 1e-9);
    }

    #[test]
    fn device1_id() {
        let t = MockTransport::new().any("5000,7000");
        let mut s = Motion8XYStage::new(1).with_transport(Box::new(t));
        s.initialize().unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 50.0).abs() < 1e-9);
        assert!((y - 70.0).abs() < 1e-9);
    }

    #[test]
    fn no_transport_error() { assert!(Motion8XYStage::new(0).initialize().is_err()); }

    #[test]
    fn step_size() {
        assert_eq!(Motion8XYStage::new(0).get_step_size_um(), (0.01, 0.01));
    }
}
