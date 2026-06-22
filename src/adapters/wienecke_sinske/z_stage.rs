/// Wienecke & Sinske WSB ZPiezo stage (WS or CAN protocol).
///
/// ASCII command interface (CR terminated):
///   `POS Z\r`         → "<z_nm>\r\n"
///   `MOVE Z <nm>\r`   → "OK\r\n" or "ERR <msg>"
///   `RMOVE Z <dnm>\r` → "OK\r\n" or "ERR <msg>"
///   `STOP\r`          → "OK\r\n"
///
/// Step size: 0.001 µm (1 nm).  Positions in nm on the wire.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::{Cell, RefCell};

const NM_PER_UM: f64 = 1000.0;

pub struct WSZStage {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    pos_um: Cell<f64>,
    busy: Cell<bool>,
}

impl WSZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_property("Protocol", PropertyValue::String("WS".into()), false)
            .unwrap();
        props
            .set_allowed_values("Protocol", &["WS", "CAN"])
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
            pos_um: Cell::new(0.0),
            busy: Cell::new(false),
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
            Err(MmError::LocallyDefined(format!("WS Z error: {}", resp)))
        } else if resp == "OK" {
            Ok(())
        } else {
            Err(MmError::SerialInvalidResponse)
        }
    }

    fn um_to_steps(pos_um: f64) -> i64 {
        (pos_um * NM_PER_UM) as i64
    }

    fn protocol(&self) -> &str {
        self.props
            .get("Protocol")
            .map(|v| v.as_str())
            .unwrap_or("WS")
    }

    fn query_presence(&self) -> MmResult<bool> {
        let command = if self.protocol() == "WS" {
            "[3=PA?]"
        } else {
            "PRESENT Z"
        };
        let resp = self.cmd(command)?;
        if self.protocol() == "WS" {
            Ok(resp.parse::<i64>().is_ok())
        } else {
            Ok(matches!(resp.as_str(), "1" | "OK" | "PRESENT"))
        }
    }

    fn query_position(&self) -> MmResult<f64> {
        let command = if self.protocol() == "WS" {
            "[3=PA?]"
        } else {
            "POS Z"
        };
        let resp = self.cmd(command)?;
        let nm: i64 = resp
            .trim()
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        Ok(nm as f64 / NM_PER_UM)
    }

    fn query_busy(&self) -> MmResult<bool> {
        let command = if self.protocol() == "WS" {
            "[3=PO?]"
        } else {
            "BUSY Z"
        };
        let resp = self.cmd(command)?;
        Ok(matches!(resp.as_str(), "1" | "BUSY" | "MOVING"))
    }

    fn query_limit(&self, which: &str) -> MmResult<f64> {
        let resp = self.cmd(&format!("LIMIT Z {}", which))?;
        let nm: i64 = resp
            .trim()
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        Ok(nm as f64 / NM_PER_UM)
    }

    fn set_motion_property(&self, command: &str, value_um: f64) -> MmResult<()> {
        let nm = (value_um * NM_PER_UM) as i64;
        let resp = self.cmd(&format!("{} Z {}", command, nm))?;
        Self::check_ok(&resp)
    }
}

impl Default for WSZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for WSZStage {
    fn name(&self) -> &str {
        "WS-ZStage"
    }
    fn description(&self) -> &str {
        "Wienecke & Sinske WSB ZPiezo stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        if !self.query_presence()? {
            return Err(MmError::DeviceNotFound("WSB ZPiezo".into()));
        }
        self.pos_um.set(self.query_position()?);
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
        if (name == "Port" || name == "Protocol") && self.initialized {
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
        DeviceType::Stage
    }
    fn busy(&self) -> bool {
        let busy = self.query_busy().unwrap_or(self.busy.get());
        if !busy {
            if let Ok(pos) = self.query_position() {
                self.pos_um.set(pos);
            }
        }
        self.busy.set(busy);
        busy
    }
}

