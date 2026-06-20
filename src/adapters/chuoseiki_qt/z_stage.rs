/// ChuoSeiki QT single-axis (Z) stage.
///
/// Protocol (CR+LF terminated):
///   `?:CHUOSEIKI\r\n`       → "CHUOSEIKI\r\n"
///   `AGO:A<z>\r\n`          → OK or `!<n>`
///   `MGO:A<dz>\r\n`         → OK or `!<n>`
///   `Q:A0\r\n`              → `<+/->XXXXXXXXD` (position + state)
///   `H:A\r\n`               → OK or `!<n>`
///
/// Step size default: 1 µm/step.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::{Cell, RefCell};

const DEFAULT_STEP_UM: f64 = 1.0;

pub struct ChuoSeikiQTZStage {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    pos_um: Cell<f64>,
    step_um: f64,
    speed_high: i64,
    speed_low: i64,
    accel_time: i64,
    controller_axis: String,
}

impl ChuoSeikiQTZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "ChuoSeikiAxisName",
                PropertyValue::String("A".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("ChuoSeikiAxisName", &["A", "B", "C"])
            .unwrap();
        props
            .define_property("Step Size: um", PropertyValue::Float(1.0), false)
            .unwrap();
        props
            .define_property("High Speed: pps", PropertyValue::Float(2000.0), false)
            .unwrap();
        props
            .define_property("Low Speed: pps", PropertyValue::Float(500.0), false)
            .unwrap();
        props
            .define_property(
                "Accelerating Time: msec",
                PropertyValue::Float(100.0),
                false,
            )
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            pos_um: Cell::new(0.0),
            step_um: DEFAULT_STEP_UM,
            speed_high: 2000,
            speed_low: 500,
            accel_time: 100,
            controller_axis: "A".to_string(),
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
        let c = format!("{}\r\n", command);
        self.call_transport(|t| {
            let r = t.send_recv(&c)?;
            Ok(r.trim().to_string())
        })
    }

    fn check_response(resp: &str) -> MmResult<()> {
        if resp.starts_with('!') {
            Err(MmError::LocallyDefined(format!(
                "ChuoSeiki QT error: {}",
                resp
            )))
        } else {
            Ok(())
        }
    }

    fn parse_axis_position_response(resp: &str, step_um: f64) -> MmResult<(f64, bool)> {
        if resp.len() < 10 {
            return Err(MmError::LocallyDefined(format!(
                "ChuoSeiki QT: unexpected position response: {}",
                resp
            )));
        }
        let steps: i64 = resp[..9]
            .trim()
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        let state = &resp[9..10];
        if state == "H" {
            return Err(MmError::LocallyDefined(
                "ChuoSeiki QT: homing error".to_string(),
            ));
        }
        Ok((steps as f64 * step_um, state == "D"))
    }

    fn parse_busy_response(resp: &str) -> MmResult<bool> {
        if resp.contains('D') {
            Ok(true)
        } else if resp.contains('K') || resp.contains('H') {
            Ok(false)
        } else {
            Err(MmError::LocallyDefined(format!(
                "ChuoSeiki QT: unexpected busy response: {}",
                resp
            )))
        }
    }

    fn query_position_um(&self) -> MmResult<f64> {
        for _ in 0..5 {
            let resp = self.cmd(&format!("Q:{}0", self.controller_axis))?;
            let (pos, moving) = Self::parse_axis_position_response(&resp, self.step_um)?;
            self.pos_um.set(pos);
            if !moving {
                return Ok(pos);
            }
        }
        Ok(self.pos_um.get())
    }

    fn query_busy(&self) -> MmResult<bool> {
        let resp = self.cmd(&format!("Q:{}2", self.controller_axis))?;
        Self::parse_busy_response(&resp)
    }

    fn wait_until_idle(&self) -> MmResult<()> {
        for _ in 0..100 {
            if !self.query_busy()? {
                return Ok(());
            }
        }
        Err(MmError::LocallyDefined(
            "ChuoSeiki QT: timeout waiting for stage".to_string(),
        ))
    }

    fn set_positive_float_property(&mut self, name: &str, val: PropertyValue) -> MmResult<f64> {
        let value = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
        if value <= 0.0 {
            return Err(MmError::InvalidPropertyValue);
        }
        self.props.set(name, PropertyValue::Float(value))?;
        Ok(value)
    }

    fn set_i64_property(&mut self, name: &str, val: PropertyValue) -> MmResult<i64> {
        let value = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
        self.props.set(name, PropertyValue::Float(value as f64))?;
        Ok(value)
    }

    fn apply_axis_speed(&mut self) -> MmResult<()> {
        let r = self.cmd(&format!(
            "D:{}{}P{}P{}",
            self.controller_axis, self.speed_low, self.speed_high, self.accel_time
        ))?;
        Self::check_response(&r)
    }
}

