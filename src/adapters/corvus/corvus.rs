/// ITK Corvus XY-stage controller.
///
/// Protocol: commands end with space `" "` (TX terminator), responses end with `\r\n`.
///   `"0 mode "`           → enter host mode
///   `"version "`          → firmware version string
///   `"1 1 setunit "`      → set axis 1 to µm
///   `"1 2 setunit "`      → set axis 2 to µm
///   `"ge "`               → clear errors
///   `"p "`                → query position → `"X Y\r\n"` (floats, µm)
///   `"X Y move "`         → absolute move (µm)
///   `"dX dY rmove "`      → relative move (µm)
///   `"st "`               → status (bit 0 = busy)
///   `"cal "`              → calibrate/home
///   `"abort "`            → emergency stop
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct CorvusXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl CorvusXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Version", PropertyValue::String(String::new()), true)
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

    /// Send command with trailing space (Corvus TX terminator).
    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let cmd = format!("{} ", command);
        self.call_transport(|t| {
            let r = t.send_recv(&cmd)?;
            Ok(r.trim().to_string())
        })
    }

    fn parse_xy(resp: &str) -> MmResult<(f64, f64)> {
        let parts: Vec<&str> = resp.trim().split_whitespace().collect();
        if parts.len() < 2 {
            return Err(MmError::LocallyDefined(format!(
                "Cannot parse XY: {}",
                resp
            )));
        }
        Ok((
            parts[0].parse().unwrap_or(0.0),
            parts[1].parse().unwrap_or(0.0),
        ))
    }

    fn select_xy_axes(&mut self) -> MmResult<()> {
        self.cmd("2 setdim")?;
        self.cmd("1 1 setaxis")?;
        self.cmd("1 2 setaxis")?;
        Ok(())
    }
}

impl Default for CorvusXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CorvusXYStage {
    fn name(&self) -> &str {
        "XY Stage"
    }
    fn description(&self) -> &str {
        "XY Stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let _ = self.cmd("0 mode");
        let ver = self.cmd("version")?;
        self.props
            .entry_mut("Version")
            .map(|e| e.value = PropertyValue::String(ver));
        let _ = self.cmd("1 1 setunit");
        let _ = self.cmd("1 2 setunit");
        let _ = self.cmd("ge");
        self.select_xy_axes()?;
        let pos = self.cmd("p")?;
        let (x, y) = Self::parse_xy(&pos)?;
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

impl XYStage for CorvusXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        self.select_xy_axes()?;
        self.cmd(&format!("{:.4} {:.4} move", x, y))?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }
    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        Ok((self.x_um, self.y_um))
    }
    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        self.select_xy_axes()?;
        self.cmd(&format!("{:.4} {:.4} rmove", dx, dy))?;
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }
    fn home(&mut self) -> MmResult<()> {
        self.cmd("cal")?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> {
        let _ = self.cmd("abort");
        Ok(())
    }
    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Err(MmError::NotSupported)
    }
    fn get_step_size_um(&self) -> (f64, f64) {
        (0.1, 0.1)
    }
    fn set_origin(&mut self) -> MmResult<()> {
        self.select_xy_axes()?;
        self.cmd("0 0 setpos")?;
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
            .any("OK") // 0 mode
            .any("Corvus v2.3") // version
            .any("OK") // 1 1 setunit
            .any("OK") // 1 2 setunit
            .any("OK") // ge
            .expect("2 setdim ", "OK")
            .expect("1 1 setaxis ", "OK")
            .expect("1 2 setaxis ", "OK")
            .any("100.0 200.0") // p
    }

    #[test]
    fn initialize() {
        let mut s = CorvusXYStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (100.0, 200.0));
    }

    #[test]
    fn move_absolute() {
        let t = make_transport()
            .expect("2 setdim ", "OK")
            .expect("1 1 setaxis ", "OK")
            .expect("1 2 setaxis ", "OK")
            .any("OK");
        let mut s = CorvusXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(300.0, 400.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (300.0, 400.0));
    }

    #[test]
    fn move_relative() {
        let t = make_transport()
            .expect("2 setdim ", "OK")
            .expect("1 1 setaxis ", "OK")
            .expect("1 2 setaxis ", "OK")
            .any("OK");
        let mut s = CorvusXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(10.0, 20.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 110.0).abs() < 1e-9);
        assert!((y - 220.0).abs() < 1e-9);
    }

    #[test]
    fn no_transport_error() {
        assert!(CorvusXYStage::new().initialize().is_err());
    }

    #[test]
    fn limits_are_not_fabricated() {
        let s = CorvusXYStage::new();
        assert_eq!(s.get_limits_um().unwrap_err(), MmError::NotSupported);
    }
}
