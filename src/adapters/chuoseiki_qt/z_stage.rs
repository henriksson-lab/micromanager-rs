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

const DEFAULT_STEP_UM: f64 = 1.0;

pub struct ChuoSeikiQTZStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    pos_um: f64,
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
            pos_um: 0.0,
            step_um: DEFAULT_STEP_UM,
            speed_high: 2000,
            speed_low: 500,
            accel_time: 100,
            controller_axis: "A".to_string(),
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
                Ok(())
            }
            "Low Speed: pps" => {
                self.speed_low = self.set_i64_property(name, val)?;
                Ok(())
            }
            "Accelerating Time: msec" => {
                self.accel_time = self.set_i64_property(name, val)?;
                Ok(())
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
        false
    }
}

impl Stage for ChuoSeikiQTZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = (z / self.step_um) as i64;
        let r = self.cmd(&format!("AGO:{}{}", self.controller_axis, steps))?;
        Self::check_response(&r)?;
        self.pos_um = z;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        Ok(self.pos_um)
    }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        let steps = (dz / self.step_um) as i64;
        let r = self.cmd(&format!("MGO:{}{}", self.controller_axis, steps))?;
        Self::check_response(&r)?;
        self.pos_um += dz;
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
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(make_transport()));
        s.initialize().unwrap();
        assert_eq!(s.get_position_um().unwrap(), 0.0);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any("OK");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(100.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 100.0);
    }

    #[test]
    fn move_relative() {
        let t = make_transport().any("OK");
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
        let t = make_transport().expect("AGO:B10\r\n", "OK");
        let mut s = ChuoSeikiQTZStage::new().with_transport(Box::new(t));
        s.set_property("ChuoSeikiAxisName", PropertyValue::String("B".to_string()))
            .unwrap();
        s.initialize().unwrap();
        s.set_position_um(10.0).unwrap();
    }

    #[test]
    fn unsupported_stage_methods_match_upstream() {
        let mut s = ChuoSeikiQTZStage::new();
        assert_eq!(s.home().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(s.stop().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(s.get_limits().unwrap_err(), MmError::UnsupportedCommand);
    }
}