impl Stage for WSZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let znm = Self::um_to_steps(z);
        let resp = self.cmd(&format!("MOVE Z {}", znm))?;
        Self::check_ok(&resp)?;
        self.pos_um.set(z);
        self.busy.set(true);
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        let pos = self.query_position()?;
        self.pos_um.set(pos);
        Ok(pos)
    }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let dznm = Self::um_to_steps(dz);
        let resp = self.cmd(&format!("RMOVE Z {}", dznm))?;
        Self::check_ok(&resp)?;
        self.pos_um.set(self.pos_um.get() + dz);
        self.busy.set(true);
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        let resp = if self.protocol() == "WS" {
            self.cmd("[3=HL!]")
        } else {
            self.cmd("HOME LOWER")
        }?;
        Self::check_ok(&resp)?;
        self.busy.set(true);
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let resp = if self.protocol() == "WS" {
            self.cmd("[3=BR!]")
        } else {
            self.cmd("STOP Z")
        }?;
        Self::check_ok(&resp)?;
        self.busy.set(false);
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((self.query_limit("LOWER")?, self.query_limit("UPPER")?))
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

    fn ws_init(pos: &'static str) -> MockTransport {
        MockTransport::new().any(pos).any(pos)
    }

    #[test]
    fn initialize() {
        let t = ws_init("50000"); // 50 µm
        let mut s = WSZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.pos_um.get() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        let t = ws_init("0").any("OK").any("100000");
        let mut s = WSZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 100.0);
    }

    #[test]
    fn move_relative() {
        let t = ws_init("10000").any("OK").any("15000");
        let mut s = WSZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(5.0).unwrap();
        assert!((s.get_position_um().unwrap() - 15.0).abs() < 1e-9);
    }

    #[test]
    fn error_fails() {
        let t = ws_init("0").any("ERR: limit");
        let mut s = WSZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_position_um(999.0).is_err());
    }

    #[test]
    fn malformed_move_ack_does_not_update_cache() {
        let t = ws_init("0").any("DONE");
        let mut s = WSZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(
            s.set_position_um(100.0).unwrap_err(),
            MmError::SerialInvalidResponse
        );
        assert_eq!(s.pos_um.get(), 0.0);
    }

    #[test]
    fn no_transport_error() {
        assert!(WSZStage::new().initialize().is_err());
    }

    #[test]
    fn ws_busy_polls_and_refreshes_when_idle() {
        let t = ws_init("10000").any("0").any("12000");
        let mut s = WSZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(!s.busy());
        assert_eq!(s.pos_um.get(), 12.0);
    }

    #[test]
    fn can_protocol_presence_and_limits() {
        let t = MockTransport::new()
            .any("1")
            .any("25000")
            .any("-1000")
            .any("99000");
        let mut s = WSZStage::new().with_transport(Box::new(t));
        s.set_property("Protocol", PropertyValue::String("CAN".into()))
            .unwrap();
        s.initialize().unwrap();
        assert_eq!(s.get_limits().unwrap(), (-1.0, 99.0));
    }

    #[test]
    fn initialized_port_and_protocol_changes_are_forbidden() {
        let mut s = WSZStage::new().with_transport(Box::new(ws_init("0")));
        s.initialize().unwrap();
        assert_eq!(
            s.set_property("Port", PropertyValue::String("COM2".into()))
                .unwrap_err(),
            MmError::CanNotSetProperty
        );
        assert_eq!(
            s.set_property("Protocol", PropertyValue::String("CAN".into()))
                .unwrap_err(),
            MmError::CanNotSetProperty
        );
    }

    #[test]
    fn um_to_steps_truncates_like_cpp_int_cast() {
        assert_eq!(WSZStage::um_to_steps(1.9999), 1999);
        assert_eq!(WSZStage::um_to_steps(-1.9999), -1999);
    }

    #[test]
    fn failed_initialized_motion_property_write_does_not_update_cache() {
        let t = ws_init("0").any("ERR: rejected");
        let mut s = WSZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s
            .set_property("Velocity (micron/s)", PropertyValue::Float(25.0))
            .is_err());
        assert_eq!(
            s.get_property("Velocity (micron/s)").unwrap(),
            PropertyValue::Float(0.0)
        );
    }
}
