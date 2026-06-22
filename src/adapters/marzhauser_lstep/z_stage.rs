/// Marzhauser L-Step controller Z-stage.
///
/// Protocol (ASCII, `\r` terminated):
///   `?ver\r`          → version string; must contain "Vers:L"
///   `!autostatus 0\r` → disable autostatus
///   `?det\r`          → configuration; needs >= 3 axes: (det >> 4) & 0xf >= 3
///   `!dim z 1\r`      → switch Z to micrometer mode
///   `!moa z <pos>\r`  → move Z to absolute position (µm)
///   `!mor z <d>\r`    → move Z relative (µm)
///   `?pos z\r`        → current Z position in µm
///   `?err\r`          → error code; 0 = OK
///   `!pos z 0\r`      → set current Z position as origin
///   `a\r`             → abort / stop
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};

pub struct LStepZStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position_um: f64,
}

impl LStepZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            position_um: 0.0,
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
}

impl Default for LStepZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for LStepZStage {
    fn name(&self) -> &str {
        "ZAxis"
    }
    fn description(&self) -> &str {
        "L-Step Z axis driver"
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

        let det = self.cmd("?det")?;
        let config: i32 = det.trim().parse().unwrap_or(0);
        let num_axes = (config >> 4) & 0x0f;
        if num_axes < 3 {
            return Err(MmError::LocallyDefined(format!(
                "Controller has no Z axis (det={})",
                config
            )));
        }

        self.send_only("!dim z 1")?;
        let pos_str = self.cmd("?pos z")?;
        self.position_um = pos_str
            .trim()
            .parse::<f64>()
            .map_err(|_| MmError::LocallyDefined(format!("Cannot parse Z pos: {}", pos_str)))?;

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
        DeviceType::Stage
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Stage for LStepZStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        self.send_only("!dim z 1")?;
        self.send_only(&format!("!moa z {}", pos))?;
        self.check_err()?;
        self.position_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        Ok(self.position_um)
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        self.send_only("!dim z 1")?;
        self.send_only(&format!("!mor z {}", d))?;
        self.check_err()?;
        self.position_um += d;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn stop(&mut self) -> MmResult<()> {
        self.send_only("a")?;
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((-100_000.0, 100_000.0))
    }
    fn get_focus_direction(&self) -> FocusDirection {
        FocusDirection::Unknown
    }
    fn is_continuous_focus_drive(&self) -> bool {
        false
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
        // send_only calls ("!autostatus 0", "!dim z 1") do NOT consume script entries.
        MockTransport::new()
            .expect("?ver\r", "Vers:LS v3.1")
            // "!autostatus 0" is send_only — no script entry
            .expect("?det\r", "48") // 3 axes: (48 >> 4) & 0x0f = 3
            // "!dim z 1" is send_only — no script entry
            .expect("?pos z\r", "50.000")
    }

    #[test]
    fn initialize() {
        let mut stage = LStepZStage::new().with_transport(Box::new(make_transport()));
        assert_eq!(stage.name(), "ZAxis");
        assert_eq!(stage.description(), "L-Step Z axis driver");
        stage.initialize().unwrap();
        assert!((stage.get_position_um().unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        // "!dim z 1" and "!moa z 100" are send_only; "?err" is send_recv.
        let t = make_transport().expect("?err\r", "0");
        let mut stage = LStepZStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_position_um(100.0).unwrap();
        assert!((stage.get_position_um().unwrap() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn move_relative() {
        // "!dim z 1" and "!mor z 10" are send_only; "?err" is send_recv.
        let t = make_transport().expect("?err\r", "0");
        let mut stage = LStepZStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_relative_position_um(10.0).unwrap();
        assert!((stage.get_position_um().unwrap() - 60.0).abs() < 1e-9);
    }

    #[test]
    fn no_z_axis_rejected() {
        // send_only("!autostatus 0") does NOT consume a script entry.
        let t = MockTransport::new()
            .expect("?ver\r", "Vers:LS v3.1")
            .expect("?det\r", "32"); // only 2 axes
        let mut stage = LStepZStage::new().with_transport(Box::new(t));
        assert!(stage.initialize().is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(LStepZStage::new().initialize().is_err());
    }

    #[test]
    fn home_is_unsupported_like_upstream_z_stage() {
        let mut stage = LStepZStage::new().with_transport(Box::new(make_transport()));
        stage.initialize().unwrap();
        assert_eq!(stage.home(), Err(MmError::UnsupportedCommand));
        assert!((stage.get_position_um().unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn move_send_failure_preserves_cached_position() {
        let t = FailingSendTransport::new(
            "!mor z 10\r",
            vec![
                ("?ver\r", "Vers:LS v3.1"),
                ("?det\r", "48"),
                ("?pos z\r", "50.000"),
            ],
        );
        let mut stage = LStepZStage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        assert!(stage.set_relative_position_um(10.0).is_err());
        assert!((stage.get_position_um().unwrap() - 50.0).abs() < 1e-9);
    }
}