impl Default for ChuoSeikiQTZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ChuoSeikiQTZStage {
    fn name(&self) -> &str {
        "ChuoSeiki_QT 1-Axis"
    }
    fn description(&self) -> &str {
        "ChuoSeiki 1-stage driver"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let resp = self.cmd("?:CHUOSEIKI")?;
        if !resp.starts_with("CHUOSEIKI") {
            return Err(MmError::LocallyDefined(format!(
                "ChuoSeiki QT: unexpected identity: {}",
                resp
            )));
        }
        let _ = self.cmd("X:1");
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "ChuoSeikiAxisName" => Ok(PropertyValue::String(self.controller_axis.clone())),
            "Step Size: um" => Ok(PropertyValue::Float(self.step_um)),
            "High Speed: pps" => Ok(PropertyValue::Float(self.speed_high as f64)),
            "Low Speed: pps" => Ok(PropertyValue::Float(self.speed_low as f64)),
            "Accelerating Time: msec" => Ok(PropertyValue::Float(self.accel_time as f64)),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "ChuoSeikiAxisName" => {
                self.props.set(name, val)?;
                self.controller_axis = self.props.get(name)?.as_str().to_string();
                Ok(())
            }
            "Step Size: um" => {
                self.step_um = self.set_positive_float_property(name, val)?;
                Ok(())
            }
            "High Speed: pps" => {
                self.speed_high = self.set_i64_property(name, val)?;
                self.apply_axis_speed()
            }
            "Low Speed: pps" => {
                self.speed_low = self.set_i64_property(name, val)?;
                self.apply_axis_speed()
            }
            "Accelerating Time: msec" => {
                self.accel_time = self.set_i64_property(name, val)?;
                self.apply_axis_speed()
            }
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
        DeviceType::Stage
    }
    fn busy(&self) -> bool {
        self.query_busy().unwrap_or(false)
    }
}

