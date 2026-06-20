/// ASI Tiger Controller — Z stage.
///
/// Protocol (TX `\r`, RX `\r\n`):
///   `UM Z?\r`     → `:A Z=<u>\r\n` units per mm; converted to units/µm
///   `VB Z=1\r`    → `:A \r\n`      report stage position with 0.1 unit precision
///   `M Z=<z>\r`   → `:A \r\n`      absolute move (controller units)
///   `R Z=<dz>\r`  → `:A \r\n`      relative move
///   `W Z\r`       → `:A Z=<z>\r\n` query Z
///   `! Z\r`       → `:A \r\n`      home Z
///   `halt\r`      → `:A \r\n`      stop card
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::RefCell;

const DEFAULT_UNITS_PER_UM: f64 = 10.0;

pub struct AsiTigerZStage {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    unit_mult: f64,
    position_um: f64,
}

impl AsiTigerZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_property(
                "FirmwareVersion",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        props
            .define_property("FirmwareBuild", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("HexAddress", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("AxisDirection", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .set_allowed_values("AxisDirection", &["1", "-1"])
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            unit_mult: DEFAULT_UNITS_PER_UM,
            position_um: 0.0,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = RefCell::new(Some(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.borrow_mut().as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        self.call_transport(|t| {
            t.purge()?;
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    fn cmd_ok(&self, command: &str) -> MmResult<String> {
        let resp = self.cmd(command)?;
        if resp.starts_with(":N") {
            Err(MmError::LocallyDefined(format!(
                "ASI Tiger error: {}",
                resp
            )))
        } else if !resp.starts_with(":A") {
            Err(MmError::SerialInvalidResponse)
        } else {
            Ok(resp)
        }
    }

    fn parse_after_equals(resp: &str) -> MmResult<f64> {
        resp.split_once('=')
            .and_then(|(_, value)| value.split_whitespace().next())
            .ok_or_else(|| {
                MmError::LocallyDefined(format!("ASI Tiger response lacks value: {}", resp))
            })?
            .parse()
            .map_err(|_| MmError::LocallyDefined(format!("ASI Tiger non-numeric value: {}", resp)))
    }

    fn parse_position(resp: &str) -> MmResult<f64> {
        if let Ok(value) = Self::parse_after_equals(resp) {
            return Ok(value);
        }
        resp.get(2..)
            .and_then(|value| value.trim().parse::<f64>().ok())
            .ok_or_else(|| {
                MmError::LocallyDefined(format!("ASI Tiger response lacks position: {}", resp))
            })
    }
}

impl Default for AsiTigerZStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for AsiTigerZStage {
    fn name(&self) -> &str {
        "AsiTigerZStage"
    }
    fn description(&self) -> &str {
        "ASI Tiger Z Stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        let unit_resp = self.cmd_ok("UM Z?")?;
        self.unit_mult = Self::parse_after_equals(&unit_resp)? / 1000.0;
        self.cmd_ok("VB Z=1")?;
        let resp = self.cmd_ok("W Z")?;
        self.position_um = Self::parse_position(&resp)? / self.unit_mult;
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
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
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
        self.cmd_ok("RS Z")
            .map(|resp| resp.contains('B'))
            .unwrap_or(false)
    }
}

impl Stage for AsiTigerZStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let cmd = format!("M Z={}", pos * self.unit_mult);
        self.cmd_ok(&cmd)?;
        self.position_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        let resp = self.cmd_ok("W Z")?;
        Ok(Self::parse_position(&resp)? / self.unit_mult)
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let cmd = format!("R Z={}", d * self.unit_mult);
        self.cmd_ok(&cmd)?;
        self.position_um += d;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        self.cmd_ok("! Z")?;
        self.position_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        self.cmd_ok("halt")?;
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        let min = Self::parse_after_equals(&self.cmd_ok("SL Z?")?)? * 1000.0;
        let max = Self::parse_after_equals(&self.cmd_ok("SU Z?")?)? * 1000.0;
        Ok((min, max))
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
        let t = MockTransport::new()
            .expect("UM Z?\r", ":A Z=10000")
            .expect("VB Z=1\r", ":A")
            .expect("W Z\r", ":A Z=0")
            .expect("W Z\r", ":A Z=0");
        let mut s = AsiTigerZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap()).abs() < 0.001);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new()
            .expect("UM Z?\r", ":A Z=10000")
            .expect("VB Z=1\r", ":A")
            .expect("W Z\r", ":A Z=0")
            .expect("M Z=500\r", ":A")
            .expect("W Z\r", ":A Z=500");
        let mut s = AsiTigerZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(50.0).unwrap(); // 50µm → 500 units
        assert!((s.get_position_um().unwrap() - 50.0).abs() < 0.01);
    }

    #[test]
    fn move_relative() {
        let t = MockTransport::new()
            .expect("UM Z?\r", ":A Z=10000")
            .expect("VB Z=1\r", ":A")
            .expect("W Z\r", ":A Z=500")
            .expect("R Z=100\r", ":A")
            .expect("W Z\r", ":A Z=600");
        let mut s = AsiTigerZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(10.0).unwrap();
        assert!((s.get_position_um().unwrap() - 60.0).abs() < 0.01);
    }

    #[test]
    fn no_transport_error() {
        assert!(AsiTigerZStage::new().initialize().is_err());
    }

    #[test]
    fn uses_controller_unit_multiplier() {
        let t = MockTransport::new()
            .expect("UM Z?\r", ":A Z=20000")
            .expect("VB Z=1\r", ":A")
            .expect("W Z\r", ":A Z=400")
            .expect("W Z\r", ":A Z=400")
            .expect("M Z=100\r", ":A");
        let mut s = AsiTigerZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 20.0).abs() < 0.001);
        s.set_position_um(5.0).unwrap();
    }

    #[test]
    fn upstream_home_stop_and_limits_behavior() {
        let t = MockTransport::new()
            .expect("UM Z?\r", ":A Z=10000")
            .expect("VB Z=1\r", ":A")
            .expect("W Z\r", ":A 0")
            .expect("! Z\r", ":A")
            .expect("halt\r", ":A")
            .expect("SL Z?\r", ":A Z=-0.25")
            .expect("SU Z?\r", ":A Z=1.5");
        let mut s = AsiTigerZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        s.stop().unwrap();
        assert_eq!(s.get_limits().unwrap(), (-250.0, 1500.0));
    }
}
