/// ChuoSeiki MD-5000 single-axis stage.
///
/// The upstream adapter registers this as `ChuoSeiki_MD 1-Axis` and lets the
/// user choose controller axis X or Y.  Commands are the single-axis forms of
/// the MD-5000 XY protocol used by the local XY adapter.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::{Cell, RefCell};

pub struct ChuoSeikiZStage {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    pos_um: Cell<f64>,
    step_size_um: f64,
    speed_step: i64,
    accel_pattern: i64,
    controller_axis: String,
}

impl ChuoSeikiZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "FirmwareVersion",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("Controller Axis", PropertyValue::String("X".into()), false)
            .unwrap();
        props
            .set_allowed_values("Controller Axis", &["X", "Y"])
            .unwrap();
        props
            .define_property("Step Size", PropertyValue::Float(1.0), false)
            .unwrap();
        props
            .define_property("Speed", PropertyValue::Float(1000.0), false)
            .unwrap();
        props
            .define_property("AccelrationTime Pattern", PropertyValue::Float(2.0), false)
            .unwrap();
        props
            .set_allowed_values("AccelrationTime Pattern", &["1", "2", "3", "4"])
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            pos_um: Cell::new(0.0),
            step_size_um: 1.0,
            speed_step: 1000,
            accel_pattern: 2,
            controller_axis: "X".to_string(),
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
        self.call_transport(|t| Ok(t.send_recv(&c)?.trim().to_string()))
    }

    fn check_ok(resp: &str) -> MmResult<()> {
        let s = resp.trim();
        let error_code = if s.len() >= 16 {
            let first = &s[6..8];
            if first == "00" {
                &s[14..16]
            } else {
                first
            }
        } else if s.len() >= 8 {
            &s[6..8]
        } else if s.len() >= 6 {
            &s[4..6]
        } else {
            "00"
        };

        if error_code == "00" {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("ChuoSeiki error: {}", s)))
        }
    }

    fn confirm_version(&mut self) -> MmResult<String> {
        let mut last = String::new();
        for _ in 0..5 {
            let _ = self.cmd("DLM C")?;
            let ver = self.cmd("RVR")?;
            if ver.starts_with("RVR") {
                return Ok(ver);
            }
            last = ver;
        }
        Err(MmError::LocallyDefined(format!(
            "Unexpected RVR response: {}",
            last
        )))
    }

    fn parse_rlp_axis_steps(resp: &str, axis: &str) -> MmResult<i64> {
        let s = resp.trim();
        let Some(rest) = s.strip_prefix("RLP ") else {
            return Err(MmError::LocallyDefined(format!(
                "Cannot parse RLP: {}",
                resp
            )));
        };

        for part in rest.split(',') {
            let mut tokens = part.split_whitespace();
            match (tokens.next(), tokens.next(), tokens.next()) {
                (Some(found_axis), Some(value), None) if found_axis == axis => {
                    return value.parse().map_err(|_| {
                        MmError::LocallyDefined(format!("Cannot parse RLP: {}", resp))
                    });
                }
                _ => {}
            }
        }

        Err(MmError::LocallyDefined(format!(
            "Cannot parse RLP: {}",
            resp
        )))
    }

    fn query_position_um(&self) -> MmResult<f64> {
        let pos_resp = self.cmd("RLP")?;
        let steps = Self::parse_rlp_axis_steps(&pos_resp, &self.controller_axis)?;
        let pos = steps as f64 * self.step_size_um;
        self.pos_um.set(pos);
        Ok(pos)
    }

    fn query_busy(&self) -> MmResult<bool> {
        let resp = self.cmd("RDR")?;
        let status = resp
            .trim()
            .strip_prefix("RDR")
            .unwrap_or(resp.trim())
            .trim();
        if let Ok(busy) = status.parse::<i64>() {
            return Ok(busy != 0);
        }

        let axis = self.controller_axis.as_str();
        let tokens: Vec<&str> = status
            .split(|c: char| c.is_ascii_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .collect();
        for pair in tokens.windows(2) {
            if pair[0] == axis {
                return Ok(pair[1] == "1");
            }
        }

        Err(MmError::LocallyDefined(format!(
            "Cannot parse RDR: {}",
            resp
        )))
    }

    fn apply_controller_property(&mut self, name: &str, val: &PropertyValue) -> MmResult<()> {
        match (name, val) {
            ("Controller Axis", PropertyValue::String(axis)) => {
                if axis != "X" && axis != "Y" {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.controller_axis = axis.clone();
            }
            ("Step Size", PropertyValue::Float(v)) => {
                if *v <= 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.step_size_um = *v;
            }
            ("Speed", PropertyValue::Float(v)) => {
                let speed = *v as i64;
                if speed <= 0 || speed >= 20_000 {
                    return Err(MmError::LocallyDefined("ChuoSeiki parameter error".into()));
                }
                let r = self.cmd(&format!("SPD {} {}", self.controller_axis, speed))?;
                Self::check_ok(&r)?;
                self.speed_step = speed;
            }
            ("AccelrationTime Pattern", PropertyValue::Float(v)) => {
                let pattern = *v as i64;
                let r = self.cmd(&format!("SAP {} {}", self.controller_axis, pattern))?;
                Self::check_ok(&r)?;
                self.accel_pattern = pattern;
            }
            _ => {}
        }
        Ok(())
    }
}

impl Default for ChuoSeikiZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ChuoSeikiZStage {
    fn name(&self) -> &str {
        "ChuoSeiki_MD 1-Axis"
    }

    fn description(&self) -> &str {
        "Z Stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let ver = self.confirm_version()?;
        self.props
            .entry_mut("FirmwareVersion")
            .map(|e| e.value = PropertyValue::String(ver));
        self.query_position_um()?;
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
            return Err(MmError::InvalidPropertyValue);
        }
        self.apply_controller_property(name, &val)?;
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
        self.query_busy().unwrap_or(false)
    }
}

