/// Ludl MAC5000/MAC6000 Z stage.
///
/// Protocol (TX `\r`, RX `\n`):
///   `MOVE Z=<n>\r` → `:A`
///   `WHERE <axis>\r`    → `:A <z>`
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::{Cell, RefCell};

pub struct LudlZStage {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    pos_um: Cell<f64>,
    axis: String,
    step_size_um: f64,
    autofocus: i64,
}

impl LudlZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "LudlSingleAxisName",
                PropertyValue::String("Z".to_string()),
                false,
            )
            .unwrap();
        props
            .define_property("StepSize", PropertyValue::Float(1.0), false)
            .unwrap();
        props
            .define_property("Autofocus", PropertyValue::Integer(5), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            pos_um: Cell::new(0.0),
            axis: "Z".to_string(),
            step_size_um: 1.0,
            autofocus: 5,
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

    fn check_a(resp: &str) -> MmResult<&str> {
        let s = resp.trim();
        if let Some(rest) = s.strip_prefix(":A") {
            Ok(rest.trim())
        } else {
            Err(MmError::LocallyDefined(format!("Ludl error: {}", s)))
        }
    }

    fn query_position_um(&self) -> MmResult<f64> {
        let resp = self.cmd(&format!("WHERE {}", self.axis))?;
        let body = Self::check_a(&resp)?;
        let steps: i64 = body
            .split_whitespace()
            .next()
            .ok_or_else(|| {
                MmError::LocallyDefined(format!("Cannot parse WHERE response: {}", resp))
            })?
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        let pos = steps as f64 * self.step_size_um;
        self.pos_um.set(pos);
        Ok(pos)
    }

    fn poll_axis_busy(&self) -> bool {
        self.call_transport(|t| {
            if t.send(&format!("STATUS {}\r", self.axis)).is_err() {
                return Ok(false);
            }
            Ok(matches!(
                t.receive_line()
                    .ok()
                    .and_then(|resp| resp.trim().as_bytes().first().copied()),
                Some(b'B')
            ))
        })
        .unwrap_or(false)
    }

    fn set_autofocus(&mut self, autofocus: i64) -> MmResult<()> {
        if self.initialized && self.transport.is_some() {
            let resp = self.cmd(&format!("AF Z={}", autofocus))?;
            Self::check_a(&resp)?;
        }
        self.autofocus = autofocus;
        self.props
            .set("Autofocus", PropertyValue::Integer(self.autofocus))
    }
}

impl Default for LudlZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for LudlZStage {
    fn name(&self) -> &str {
        "Stage"
    }
    fn description(&self) -> &str {
        "Ludl stage driver adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
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
            "LudlSingleAxisName" => Ok(PropertyValue::String(self.axis.clone())),
            "StepSize" => Ok(PropertyValue::Float(self.step_size_um)),
            "Autofocus" => Ok(PropertyValue::Integer(self.autofocus)),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "LudlSingleAxisName" => {
                let axis = val.as_str();
                if !matches!(axis, "X" | "Y" | "Z" | "R" | "T" | "F" | "A" | "B" | "C") {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.axis = axis.to_string();
                self.props
                    .set(name, PropertyValue::String(self.axis.clone()))
            }
            "StepSize" => {
                let step = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if step <= 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.step_size_um = step;
                self.props.set(name, PropertyValue::Float(step))
            }
            "Autofocus" => {
                let autofocus = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_autofocus(autofocus)
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
        self.poll_axis_busy()
    }
}

impl Stage for LudlZStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = (z / self.step_size_um + 0.5) as i64;
        let r = self.cmd(&format!("MOVE {}={}", self.axis, steps))?;
        Self::check_a(&r)?;
        self.pos_um.set(z);
        Ok(())
    }
    fn get_position_um(&self) -> MmResult<f64> {
        if self.initialized && self.transport.is_some() {
            self.query_position_um()
        } else {
            Ok(self.pos_um.get())
        }
    }
    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        self.set_position_um(self.pos_um.get() + dz)
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

    #[test]
    fn initialize() {
        let t = MockTransport::new();
        let mut s = LudlZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.name(), "Stage");
        assert_eq!(s.description(), "Ludl stage driver adapter");
        assert_eq!(s.pos_um.get(), 0.0);
        assert!(s.has_property("LudlSingleAxisName"));
        assert!(s.has_property("StepSize"));
        assert!(s.has_property("Autofocus"));
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new()
            .expect("MOVE Z=200\r", ":A")
            .expect("WHERE Z\r", ":A 200");
        let mut s = LudlZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(200.0).unwrap();
        assert_eq!(s.get_position_um().unwrap(), 200.0);
    }

    #[test]
    fn step_size_axis_and_unsupported_ops_match_upstream_surface() {
        let t = MockTransport::new().expect("MOVE R=100\r", ":A");
        let mut s = LudlZStage::new().with_transport(Box::new(t));
        s.set_property("LudlSingleAxisName", PropertyValue::String("R".to_string()))
            .unwrap();
        s.set_property("StepSize", PropertyValue::Float(0.5))
            .unwrap();
        s.initialize().unwrap();
        s.set_position_um(50.0).unwrap();
        assert_eq!(s.get_limits().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(s.home().unwrap_err(), MmError::UnsupportedCommand);
        assert_eq!(s.stop().unwrap_err(), MmError::UnsupportedCommand);
    }

    #[test]
    fn no_transport_error() {
        assert!(LudlZStage::new().initialize().is_err());
    }

    #[test]
    fn get_position_queries_live_where_response_like_upstream() {
        let t = MockTransport::new()
            .expect("WHERE Z\r", ":A 123")
            .expect("WHERE Z\r", ":A 456");
        let mut s = LudlZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert_eq!(s.get_position_um().unwrap(), 123.0);
        assert_eq!(s.get_position_um().unwrap(), 456.0);
    }

    #[test]
    fn busy_polls_selected_axis_like_upstream() {
        let t = MockTransport::new()
            .expect("STATUS Z\r", "B")
            .expect("STATUS Z\r", "N")
            .expect("STATUS Z\r", "?");
        let mut s = LudlZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!(s.busy());
        assert!(!s.busy());
        assert!(!s.busy());
    }

    #[test]
    fn autofocus_write_sends_upstream_command_and_preserves_cache_on_error() {
        let t = MockTransport::new()
            .expect("AF Z=8\r", ":A")
            .expect("AF Z=9\r", ":N 2");
        let mut s = LudlZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_property("Autofocus", PropertyValue::Integer(8))
            .unwrap();
        assert_eq!(
            s.get_property("Autofocus").unwrap(),
            PropertyValue::Integer(8)
        );
        assert!(s
            .set_property("Autofocus", PropertyValue::Integer(9))
            .is_err());
        assert_eq!(
            s.get_property("Autofocus").unwrap(),
            PropertyValue::Integer(8)
        );
    }
}
