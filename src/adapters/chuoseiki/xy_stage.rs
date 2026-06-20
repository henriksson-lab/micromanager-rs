/// ChuoSeiki MD-5000 XY stage controller.
///
/// Protocol (CRLF-terminated):
///   `DLM C\r\n`       → (no meaningful response — clear error)
///   `RVR\r\n`         → "RVR <firmware>\r\n" (version check)
///   `ABA X <steps>,Y <steps>\r\n`→ "... 00" (last 2 chars = error code; "00" = OK)
///   `ICA X <steps>,Y <steps>\r\n`→ same, relative move
///   `HMB X,Y\r\n`      → same, home search
///   `SST X,Y\r\n`      → same, deceleration stop
///   `RLP\r\n`         → "RLP X <steps>,Y <steps>\r\n"
///
/// Step size: 1 µm/step (assumed; MD-5000 default resolution).
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

pub struct ChuoSeikiXYStage {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    x_um: f64,
    y_um: f64,
    step_size_um_x: f64,
    step_size_um_y: f64,
    speed_step_x: i64,
    speed_step_y: i64,
    accel_pattern_x: i64,
    accel_pattern_y: i64,
}

impl ChuoSeikiXYStage {
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
            .define_property("X Step Size", PropertyValue::Float(1.0), false)
            .unwrap();
        props
            .define_property("X Speed", PropertyValue::Float(1000.0), false)
            .unwrap();
        props
            .define_property("X Accel. pattern", PropertyValue::Float(2.0), false)
            .unwrap();
        props
            .define_property("Y Step Size", PropertyValue::Float(1.0), false)
            .unwrap();
        props
            .define_property("Y Speed", PropertyValue::Float(1000.0), false)
            .unwrap();
        props
            .define_property("Y Accel. pattern", PropertyValue::Float(2.0), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            x_um: 0.0,
            y_um: 0.0,
            step_size_um_x: 1.0,
            step_size_um_y: 1.0,
            speed_step_x: 1000,
            speed_step_y: 1000,
            accel_pattern_x: 2,
            accel_pattern_y: 2,
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
            Some(t) => {
                let mut t = t.borrow_mut();
                f(t.as_mut())
            }
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

    /// Check that the last 2 chars of response are "00" (no error).
    fn check_ok(resp: &str) -> MmResult<()> {
        let s = resp.trim();
        if s.len() >= 2 && &s[s.len() - 2..] == "00" {
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

    /// Parse "RLP X <steps>,Y <steps>" → (x_steps, y_steps)
    fn parse_rlp_steps(resp: &str) -> MmResult<(i64, i64)> {
        let s = resp.trim();
        let Some(rest) = s.strip_prefix("RLP ") else {
            return Err(MmError::LocallyDefined(format!(
                "Cannot parse RLP: {}",
                resp
            )));
        };
        let mut parts = rest.split(',');
        let x_part = parts
            .next()
            .ok_or_else(|| MmError::LocallyDefined(format!("Cannot parse RLP: {}", resp)))?;
        let y_part = parts
            .next()
            .ok_or_else(|| MmError::LocallyDefined(format!("Cannot parse RLP: {}", resp)))?;
        if parts.next().is_some() {
            return Err(MmError::LocallyDefined(format!(
                "Cannot parse RLP: {}",
                resp
            )));
        }

        let parse_axis = |part: &str, axis: &str| -> MmResult<i64> {
            let mut tokens = part.split_whitespace();
            match (tokens.next(), tokens.next(), tokens.next()) {
                (Some(found_axis), Some(value), None) if found_axis == axis => value
                    .parse()
                    .map_err(|_| MmError::LocallyDefined(format!("Cannot parse RLP: {}", resp))),
                _ => Err(MmError::LocallyDefined(format!(
                    "Cannot parse RLP: {}",
                    resp
                ))),
            }
        };

        Ok((parse_axis(x_part, "X")?, parse_axis(y_part, "Y")?))
    }

    fn query_position(&self) -> MmResult<(f64, f64)> {
        let pos_resp = self.cmd("RLP")?;
        let (x_steps, y_steps) = Self::parse_rlp_steps(&pos_resp)?;
        Ok((
            x_steps as f64 * self.step_size_um_x,
            y_steps as f64 * self.step_size_um_y,
        ))
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

        let mut saw_idle_flag = false;
        for token in status.split(|c: char| !c.is_ascii_digit() && c != '-') {
            if token.is_empty() {
                continue;
            }
            match token.parse::<i64>() {
                Ok(1) => return Ok(true),
                Ok(0) => saw_idle_flag = true,
                Ok(_) => {}
                Err(_) => {}
            }
        }
        if saw_idle_flag {
            Ok(false)
        } else {
            Err(MmError::LocallyDefined(format!(
                "Cannot parse RDR: {}",
                resp
            )))
        }
    }

    fn apply_controller_property(&mut self, name: &str, val: &PropertyValue) -> MmResult<()> {
        match (name, val) {
            ("X Step Size", PropertyValue::Float(v)) => self.step_size_um_x = *v,
            ("Y Step Size", PropertyValue::Float(v)) => self.step_size_um_y = *v,
            ("X Speed", PropertyValue::Float(v)) => {
                let speed = *v as i64;
                if speed <= 0 || speed >= 20_000 {
                    return Err(MmError::LocallyDefined("ChuoSeiki parameter error".into()));
                }
                let r = self.cmd(&format!("SPD X {}", speed))?;
                Self::check_ok(&r)?;
                self.speed_step_x = speed;
            }
            ("Y Speed", PropertyValue::Float(v)) => {
                let speed = *v as i64;
                if speed <= 0 || speed >= 20_000 {
                    return Err(MmError::LocallyDefined("ChuoSeiki parameter error".into()));
                }
                let r = self.cmd(&format!("SPD Y {}", speed))?;
                Self::check_ok(&r)?;
                self.speed_step_y = speed;
            }
            ("X Accel. pattern", PropertyValue::Float(v)) => {
                let pattern = *v as i64;
                let r = self.cmd(&format!("SAP X {}", pattern))?;
                Self::check_ok(&r)?;
                self.accel_pattern_x = pattern;
            }
            ("Y Accel. pattern", PropertyValue::Float(v)) => {
                let pattern = *v as i64;
                let r = self.cmd(&format!("SAP Y {}", pattern))?;
                Self::check_ok(&r)?;
                self.accel_pattern_y = pattern;
            }
            _ => {}
        }
        Ok(())
    }

    fn move_absolute_steps(&mut self, x_steps: i64, y_steps: i64) -> MmResult<()> {
        let r = self.cmd(&format!("ABA X {},Y {}", x_steps, y_steps))?;
        Self::check_ok(&r)
    }

    fn move_relative_steps(&mut self, x_steps: i64, y_steps: i64) -> MmResult<()> {
        let r = self.cmd(&format!("ICA X {},Y {}", x_steps, y_steps))?;
        Self::check_ok(&r)
    }
}

impl Default for ChuoSeikiXYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ChuoSeikiXYStage {
    fn name(&self) -> &str {
        "ChuoSeiki_MD 2-Axis"
    }
    fn description(&self) -> &str {
        "XY Stages"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let ver = self.confirm_version()?;
        self.props
            .entry_mut("FirmwareVersion")
            .map(|e| e.value = PropertyValue::String(ver));
        let (x, y) = self.query_position()?;
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
        DeviceType::XYStage
    }
    fn busy(&self) -> bool {
        self.query_busy().unwrap_or(false)
    }
}

impl XYStage for ChuoSeikiXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let x_steps = (x / self.step_size_um_x).round() as i64;
        let y_steps = (y / self.step_size_um_y).round() as i64;
        self.move_absolute_steps(x_steps, y_steps)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }
    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        self.query_position()
    }
    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let x_steps = (dx / self.step_size_um_x).round() as i64;
        let y_steps = (dy / self.step_size_um_y).round() as i64;
        self.move_relative_steps(x_steps, y_steps)?;
        self.x_um += x_steps as f64 * self.step_size_um_x;
        self.y_um += y_steps as f64 * self.step_size_um_y;
        Ok(())
    }
    fn home(&mut self) -> MmResult<()> {
        let r = self.cmd("HMB X,Y")?;
        Self::check_ok(&r)?;
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> {
        let r = self.cmd("SST X,Y")?;
        Self::check_ok(&r)
    }
    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Err(MmError::UnsupportedCommand)
    }
    fn get_step_size_um(&self) -> (f64, f64) {
        (self.step_size_um_x, self.step_size_um_y)
    }
    fn set_origin(&mut self) -> MmResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any("OK") // DLM C
            .any("RVR MD5000 v1.2") // RVR
            .any("RLP X 100,Y 200") // RLP
    }

    #[test]
    fn initialize() {
        let t = make_transport().expect("RLP\r\n", "RLP X 100,Y 200");
        let mut s = ChuoSeikiXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.name(), "ChuoSeiki_MD 2-Axis");
        assert_eq!(s.description(), "XY Stages");
        assert_eq!(s.get_xy_position_um().unwrap(), (100.0, 200.0));
    }

    #[test]
    fn move_absolute() {
        let t = make_transport()
            .expect("ABA X 300,Y 400\r\n", "ABA X 00ERS Y 00")
            .expect("RLP\r\n", "RLP X 300,Y 400");
        let mut s = ChuoSeikiXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_xy_position_um(300.0, 400.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (300.0, 400.0));
    }

    #[test]
    fn move_relative_uses_ica() {
        let t = make_transport()
            .expect("ICA X 3,Y -4\r\n", "ICA X 00ERS Y 00")
            .expect("RLP\r\n", "RLP X 103,Y 196");
        let mut s = ChuoSeikiXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_xy_position_um(3.0, -4.0).unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (103.0, 196.0));
    }

    #[test]
    fn home_and_stop_use_controller_commands() {
        let t = make_transport()
            .expect("HMB X,Y\r\n", "HMB 00")
            .expect("SST X,Y\r\n", "SST 00")
            .expect("RLP\r\n", "RLP X 0,Y 0");
        let mut s = ChuoSeikiXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        s.stop().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn speed_property_sends_spd() {
        let t = make_transport().expect("SPD X 1200\r\n", "SPD X 00");
        let mut s = ChuoSeikiXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("X Speed", PropertyValue::Float(1200.0))
            .unwrap();
        assert_eq!(
            s.get_property("X Speed").unwrap(),
            PropertyValue::Float(1200.0)
        );
    }

    #[test]
    fn parse_rlp_ok() {
        let (x, y) = ChuoSeikiXYStage::parse_rlp_steps("RLP X 1000,Y -500").unwrap();
        assert_eq!(x, 1000);
        assert_eq!(y, -500);
    }

    #[test]
    fn check_ok_passes() {
        assert!(ChuoSeikiXYStage::check_ok("ABA X 100 00").is_ok());
        assert!(ChuoSeikiXYStage::check_ok("ABA X 100 01").is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(ChuoSeikiXYStage::new().initialize().is_err());
    }

    #[test]
    fn port_cannot_change_after_initialize() {
        let mut s = ChuoSeikiXYStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        assert_eq!(
            s.set_property("Port", PropertyValue::String("COM2".to_string()))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn get_position_reads_live_rlp() {
        let t = make_transport().expect("RLP\r\n", "RLP X 321,Y -654");
        let mut s = ChuoSeikiXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (321.0, -654.0));
    }

    #[test]
    fn busy_polls_rdr_status() {
        let t = make_transport().expect("RDR\r\n", "RDR 1");
        let mut s = ChuoSeikiXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
    }

    #[test]
    fn busy_parses_axis_flags_from_rdr_response() {
        let t = make_transport().expect("RDR\r\n", "RDR X 0, Y 1");
        let mut s = ChuoSeikiXYStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
    }
}
