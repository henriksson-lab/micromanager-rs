/// Xeryon piezo XY stage via serial (RS232/USB).
///
/// Protocol: multi-axis TAG=VALUE, prefixed with axis letter.
///   `X:DPOS=<counts>\n` — set desired position (absolute)
///   `X:EPOS=<counts>\r\n` — response: encoder position
///   `X:STAT=<bitmask>\r\n` — response: status register
///
/// Init:
///   Send `RSET=0` per axis, wait, then send encoder resolution command.
///   Query initial position via `X:EPOS` / `Y:EPOS`.
///
/// Default encoder resolution: 312.5 nm per count (XLS1=312).
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

/// Encoder resolution in nanometers per count.
const DEFAULT_ENCODER_RES_NM: f64 = 312.5;

/// Speed multiplier used when sending SSPD.
const SPEED_MULTIPLIER: i64 = 1000;

/// Default speed in mm/s.
const DEFAULT_SPEED_MM_S: f64 = 1.0;

pub struct XeryonXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    encoder_res_nm: f64,
    x_um: f64,
    y_um: f64,
}

impl XeryonXYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "EncoderResolution",
                PropertyValue::String("XLS1=312".into()),
                false,
            )
            .unwrap();
        props
            .define_property(
                "Speed_mm_per_s",
                PropertyValue::Float(DEFAULT_SPEED_MM_S),
                false,
            )
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            encoder_res_nm: DEFAULT_ENCODER_RES_NM,
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

    /// Send an axis-prefixed command: `<axis>:TAG=VALUE\n`.
    fn axis_cmd(&mut self, axis: char, tag: &str, value: i64) -> MmResult<String> {
        let full = format!("{}:{}={}\n", axis, tag, value);
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }

    /// Send a raw string command (e.g. encoder resolution).
    fn raw_cmd(&mut self, cmd: &str) -> MmResult<()> {
        let full = format!("{}\n", cmd);
        self.call_transport(|t| t.send(&full))
    }

    /// Send axis-prefixed command that expects no specific response.
    fn axis_send(&mut self, axis: char, tag: &str, value: i64) -> MmResult<()> {
        let full = format!("{}:{}={}\n", axis, tag, value);
        self.call_transport(|t| t.send(&full))
    }

    /// Parse a `<axis>:TAG=VALUE` response, returning the integer value.
    fn parse_response(resp: &str) -> Option<i64> {
        resp.split('=').nth(1).and_then(|s| s.trim().parse().ok())
    }

    /// Query encoder position for an axis.
    fn get_encoder_pos(&mut self, axis: char) -> MmResult<i64> {
        let resp = self.axis_cmd(axis, "EPOS", 0)?;
        Self::parse_response(&resp)
            .ok_or_else(|| MmError::LocallyDefined(format!("bad EPOS response: {}", resp)))
    }

    /// Convert encoder counts to micrometers.
    fn counts_to_um(&self, counts: i64) -> f64 {
        counts as f64 * self.encoder_res_nm / 1000.0
    }

    /// Convert micrometers to encoder counts.
    fn um_to_counts(&self, um: f64) -> i64 {
        (um * 1000.0 / self.encoder_res_nm).round() as i64
    }

    /// Look up encoder resolution in nm from the resolution command string.
    fn resolution_from_cmd(cmd: &str) -> f64 {
        // e.g. "XLS1=312" → 312 → 312.5, "XLS1=5" → 5.0, etc.
        if let Some(val_str) = cmd.split('=').nth(1) {
            match val_str.trim() {
                "312" => 312.5,
                "1250" | "1251" => 1250.0,
                "78" => 78.125,
                "5" => 5.0,
                "1" => 1.0,
                other => other.parse::<f64>().unwrap_or(DEFAULT_ENCODER_RES_NM),
            }
        } else {
            DEFAULT_ENCODER_RES_NM
        }
    }
}

