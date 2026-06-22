/// Marzhauser L-Step controller XY-stage.
///
/// Protocol (ASCII, `\r` terminated):
///   `?ver\r`          → version string; must contain "Vers:L" (LS or LP)
///   `!autostatus 0\r` → disable autostatus reports
///   `?det\r`          → detect configuration (number of axes encoded in nibble)
///   `!dim 1 1\r`      → switch to micrometer mode for XY
///   `!moa <x> <y>\r`  → move to absolute position (µm, space-separated)
///   `!mor <dx> <dy>\r`→ move relative (µm)
///   `?pos\r`          → current position: `<x> <y>` in µm
///   `?err\r`          → error code; 0 = OK
///   `!pos 0 0\r`      → set current position as origin
///   `!cal x\r`        → calibrate (home) X axis
///   `!cal y\r`        → calibrate (home) Y axis
///   `a\r`             → abort / stop all motion
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct LStepXYStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl LStepXYStage {
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
        self.call_transport(|t| Ok(t.send_recv(&cmd)?.trim().to_string()))
    }

    fn send_only(&mut self, command: &str) -> MmResult<()> {
        let cmd = format!("{}\r", command);
        self.call_transport(|t| {
            t.send(&cmd)?;
            Ok(())
        })
    }

    fn check_err(&mut self) -> MmResult<()> {
        let resp = self.cmd("?err")?;
        let code: i32 = resp.trim().parse().unwrap_or(1);
        if code != 0 {
            return Err(MmError::LocallyDefined(format!(
                "LStep error code: {}",
                code
            )));
        }
        Ok(())
    }

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

impl Default for LStepXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for LStepXYStage {
    fn name(&self) -> &str {
        "XYStage"
    }
    fn description(&self) -> &str {
        "L-Step XY stage driver adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        let ver = self.cmd("?ver")?;
        if !ver.contains("Vers:L") {
            return Err(MmError::LocallyDefined(format!(
                "Unexpected controller version: {}",
                ver
            )));
        }

        self.send_only("!autostatus 0")?;

        // Check axis count via ?det
        let det = self.cmd("?det")?;
        let config: i32 = det.trim().parse().unwrap_or(0);
        let num_axes = (config >> 4) & 0x0f;
        if num_axes < 2 {
            return Err(MmError::LocallyDefined(format!(
                "Controller has fewer than 2 axes (det={})",
                config
            )));
        }

        // Switch to µm and read current position
        self.send_only("!dim 1 1")?;
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

impl XYStage for LStepXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        self.send_only("!dim 1 1")?;
        self.send_only(&format!("!moa {} {}", x, y))?;
        self.check_err()?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        Ok((self.x_um, self.y_um))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        self.send_only("!dim 1 1")?;
        self.send_only(&format!("!mor {} {}", dx, dy))?;
        self.check_err()?;
        self.x_um += dx;
        self.y_um += dy;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        self.send_only("!cal x")?;
        self.send_only("!cal y")?;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        self.send_only("a")?;
        Ok(())
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((-100_000.0, 100_000.0, -100_000.0, 100_000.0))
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        (0.02, 0.02)
    }

    fn set_origin(&mut self) -> MmResult<()> {
        self.send_only("!pos 0 0")?;
        self.check_err()?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;
    use std::collections::VecDeque;

    struct FailingSendTransport {
        fail_on: String,
        last_sent: String,
        replies: VecDeque<(String, String)>,
    }

    impl FailingSendTransport {
        fn new(fail_on: &str, replies: Vec<(&str, &str)>) -> Self {
            Self {
                fail_on: fail_on.to_string(),
                last_sent: String::new(),
                replies: replies
                    .into_iter()
                    .map(|(cmd, resp)| (cmd.to_string(), resp.to_string()))
                    .collect(),
            }
        }
    }

    impl Transport for FailingSendTransport {
        fn send(&mut self, cmd: &str) -> MmResult<()> {
            if cmd == self.fail_on {
                return Err(MmError::LocallyDefined("send failed".into()));
            }
            self.last_sent = cmd.to_string();
            Ok(())
        }

        fn receive_line(&mut self) -> MmResult<String> {
            let (expected, reply) = self.replies.pop_front().ok_or(MmError::SerialTimeout)?;
            if self.last_sent != expected {
                return Err(MmError::LocallyDefined(format!(
                    "expected {}, got {}",
                    expected, self.last_sent
                )));
            }
            Ok(reply)
        }

        fn purge(&mut self) -> MmResult<()> {
            Ok(())
        }
    }

    fn make_transport() -> MockTransport {
        // Only include commands that are send_recv (cmd()), not send_only().
        // send_only() calls transport.send() but NOT receive_line(), so no script entry needed.
        MockTransport::new()
            .expect("?ver\r", "Vers:LS v3.1")
            // "!autostatus 0" is send_only — no script entry
            .expect("?det\r", "32") // 2 axes: (32 >> 4) & 0x0f = 2
            // "!dim 1 1" is send_only — no script entry
            .expect("?pos\r", "10.000 20.000")
    }

    #[test]
    fn initialize() {
        let mut stage = LStepXYStage::new().with_transport(Box::new(make_transport()));
        assert_eq!(stage.name(), "XYStage");
        assert_eq!(stage.description(), "L-Step XY stage driver adapter");
        stage.initialize().unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (10.0, 20.0));
    }

    #[test]
    fn wrong_controller_rejected() {
        let t = MockTransport::new().expect("?ver\r", "Other v1.0");
        let mut stage = LStepXYStage::new().with_transport(Box::new(t));
        assert!(stage.initialize().is_err());
    }

    #[test]
    fn move_absolute() {
        // "!dim 1 1" and "!moa ..." are send_only (no response consumed).
        // "?err" is send_recv (response consumed).
        let t = make_transport().expect("?err\r", "0");
        let mut stage = LStepXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_xy_position_um(100.0, 200.0).unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (100.0, 200.0));
    }

    #[test]
    fn move_relative() {
        // "!dim 1 1" and "!mor ..." are send_only (no response consumed).
        let t = make_transport().expect("?err\r", "0");
        let mut stage = LStepXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_relative_xy_position_um(5.0, 10.0).unwrap();
        let (x, y) = stage.get_xy_position_um().unwrap();
        assert!((x - 15.0).abs() < 1e-9);
        assert!((y - 30.0).abs() < 1e-9);
    }

    #[test]
    fn no_transport_error() {
        assert!(LStepXYStage::new().initialize().is_err());
    }

    #[test]
    fn home_does_not_invent_zero_position() {
        let mut stage = LStepXYStage::new().with_transport(Box::new(make_transport()));
        stage.initialize().unwrap();
        stage.home().unwrap();
        assert_eq!(stage.get_xy_position_um().unwrap(), (10.0, 20.0));
    }

    #[test]
    fn move_send_failure_preserves_cached_position() {
        let t = FailingSendTransport::new(
            "!moa 100 200\r",
            vec![
                ("?ver\r", "Vers:LS v3.1"),
                ("?det\r", "32"),
                ("?pos\r", "10.000 20.000"),
            ],
        );
        let mut stage = LStepXYStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        assert!(stage.set_xy_position_um(100.0, 200.0).is_err());
        assert_eq!(stage.get_xy_position_um().unwrap(), (10.0, 20.0));
    }
}
