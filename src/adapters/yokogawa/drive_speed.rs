/// Yokogawa CSU-X1 disk drive speed controller.
///
/// Upstream CSUX `DriveSpeed` is a Generic device using the CSUX hub commands:
///   `MS, ?`          -> `<rpm>\rA`
///   `MS_MAX, ?`      -> `<rpm>\rA`
///   `MS, <rpm>`      -> `A`
///   `MS_RUN`/`MS_STOP` -> `A`
///   `MS_ADJUST, <ms>` -> `A`
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};

pub struct CsuXDriveSpeed {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    current: Cell<i64>,
    min: i64,
    max: i64,
    running: bool,
    auto_adjust_ms: f64,
}

impl CsuXDriveSpeed {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            current: Cell::new(1500),
            min: 0,
            max: 5000,
            running: false,
            auto_adjust_ms: 0.0,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(RefCell::new(t));
        self
    }

    fn ensure_runtime_properties(&mut self) -> MmResult<()> {
        if !self.props.has_property("Name") {
            self.props.define_property(
                "Name",
                PropertyValue::String("CSUX-DriveSpeed".into()),
                true,
            )?;
        }
        if !self.props.has_property("Description") {
            self.props.define_property(
                "Description",
                PropertyValue::String("CSUX Drive Speed".into()),
                true,
            )?;
        }
        if !self.props.has_property("State") {
            self.props
                .define_property("State", PropertyValue::Integer(1500), false)?;
            self.props
                .set_property_limits("State", self.min as f64, self.max as f64)?;
        }
        if !self.props.has_property("Run") {
            self.props
                .define_property("Run", PropertyValue::String("On".into()), false)?;
            self.props.set_allowed_values("Run", &["On", "Off"])?;
        }
        if !self.props.has_property("AutoAdjust") {
            self.props
                .define_property("AutoAdjust", PropertyValue::Float(0.0), false)?;
        }
        Ok(())
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
        let full = format!("{command}\r");
        self.call_transport(|t| {
            t.purge()?;
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    fn check_ack(resp: &str) -> MmResult<()> {
        match resp.trim_end().chars().last() {
            Some('A') => Ok(()),
            Some('N') => Err(MmError::LocallyDefined(format!("CSU-X NAK: {resp}"))),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn parse_integer_ack(resp: &str) -> MmResult<i64> {
        Self::check_ack(resp)?;
        let mut chars = resp.trim_start().chars().peekable();
        let sign = match chars.peek() {
            Some('-') => {
                chars.next();
                -1
            }
            Some('+') => {
                chars.next();
                1
            }
            _ => 1,
        };
        let mut value = 0_i64;
        while let Some(c) = chars.peek() {
            if let Some(digit) = c.to_digit(10) {
                value = value.saturating_mul(10).saturating_add(digit as i64);
                chars.next();
            } else {
                break;
            }
        }
        Ok(value * sign)
    }

    fn query_speed(&self) -> MmResult<i64> {
        let speed = Self::parse_integer_ack(&self.cmd("MS, ?")?)?;
        self.current.set(speed);
        Ok(speed)
    }

    fn query_max_speed(&self) -> MmResult<i64> {
        Self::parse_integer_ack(&self.cmd("MS_MAX, ?")?)
    }

    fn set_speed(&mut self, requested_speed: i64) -> MmResult<()> {
        if self.initialized && requested_speed == self.current.get() {
            return Ok(());
        }
        let speed = requested_speed.clamp(self.min, self.max);
        if self.initialized {
            let resp = self.cmd(&format!("MS, {speed}"))?;
            Self::check_ack(&resp)?;
        }
        self.current.set(speed);
        self.auto_adjust_ms = 0.0;
        self.props.set("State", PropertyValue::Integer(speed))?;
        self.props
            .set("AutoAdjust", PropertyValue::Float(self.auto_adjust_ms))?;
        Ok(())
    }

    fn set_running(&mut self, running: bool) -> MmResult<()> {
        self.running = running;
        if self.initialized {
            let resp = self.cmd(if running { "MS_RUN" } else { "MS_STOP" })?;
            Self::check_ack(&resp)?;
        }
        self.props.set(
            "Run",
            PropertyValue::String(if running { "On" } else { "Off" }.into()),
        )
    }

    fn set_auto_adjust(&mut self, exposure_ms: f64) -> MmResult<()> {
        if self.initialized {
            let resp = self.cmd(&format!("MS_ADJUST, {exposure_ms}"))?;
            Self::check_ack(&resp)?;
        }
        self.auto_adjust_ms = exposure_ms;
        self.props
            .set("AutoAdjust", PropertyValue::Float(exposure_ms))
    }
}

impl Default for CsuXDriveSpeed {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CsuXDriveSpeed {
    fn name(&self) -> &str {
        "CSUX-DriveSpeed"
    }

    fn description(&self) -> &str {
        "DriveSpeed"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.ensure_runtime_properties()?;
        self.query_speed()?;
        if let Ok(max) = self.query_max_speed() {
            self.max = max;
            self.props
                .set_property_limits("State", self.min as f64, self.max as f64)?;
        }
        self.props
            .set("State", PropertyValue::Integer(self.current.get()))?;
        self.running = self.current.get() > 0;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" if self.props.has_property("State") => {
                if self.initialized && self.transport.is_some() {
                    Ok(PropertyValue::Integer(self.query_speed()?))
                } else {
                    Ok(PropertyValue::Integer(self.current.get()))
                }
            }
            "Run" if self.props.has_property("Run") => Ok(PropertyValue::String(
                if self.running { "On" } else { "Off" }.into(),
            )),
            "AutoAdjust" if self.props.has_property("AutoAdjust") => {
                Ok(PropertyValue::Float(self.auto_adjust_ms))
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "State" if self.props.has_property("State") => {
                let speed = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_speed(speed)
            }
            "Run" if self.props.has_property("Run") => match val.as_str() {
                "On" => self.set_running(true),
                "Off" => self.set_running(false),
                _ => Err(MmError::InvalidPropertyValue),
            },
            "AutoAdjust" if self.props.has_property("AutoAdjust") => {
                let exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_auto_adjust(exposure_ms)
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
        DeviceType::Generic
    }

    fn busy(&self) -> bool {
        false
    }
}

impl Generic for CsuXDriveSpeed {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_creates_upstream_properties_and_reads_speed() {
        let t = MockTransport::new()
            .expect("MS, ?\r", "1800\rA")
            .expect("MS_MAX, ?\r", "4000\rA")
            .expect("MS, ?\r", "1800\rA");
        let mut d = CsuXDriveSpeed::new().with_transport(Box::new(t));
        assert!(!d.has_property("Name"));
        d.initialize().unwrap();
        assert_eq!(
            d.get_property("Name").unwrap(),
            PropertyValue::String("CSUX-DriveSpeed".into())
        );
        assert_eq!(
            d.get_property("Description").unwrap(),
            PropertyValue::String("CSUX Drive Speed".into())
        );
        assert!(d.is_property_read_only("Name"));
        assert!(d.is_property_read_only("Description"));
        assert_eq!(
            d.get_property("State").unwrap(),
            PropertyValue::Integer(1800)
        );
        assert_eq!(
            d.get_property("Run").unwrap(),
            PropertyValue::String("On".into())
        );
        assert_eq!(d.device_type(), DeviceType::Generic);
    }

    #[test]
    fn live_speed_read_updates_cache_and_same_speed_set_is_noop() {
        let t = MockTransport::new()
            .expect("MS, ?\r", "1500\rA")
            .expect("MS_MAX, ?\r", "5000\rA")
            .expect("MS, ?\r", "2000\rA");
        let mut d = CsuXDriveSpeed::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(
            d.get_property("State").unwrap(),
            PropertyValue::Integer(2000)
        );
        d.auto_adjust_ms = 12.5;
        d.set_property("State", PropertyValue::Integer(2000))
            .unwrap();
        assert_eq!(
            d.get_property("AutoAdjust").unwrap(),
            PropertyValue::Float(12.5)
        );
    }

    #[test]
    fn setting_state_clamps_and_clears_auto_adjust() {
        let t = MockTransport::new()
            .expect("MS, ?\r", "1500\rA")
            .expect("MS_MAX, ?\r", "2500\rA")
            .expect("MS, 2500\r", "A")
            .expect("MS, ?\r", "2500\rA");
        let mut d = CsuXDriveSpeed::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.auto_adjust_ms = 12.5;
        d.set_property("State", PropertyValue::Integer(3000))
            .unwrap();
        assert_eq!(
            d.get_property("State").unwrap(),
            PropertyValue::Integer(2500)
        );
        assert_eq!(
            d.get_property("AutoAdjust").unwrap(),
            PropertyValue::Float(0.0)
        );
    }

    #[test]
    fn query_speed_uses_upstream_atol_style_numeric_prefix() {
        let t = MockTransport::new()
            .expect("MS, ?\r", "2400rpm\rA")
            .expect("MS_MAX, ?\r", "5000rpm\rA");
        let mut d = CsuXDriveSpeed::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.current.get(), 2400);
        assert_eq!(d.max, 5000);
        assert_eq!(
            CsuXDriveSpeed::parse_integer_ack("not-a-number\rA").unwrap(),
            0
        );
    }

    #[test]
    fn run_and_auto_adjust_send_upstream_commands() {
        let t = MockTransport::new()
            .expect("MS, ?\r", "0\rA")
            .expect("MS_MAX, ?\r", "5000\rA")
            .expect("MS_RUN\r", "A")
            .expect("MS_ADJUST, 33.5\r", "A")
            .expect("MS_STOP\r", "A");
        let mut d = CsuXDriveSpeed::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_property("Run", PropertyValue::String("On".into()))
            .unwrap();
        d.set_property("AutoAdjust", PropertyValue::Float(33.5))
            .unwrap();
        d.set_property("Run", PropertyValue::String("Off".into()))
            .unwrap();
        assert_eq!(
            d.get_property("Run").unwrap(),
            PropertyValue::String("Off".into())
        );
    }

    #[test]
    fn port_is_locked_after_initialize() {
        let t = MockTransport::new()
            .expect("MS, ?\r", "1500\rA")
            .expect("MS_MAX, ?\r", "5000\rA");
        let mut d = CsuXDriveSpeed::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(
            d.set_property("Port", PropertyValue::String("COM2".into())),
            Err(MmError::InvalidPropertyValue)
        );
    }
}