impl Default for XeryonXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for XeryonXYStage {
    fn name(&self) -> &str {
        "XeryonXYStage"
    }
    fn description(&self) -> &str {
        "Xeryon piezo XY stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Determine encoder resolution from property
        let res_cmd = self.props.get("EncoderResolution")?.as_str().to_string();
        self.encoder_res_nm = Self::resolution_from_cmd(&res_cmd);

        // Reset both axes
        self.axis_send('X', "RSET", 0)?;
        self.axis_send('Y', "RSET", 0)?;

        // Send encoder resolution command for both axes
        self.raw_cmd(&res_cmd)?;

        // Set default speed
        let speed = self
            .props
            .get("Speed_mm_per_s")?
            .as_f64()
            .unwrap_or(DEFAULT_SPEED_MM_S);
        let speed_raw = (speed * SPEED_MULTIPLIER as f64) as i64;
        self.axis_send('X', "SSPD", speed_raw)?;
        self.axis_send('Y', "SSPD", speed_raw)?;

        // Read current positions
        let x_counts = self.get_encoder_pos('X')?;
        let y_counts = self.get_encoder_pos('Y')?;
        self.x_um = self.counts_to_um(x_counts);
        self.y_um = self.counts_to_um(y_counts);

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.axis_send('X', "STOP", 0);
            let _ = self.axis_send('Y', "STOP", 0);
            self.initialized = false;
        }
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

impl XYStage for XeryonXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let cx = self.um_to_counts(x);
        let cy = self.um_to_counts(y);
        self.axis_cmd('X', "DPOS", cx)?;
        self.axis_cmd('Y', "DPOS", cy)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        Ok((self.x_um, self.y_um))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let new_x = self.x_um + dx;
        let new_y = self.y_um + dy;
        self.set_xy_position_um(new_x, new_y)
    }

    fn home(&mut self) -> MmResult<()> {
        self.axis_cmd('X', "INDX", 0)?;
        self.axis_cmd('Y', "INDX", 0)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        self.axis_send('X', "STOP", 0)?;
        self.axis_send('Y', "STOP", 0)?;
        Ok(())
    }

    fn set_origin(&mut self) -> MmResult<()> {
        self.axis_cmd('X', "ZERO", 0)?;
        self.axis_cmd('Y', "ZERO", 0)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        let step = self.encoder_res_nm / 1000.0;
        (step, step)
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((0.0, 0.0, 0.0, 0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            // Only send_recv calls need script entries; RSET/encoder/SSPD use send-only.
            .expect("X:EPOS=0\n", "X:EPOS=0")
            .expect("Y:EPOS=0\n", "Y:EPOS=0")
    }

    #[test]
    fn initialize() {
        let mut s = XeryonXYStage::new().with_transport(Box::new(make_init_transport()));
        s.initialize().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn move_absolute() {
        // 100 µm = 100 * 1000 / 312.5 = 320 counts
        let t = make_init_transport()
            .expect("X:DPOS=320\n", "X:DPOS=320")
            .expect("Y:DPOS=640\n", "Y:DPOS=640");
        let mut s = XeryonXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(100.0, 200.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 100.0).abs() < 0.01);
        assert!((y - 200.0).abs() < 0.01);
    }

    #[test]
    fn move_relative() {
        // 50 µm = 50 * 1000 / 312.5 = 160 counts
        let t = make_init_transport()
            .expect("X:DPOS=160\n", "X:DPOS=160")
            .expect("Y:DPOS=-160\n", "Y:DPOS=-160");
        let mut s = XeryonXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(50.0, -50.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 50.0).abs() < 0.01);
        assert!((y + 50.0).abs() < 0.01);
    }

    #[test]
    fn home() {
        let t = make_init_transport()
            .expect("X:INDX=0\n", "X:INDX=OK")
            .expect("Y:INDX=0\n", "Y:INDX=OK");
        let mut s = XeryonXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn stop() {
        let t = make_init_transport()
            .expect("X:STOP=0\n", "X:STOP=OK")
            .expect("Y:STOP=0\n", "Y:STOP=OK");
        let mut s = XeryonXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.stop().unwrap();
    }

    #[test]
    fn step_size() {
        let s = XeryonXYStage::new();
        let (sx, sy) = s.get_step_size_um();
        assert!((sx - 0.3125).abs() < 1e-6);
        assert!((sy - 0.3125).abs() < 1e-6);
    }

    #[test]
    fn encoder_resolution_parsing() {
        assert!((XeryonXYStage::resolution_from_cmd("XLS1=312") - 312.5).abs() < 0.01);
        assert!((XeryonXYStage::resolution_from_cmd("XLS1=78") - 78.125).abs() < 0.01);
        assert!((XeryonXYStage::resolution_from_cmd("XLS1=5") - 5.0).abs() < 0.01);
        assert!((XeryonXYStage::resolution_from_cmd("XLS1=1") - 1.0).abs() < 0.01);
        assert!((XeryonXYStage::resolution_from_cmd("XLS1=1250") - 1250.0).abs() < 0.01);
        assert!((XeryonXYStage::resolution_from_cmd("XLS3=1251") - 1250.0).abs() < 0.01);
    }

    #[test]
    fn no_transport_error() {
        assert!(XeryonXYStage::new().initialize().is_err());
    }
}
