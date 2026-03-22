use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

/// Cobolt / HÜBNER Photonics laser controller.
///
/// Implements the `Shutter` trait: open = laser on, closed = laser off.
/// Also exposes power setpoint and readback as properties.
pub struct CoboltLaser {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    power_setpoint_mw: f64,
    serial_number: String,
    firmware_version: String,
    hours: String,
}

impl CoboltLaser {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("PowerSetpoint_mW", PropertyValue::Float(0.0), false).unwrap();
        props.set_property_limits("PowerSetpoint_mW", 0.0, 1000.0).unwrap();
        props.define_property("PowerReadback_mW", PropertyValue::Float(0.0), true).unwrap();
        props.define_property("SerialNumber", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("FirmwareVersion", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("UsageHours", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("KeyStatus", PropertyValue::String("Off".into()), true).unwrap();
        props.define_property("FaultCode", PropertyValue::String("0".into()), true).unwrap();
        props.define_property("Interlock", PropertyValue::String("0".into()), true).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            power_setpoint_mw: 0.0,
            serial_number: String::new(),
            firmware_version: String::new(),
            hours: String::new(),
        }
    }

    /// Inject a transport (serial port or mock).
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

    /// Send a command and return the trimmed response line.
    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let cmd = command.to_string();
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }

    fn refresh_power_readback(&mut self) -> MmResult<f64> {
        let resp = self.cmd("p?")?;
        resp.parse::<f64>().map_err(|_| MmError::SerialInvalidResponse)
    }

    fn send_power_setpoint(&mut self, mw: f64) -> MmResult<()> {
        let cmd = format!("slp {:.4}", mw);
        let resp = self.cmd(&cmd)?;
        if resp != "OK" {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(())
    }
}

impl Default for CoboltLaser {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CoboltLaser {
    fn name(&self) -> &str {
        "Cobolt"
    }

    fn description(&self) -> &str {
        "Cobolt laser controller (HÜBNER Photonics)"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Query identification fields
        let sn = self.cmd("sn?")?;
        self.serial_number = sn.clone();
        self.props.entry_mut("SerialNumber")
            .map(|e| e.value = PropertyValue::String(sn));

        let ver = self.cmd("ver?")?;
        self.firmware_version = ver.clone();
        self.props.entry_mut("FirmwareVersion")
            .map(|e| e.value = PropertyValue::String(ver));

        let hrs = self.cmd("hrs?")?;
        self.hours = hrs.clone();
        self.props.entry_mut("UsageHours")
            .map(|e| e.value = PropertyValue::String(hrs));

        // Query initial state
        let state_resp = self.cmd("l?")?;
        self.is_open = state_resp.trim() == "1";

        // Query power setpoint
        let sp = self.cmd("glp?")?;
        if let Ok(mw) = sp.parse::<f64>() {
            self.power_setpoint_mw = mw;
            self.props.entry_mut("PowerSetpoint_mW")
                .map(|e| e.value = PropertyValue::Float(mw));
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            // Best-effort: turn laser off on shutdown
            let _ = self.cmd("l0");
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "PowerSetpoint_mW" => Ok(PropertyValue::Float(self.power_setpoint_mw)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "PowerSetpoint_mW" => {
                let mw = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.send_power_setpoint(mw)?;
                }
                self.power_setpoint_mw = mw;
                self.props.entry_mut("PowerSetpoint_mW")
                    .map(|e| e.value = PropertyValue::Float(mw));
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
        DeviceType::Shutter
    }

    fn busy(&self) -> bool {
        false
    }
}

impl Shutter for CoboltLaser {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open { "l1" } else { "l0" };
        let resp = self.cmd(cmd)?;
        if resp != "OK" {
            return Err(MmError::SerialInvalidResponse);
        }
        self.is_open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.is_open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        self.set_open(true)?;
        // In a real adapter, we'd wait delta_t ms then close.
        // Here we leave it open and let caller close.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn mock_laser() -> CoboltLaser {
        let transport = MockTransport::new()
            // initialize() queries
            .expect("sn?", "12345")
            .expect("ver?", "1.0.0")
            .expect("hrs?", "42.5")
            .expect("l?", "0")
            .expect("glp?", "50.0");

        CoboltLaser::new().with_transport(Box::new(transport))
    }

    #[test]
    fn initialize_reads_fields() {
        let mut laser = mock_laser();
        laser.initialize().unwrap();
        assert!(!laser.get_open().unwrap());
        assert_eq!(laser.power_setpoint_mw, 50.0);
        assert_eq!(
            laser.get_property("SerialNumber").unwrap(),
            PropertyValue::String("12345".into())
        );
        assert_eq!(
            laser.get_property("UsageHours").unwrap(),
            PropertyValue::String("42.5".into())
        );
    }

    #[test]
    fn open_close_laser() {
        let transport = MockTransport::new()
            .expect("sn?", "X")
            .expect("ver?", "1.0")
            .expect("hrs?", "0")
            .expect("l?", "0")
            .expect("glp?", "0.0")
            // set_open(true)
            .expect("l1", "OK")
            // set_open(false)
            .expect("l0", "OK");

        let mut laser = CoboltLaser::new().with_transport(Box::new(transport));
        laser.initialize().unwrap();
        laser.set_open(true).unwrap();
        assert!(laser.get_open().unwrap());
        laser.set_open(false).unwrap();
        assert!(!laser.get_open().unwrap());
    }

    #[test]
    fn set_power_setpoint() {
        let transport = MockTransport::new()
            .expect("sn?", "X")
            .expect("ver?", "1.0")
            .expect("hrs?", "0")
            .expect("l?", "0")
            .expect("glp?", "0.0")
            // set_property("PowerSetpoint_mW", 100.0)
            .expect("slp 100.0000", "OK");

        let mut laser = CoboltLaser::new().with_transport(Box::new(transport));
        laser.initialize().unwrap();
        laser.set_property("PowerSetpoint_mW", PropertyValue::Float(100.0)).unwrap();
        assert_eq!(laser.power_setpoint_mw, 100.0);
    }

    #[test]
    fn no_transport_returns_not_connected() {
        let mut laser = CoboltLaser::new();
        assert!(laser.initialize().is_err());
    }
}
