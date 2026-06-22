/// Newport SMC100 single-axis motion controller.
///
/// Protocol (`\r\n` terminator, address prefix "1"):
///   `1ID\r\n`         → "1IDSMC100CC..."  (identity)
///   `1TP\r\n`         → "1TP<value>"  (current position, mm)
///   `1PA<+mm.6f>\r\n` → absolute move (mm)
///   `1PR<+mm.6f>\r\n` → relative move (mm)
///   `1OR\r\n`         → home search
///   `1ST\r\n`         → stop
///   `1ZT\r\n`         → "1ZT<min> <max>" (travel limits, mm)
///   `1TS\r\n`         → "1TS000000XX" (last 2 hex chars = state code)
///                        0x32/0x33/0x34 = READY
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::RefCell;

pub struct NewportSmc {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    conversion_factor: f64,
    controller_address: u8,
    lower_limit: f64,
    upper_limit: f64,
    velocity_upper_limit: f64,
}

impl NewportSmc {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_property("Identity", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_pre_init_property("Conversion Factor", PropertyValue::Float(1000.0))
            .unwrap();
        props
            .define_pre_init_property("Max Position (mm)", PropertyValue::Float(100.0))
            .unwrap();
        props
            .define_pre_init_property("Controller Address", PropertyValue::Integer(1))
            .unwrap();
        props
            .set_property_limits("Controller Address", 1.0, 31.0)
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            conversion_factor: 1000.0,
            controller_address: 1,
            lower_limit: 0.0,
            upper_limit: 25.0,
            velocity_upper_limit: 100_000_000_000.0,
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

    fn make_command(&self, command: &str) -> String {
        format!("{}{}", self.controller_address, command)
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let c = format!("{}\r\n", command);
        self.call_transport(|t| {
            let r = t.send_recv(&c)?;
            Ok(r.trim().to_string())
        })
    }

    fn controller_cmd(&self, command: &str) -> MmResult<String> {
        self.cmd(&self.make_command(command))
    }

    fn send_controller_cmd(&self, command: &str) -> MmResult<()> {
        let c = format!("{}\r\n", self.make_command(command));
        self.call_transport(|t| t.send(&c))
    }

    /// Parse "1TP<value>" → µm
    fn parse_position(&self, resp: &str) -> MmResult<f64> {
        let s = resp.trim();
        let prefix = self.make_command("TP");
        let val_str = if s.starts_with(&prefix) {
            &s[prefix.len()..]
        } else {
            s
        };
        val_str
            .parse::<f64>()
            .map(|mm| mm * self.conversion_factor)
            .map_err(|_| MmError::LocallyDefined(format!("Cannot parse position: {}", s)))
    }

    /// Parse "1ZT<min> <max>" → (min_um, max_um)
    fn parse_limits(&self, resp: &str) -> Option<(f64, f64)> {
        let s = resp.trim();
        let prefix = self.make_command("ZT");
        let s = if s.starts_with(&prefix) {
            &s[prefix.len()..]
        } else {
            s
        };
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() >= 2 {
            let min = parts[0].parse::<f64>().ok()?;
            let max = parts[1].parse::<f64>().ok()?;
            Some((min, max))
        } else {
            None
        }
    }

    fn read_controller_info(&mut self) -> MmResult<()> {
        self.send_controller_cmd("ZT")?;
        loop {
            let answer = self.call_transport(|t| t.receive_line())?;
            let s = answer.trim();
            if let Some((min, max)) = self.parse_limits(s) {
                self.lower_limit = min;
                self.upper_limit = self.upper_limit.min(max);
            }
            let offset = if self.controller_address > 9 { 2 } else { 1 };
            if s.len() >= offset + 2 {
                let code = &s[offset..];
                if let Some(rest) = code.strip_prefix("SL") {
                    if let Ok(v) = rest.parse::<f64>() {
                        self.lower_limit = v;
                    }
                } else if let Some(rest) = code.strip_prefix("SR") {
                    if let Ok(v) = rest.parse::<f64>() {
                        self.upper_limit = self.upper_limit.min(v);
                    }
                } else if let Some(rest) = code.strip_prefix("VA") {
                    if let Ok(v) = rest.parse::<f64>() {
                        self.velocity_upper_limit = v;
                    }
                } else if code.starts_with("PW0") {
                    break;
                }
            }
            if s.starts_with(&self.make_command("ZT")) {
                break;
            }
        }
        Ok(())
    }

    fn moving_from_status(resp: &str) -> bool {
        let s = resp.trim();
        s.len() >= 8 && s[7..].parse::<i32>().map(|v| v == 28).unwrap_or(false)
    }

    fn check_error(&self) -> MmResult<()> {
        self.check_error_inner(false)
    }

    fn check_error_inner(&self, retried_home: bool) -> MmResult<()> {
        let cmd = self.make_command("TE");
        let answer = self.cmd(&cmd)?;
        let code = answer
            .trim()
            .strip_prefix(&cmd)
            .and_then(|s| s.chars().next())
            .ok_or_else(|| MmError::LocallyDefined(format!("Bad SMC TE response: {}", answer)))?;
        match code {
            '@' => Ok(()),
            'H' => {
                if retried_home {
                    return Err(MmError::LocallyDefined(
                        "Controller remained unreferenced after home".into(),
                    ));
                }
                self.send_controller_cmd("OR")?;
                self.wait_for_not_busy()?;
                self.check_error_inner(true)
            }
            _ => Err(MmError::LocallyDefined(format!(
                "Controller reported error code: {}",
                code
            ))),
        }
    }

    fn wait_for_not_busy(&self) -> MmResult<()> {
        for _ in 0..1000 {
            if !self.busy() {
                return Ok(());
            }
        }
        Err(MmError::SerialTimeout)
    }
}

impl Default for NewportSmc {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for NewportSmc {
    fn name(&self) -> &str {
        "NewportZStage"
    }
    fn description(&self) -> &str {
        "Newport SMC100CC controller adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        if let PropertyValue::Float(v) = self.props.get("Conversion Factor")?.clone() {
            self.conversion_factor = v;
        }
        if let PropertyValue::Float(v) = self.props.get("Max Position (mm)")?.clone() {
            self.upper_limit = v;
        }
        if let PropertyValue::Integer(v) = self.props.get("Controller Address")?.clone() {
            self.controller_address = u8::try_from(v).map_err(|_| MmError::InvalidPropertyValue)?;
        }
        let _ = self.controller_cmd("TS")?;
        self.read_controller_info()?;
        self.send_controller_cmd("OR")?;
        self.wait_for_not_busy()?;
        let _ = self.check_error();
        let id = self.controller_cmd("ID")?;
        self.props
            .entry_mut("Identity")
            .map(|e| e.value = PropertyValue::String(id));
        let pos = self.get_position_um()?;
        self.props
            .define_property("Position", PropertyValue::Float(pos), false)
            .unwrap();
        self.props
            .define_property("Velocity in mm per sec", PropertyValue::Float(0.0), false)
            .unwrap();
        self.props
            .set_property_limits(
                "Velocity in mm per sec",
                0.000001,
                self.velocity_upper_limit,
            )
            .unwrap();
        if let Ok(version) = self.controller_cmd("VE") {
            let prefix = self.make_command("VE");
            let version = version
                .strip_prefix(&prefix)
                .unwrap_or(&version)
                .trim()
                .to_string();
            self.props
                .define_property("Controller version", PropertyValue::String(version), true)
                .unwrap();
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Position" => Ok(PropertyValue::Float(self.get_position_um()?)),
            "Velocity in mm per sec" => {
                let resp = self.controller_cmd("VA?")?;
                let prefix = self.make_command("VA");
                let value = resp
                    .trim()
                    .strip_prefix(&prefix)
                    .unwrap_or(resp.trim())
                    .parse::<f64>()
                    .map_err(|_| MmError::LocallyDefined(format!("Bad SMC velocity: {}", resp)))?;
                Ok(PropertyValue::Float(value))
            }
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Port" && self.initialized {
            return Err(MmError::InvalidPropertyValue);
        }
        match (name, val) {
            ("Position", PropertyValue::Float(v)) => self.set_position_um(v),
            ("Velocity in mm per sec", PropertyValue::Float(v)) => {
                self.send_controller_cmd(&format!("VA{}", v))?;
                self.props.set(name, PropertyValue::Float(v))
            }
            (name, val) => self.props.set(name, val),
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
        self.controller_cmd("TS")
            .map(|resp| Self::moving_from_status(&resp))
            .unwrap_or(true)
    }
}

impl Stage for NewportSmc {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        self.wait_for_not_busy()?;
        let pos = z / self.conversion_factor;
        if pos > self.upper_limit || self.lower_limit > pos {
            return Err(MmError::InvalidPropertyValue);
        }
        self.send_controller_cmd(&format!("PA{}", pos))?;
        self.check_error()?;
        Ok(())
    }
    fn get_position_um(&self) -> MmResult<f64> {
        let resp = self.controller_cmd("TP")?;
        self.parse_position(&resp)
    }
    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        self.wait_for_not_busy()?;
        self.send_controller_cmd(&format!("PR{}", dz / self.conversion_factor))?;
        self.check_error()?;
        Ok(())
    }
    fn home(&mut self) -> MmResult<()> {
        self.send_controller_cmd("OR")?;
        self.wait_for_not_busy()
    }
    fn stop(&mut self) -> MmResult<()> {
        let _ = self.send_controller_cmd("ST");
        Ok(())
    }
    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((
            self.lower_limit * self.conversion_factor,
            self.upper_limit * self.conversion_factor,
        ))
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
            .expect("1TS\r\n", "1TS00000000")
            .expect("1ZT\r\n", "1ZT0.000000 25.000000")
            .expect("1TS\r\n", "1TS00000000")
            .expect("1TE\r\n", "1TE@")
            .expect("1ID\r\n", "1IDSMC100CC v2.0")
            .expect("1TP\r\n", "1TP+0.010000")
            .expect("1VE\r\n", "1VE2.0")
    }

    #[test]
    fn initialize() {
        let mut dev = NewportSmc::new()
            .with_transport(Box::new(make_transport().expect("1TP\r\n", "1TP+0.010000")));
        dev.initialize().unwrap();
        // 0.01 mm * 1000 = 10 µm
        assert!((dev.get_position_um().unwrap() - 10.0).abs() < 1e-6);
        assert!((dev.lower_limit - 0.0).abs() < 1e-3);
        assert!((dev.upper_limit - 25.0).abs() < 1e-3);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport()
            .expect("1TS\r\n", "1TS00000000")
            .expect("1TE\r\n", "1TE@")
            .expect("1TP\r\n", "1TP3.000000");
        let mut dev = NewportSmc::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_position_um(3000.0).unwrap();
        assert_eq!(dev.get_position_um().unwrap(), 3000.0);
    }

    #[test]
    fn parse_limits_ok() {
        let dev = NewportSmc::new();
        let (min, max) = dev.parse_limits("1ZT0.000000 25.000000").unwrap();
        assert!((min - 0.0).abs() < 1e-3);
        assert!((max - 25.0).abs() < 1e-3);
    }

    #[test]
    fn no_transport_error() {
        assert!(NewportSmc::new().initialize().is_err());
    }

    #[test]
    fn busy_uses_ts_status() {
        let dev = NewportSmc::new().with_transport(Box::new(
            MockTransport::new().expect("1TS\r\n", "1TS00000028"),
        ));
        assert!(dev.busy());
    }

    #[test]
    fn absolute_move_rejects_out_of_limit_before_command() {
        let mut dev = NewportSmc::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(dev.set_position_um(30_000.0).is_err());
    }

    #[test]
    fn failed_controller_error_does_not_change_live_position() {
        let t = make_transport()
            .expect("1TS\r\n", "1TS00000000")
            .expect("1TE\r\n", "1TEA")
            .expect("1TP\r\n", "1TP0.010000");
        let mut dev = NewportSmc::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert!(dev.set_position_um(3000.0).is_err());
        assert_eq!(dev.get_position_um().unwrap(), 10.0);
    }

    #[test]
    fn unreferenced_error_homes_and_rechecks_error() {
        let t = make_transport()
            .expect("1TS\r\n", "1TS00000000")
            .expect("1TE\r\n", "1TEH")
            .expect("1TS\r\n", "1TS00000000")
            .expect("1TE\r\n", "1TE@")
            .expect("1TP\r\n", "1TP3.000000");
        let mut dev = NewportSmc::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_position_um(3000.0).unwrap();
        assert_eq!(dev.get_position_um().unwrap(), 3000.0);
    }
}