impl Stage for ChuoSeikiQTZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = (z / self.step_um) as i64;
        let r = self.cmd(&format!("AGO:{}{}", self.controller_axis, steps))?;
        Self::check_response(&r)?;
        self.wait_until_idle()?;
        self.pos_um.set(z);
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        self.query_position_um()
    }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let steps = (dz / self.step_um) as i64;
        let r = self.cmd(&format!("MGO:{}{}", self.controller_axis, steps))?;
        Self::check_response(&r)?;
        self.pos_um.set(self.pos_um.get() + dz);
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn stop(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Err(MmError::UnsupportedCommand)
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

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any("CHUOSEIKI") // identity
            .any("OK") // X:1
    }

    #[test]
    fn initialize() {
        let t = make_transport().expect("Q:A0\r\n", "+00000000K");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_position_um().unwrap(), 0.0);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport()
            .expect("AGO:A100\r\n", "OK")
            .expect("Q:A2\r\n", "K")
            .expect("Q:A0\r\n", "+00000100K");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 100.0);
    }

    #[test]
    fn move_relative() {
        let t = make_transport().any("OK").expect("Q:A0\r\n", "+00000025K");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(25.0).unwrap();
        assert!((s.get_position_um().unwrap() - 25.0).abs() < 1e-9);
    }

    #[test]
    fn error_response_fails() {
        let t = make_transport().any("!6");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.set_position_um(999_999.0).is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(ChuoSeikiQTZStage::new().initialize().is_err());
    }

    #[test]
    fn axis_property_controls_commands() {
        let t = make_transport()
            .expect("AGO:B10\r\n", "OK")
            .expect("Q:B2\r\n", "K");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.set_property("ChuoSeikiAxisName", PropertyValue::String("B".to_string()))
            .unwrap();
        s.initialize().unwrap();
        s.set_position_um(10.0).unwrap();
    }

    #[test]
    fn busy_polls_selected_axis_state() {
        let t = make_transport().expect("Q:B2\r\n", "+00000000D");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.set_property("ChuoSeikiAxisName", PropertyValue::String("B".to_string()))
            .unwrap();
        s.initialize().unwrap();
        assert!(s.busy());
    }

    #[test]
    fn homing_state_is_not_busy() {
        let t = make_transport().expect("Q:B2\r\n", "+00000000H");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.set_property("ChuoSeikiAxisName", PropertyValue::String("B".to_string()))
            .unwrap();
        s.initialize().unwrap();
        assert!(!s.busy());
    }

    #[test]
    fn get_position_reads_live_selected_axis() {
        let t = make_transport().expect("Q:C0\r\n", "+00000123K");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.set_property("ChuoSeikiAxisName", PropertyValue::String("C".to_string()))
            .unwrap();
        s.initialize().unwrap();
        assert_eq!(s.get_position_um().unwrap(), 123.0);
    }

    #[test]
    fn get_position_retries_while_axis_state_is_moving() {
        let t = make_transport()
            .expect("Q:C0\r\n", "+00000100D")
            .expect("Q:C0\r\n", "+00000123K");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.set_property("ChuoSeikiAxisName", PropertyValue::String("C".to_string()))
            .unwrap();
        s.initialize().unwrap();
        assert_eq!(s.get_position_um().unwrap(), 123.0);
    }

    #[test]
    fn get_position_reports_homing_error() {
        let t = make_transport().expect("Q:C0\r\n", "+00000123H");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.set_property("ChuoSeikiAxisName", PropertyValue::String("C".to_string()))
            .unwrap();
        s.initialize().unwrap();
        assert!(s.get_position_um().is_err());
    }

    #[test]
    fn unsupported_stage_methods_match_upstream() {
        let mut s = ChuoSeikiQTZStage::new();
        assert_eq!(s.home().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(s.stop().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(s.get_limits().unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn speed_properties_send_axis_d_commands() {
        let t = make_transport()
            .expect("D:B500P2500P100\r\n", "OK")
            .expect("D:B400P2500P100\r\n", "OK")
            .expect("D:B400P2500P150\r\n", "OK");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.set_property("ChuoSeikiAxisName", PropertyValue::String("B".to_string()))
            .unwrap();
        s.initialize().unwrap();
        s.set_property("High Speed: pps", PropertyValue::Float(2500.0))
            .unwrap();
        s.set_property("Low Speed: pps", PropertyValue::Float(400.0))
            .unwrap();
        s.set_property("Accelerating Time: msec", PropertyValue::Float(150.0))
            .unwrap();
    }

    #[test]
    fn absolute_move_waits_until_not_busy() {
        let t = make_transport()
            .expect("AGO:B100\r\n", "OK")
            .expect("Q:B2\r\n", "D")
            .expect("Q:B2\r\n", "K");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.set_property("ChuoSeikiAxisName", PropertyValue::String("B".to_string()))
            .unwrap();
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
    }
}