impl Stage for ChuoSeikiZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = (z / self.step_size_um) as i64;
        let r = self.cmd(&format!("ABA {} {}", self.controller_axis, steps))?;
        Self::check_ok(&r)?;
        self.pos_um.set(steps as f64 * self.step_size_um);
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        self.query_position_um()
    }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let steps = (dz / self.step_size_um) as i64;
        let r = self.cmd(&format!("ICA {} {}", self.controller_axis, steps))?;
        Self::check_ok(&r)?;
        self.pos_um
            .set(self.pos_um.get() + steps as f64 * self.step_size_um);
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn stop(&mut self) -> MmResult<()> {
        let r = self.cmd(&format!("SST {}", self.controller_axis))?;
        Self::check_ok(&r)
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
            .any("OK")
            .any("RVR MD5000 v1.2")
            .any("RLP X 100,Y 200")
    }

    #[test]
    fn initialize_registers_upstream_surface() {
        let t = make_transport().expect("RLP\r\n", "RLP X 100,Y 200");
        let mut s = ChuoSeikiZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.name(), "ChuoSeiki_MD 1-Axis");
        assert_eq!(s.description(), "Z Stage");
        assert_eq!(s.device_type(), DeviceType::Stage);
        assert!(s.has_property("Controller Axis"));
        assert!(s.has_property("AccelrationTime Pattern"));
        assert_eq!(s.get_position_um().unwrap(), 100.0);
    }

    #[test]
    fn y_axis_position_can_be_selected() {
        let t = make_transport().expect("RLP\r\n", "RLP X 100,Y 200");
        let mut s = ChuoSeikiZStage::new().with_transport(Box::new(t));
        s.set_property("Controller Axis", PropertyValue::String("Y".to_string()))
            .unwrap();
        s.initialize().unwrap();
        assert_eq!(s.get_position_um().unwrap(), 200.0);
    }

    #[test]
    fn move_absolute_and_relative_use_single_axis_commands() {
        let t = make_transport()
            .expect("ABA X 300\r\n", "ABA X 00")
            .expect("ICA X -25\r\n", "ICA X 00")
            .expect("RLP\r\n", "RLP X 275,Y 0");
        let mut s = ChuoSeikiZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(300.0).unwrap();
        s.set_relative_position_um(-25.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 275.0);
    }

    #[test]
    fn stop_uses_sst_and_home_is_unsupported() {
        let t = make_transport().expect("SST X\r\n", "SST 00");
        let mut s = ChuoSeikiZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.home(), Err(MmError::UnsupportedCommand));
        s.stop().unwrap();
    }

    #[test]
    fn speed_property_sends_spd() {
        let t = make_transport().expect("SPD X 1200\r\n", "SPD X 00");
        let mut s = ChuoSeikiZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("Speed", PropertyValue::Float(1200.0))
            .unwrap();
        assert_eq!(
            s.get_property("Speed").unwrap(),
            PropertyValue::Float(1200.0)
        );
    }

    #[test]
    fn busy_polls_selected_axis() {
        let t = make_transport().expect("RDR\r\n", "RDR X 1,Y 0");
        let mut s = ChuoSeikiZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
    }

    #[test]
    fn port_cannot_change_after_initialize() {
        let mut s = ChuoSeikiZStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        assert_eq!(
            s.set_property("Port", PropertyValue::String("COM2".to_string()))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }
}
