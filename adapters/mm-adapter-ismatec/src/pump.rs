/// Ismatec MCP peristaltic pump controller.
///
/// Protocol (TX `\r`, RX `\r\n`):
///   Address (1–8) is prepended to every command.
///   Single-char commands return `*` on success; string commands return a line.
///
///   `<addr>(\r`           → firmware version string
///   `<addr>-\r`           → `*`  reset overload
///   `<addr>L\r`           → `*`  set mode: continuous RPM
///   `<addr>M\r`           → `*`  set mode: continuous flow rate
///   `<addr>J\r`           → `*`  set direction: clockwise
///   `<addr>K\r`           → `*`  set direction: counter-clockwise
///   `<addr>S<5-dig>\r`    → `*`  set speed RPM × 10 (e.g. `S00600` = 60.0 RPM)
///   `<addr>H\r`           → `*`  start pump
///   `<addr>I\r`           → `*`  stop pump
///   `<addr>E\r`           → string: pump running status
///   `<addr>+\r`           → string: tubing inner diameter (mm)
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::Device;
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

pub struct IsmatecPump {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    address: u8,
    speed_rpm: f64,
    clockwise: bool,
    running: bool,
}

impl IsmatecPump {
    pub fn new(address: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Address", PropertyValue::Integer(address as i64), false).unwrap();
        props.define_property("FirmwareVersion", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("Speed_RPM", PropertyValue::Float(0.0), false).unwrap();
        props.define_property("Direction", PropertyValue::String("Clockwise".into()), false).unwrap();
        props.set_allowed_values("Direction", &["Clockwise", "CounterClockwise"]).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            address,
            speed_rpm: 0.0,
            clockwise: true,
            running: false,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where F: FnOnce(&mut dyn Transport) -> MmResult<R> {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let c = format!("{}{}\r", self.address, command);
        self.call_transport(|t| { let r = t.send_recv(&c)?; Ok(r.trim().to_string()) })
    }

    fn cmd_ack(&mut self, command: &str) -> MmResult<()> {
        let resp = self.cmd(command)?;
        if resp == "*" {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("MCP NAK: {}", resp)))
        }
    }

    /// Format speed for `S` command: 5 digits with 1 implied decimal (RPM × 10).
    fn format_speed(rpm: f64) -> String {
        let val = (rpm * 10.0).round() as u32;
        format!("S{:05}", val)
    }
}

impl Default for IsmatecPump { fn default() -> Self { Self::new(1) } }

impl Device for IsmatecPump {
    fn name(&self) -> &str { "IsmatecPump" }
    fn description(&self) -> &str { "Ismatec MCP peristaltic pump" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Reset overload
        let _ = self.cmd_ack("-");
        // Get firmware version
        let ver = self.cmd("(")?;
        self.props.entry_mut("FirmwareVersion").map(|e| e.value = PropertyValue::String(ver));
        // Set continuous RPM mode
        self.cmd_ack("L")?;
        // Set direction
        self.cmd_ack(if self.clockwise { "J" } else { "K" })?;
        // Set initial speed (0)
        self.cmd_ack(&Self::format_speed(self.speed_rpm))?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.cmd_ack("I"); // stop
            self.running = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Speed_RPM"  => Ok(PropertyValue::Float(self.speed_rpm)),
            "Direction"  => Ok(PropertyValue::String(
                if self.clockwise { "Clockwise" } else { "CounterClockwise" }.into())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Speed_RPM" => {
                let rpm = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized { self.cmd_ack(&Self::format_speed(rpm))?; }
                self.speed_rpm = rpm;
                self.props.entry_mut("Speed_RPM").map(|e| e.value = PropertyValue::Float(rpm));
                Ok(())
            }
            "Direction" => {
                let s = val.as_str().to_string();
                self.clockwise = s == "Clockwise";
                if self.initialized {
                    self.cmd_ack(if self.clockwise { "J" } else { "K" })?;
                }
                self.props.entry_mut("Direction").map(|e| e.value = PropertyValue::String(s));
                Ok(())
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Generic }
    fn busy(&self) -> bool { false }
}

/// Expose start/stop via set_property("Running", ...) for scripting convenience.
impl IsmatecPump {
    pub fn start(&mut self) -> MmResult<()> {
        self.cmd_ack("H")?;
        self.running = true;
        Ok(())
    }

    pub fn stop(&mut self) -> MmResult<()> {
        self.cmd_ack("I")?;
        self.running = false;
        Ok(())
    }

    pub fn is_running(&self) -> bool { self.running }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .any("*")                    // -  reset overload
            .any("MCP Standard v1.4")    // (  firmware
            .any("*")                    // L  continuous RPM mode
            .any("*")                    // J  clockwise
            .any("*")                    // S00000  set speed 0
    }

    #[test]
    fn initialize() {
        let mut p = IsmatecPump::new(1).with_transport(Box::new(make_init_transport()));
        p.initialize().unwrap();
        assert!(!p.is_running());
    }

    #[test]
    fn start_stop() {
        let t = make_init_transport().any("*").any("*");
        let mut p = IsmatecPump::new(1).with_transport(Box::new(t));
        p.initialize().unwrap();
        p.start().unwrap();
        assert!(p.is_running());
        p.stop().unwrap();
        assert!(!p.is_running());
    }

    #[test]
    fn set_speed() {
        let t = make_init_transport().any("*");
        let mut p = IsmatecPump::new(1).with_transport(Box::new(t));
        p.initialize().unwrap();
        p.set_property("Speed_RPM", PropertyValue::Float(60.0)).unwrap();
        assert_eq!(p.speed_rpm, 60.0);
    }

    #[test]
    fn set_ccw() {
        let t = make_init_transport().any("*");
        let mut p = IsmatecPump::new(1).with_transport(Box::new(t));
        p.initialize().unwrap();
        p.set_property("Direction", PropertyValue::String("CounterClockwise".into())).unwrap();
        assert!(!p.clockwise);
    }

    #[test]
    fn format_speed() {
        assert_eq!(IsmatecPump::format_speed(60.0),  "S00600");
        assert_eq!(IsmatecPump::format_speed(0.0),   "S00000");
        assert_eq!(IsmatecPump::format_speed(100.0), "S01000");
        assert_eq!(IsmatecPump::format_speed(6.5),   "S00065");
    }

    #[test]
    fn nak_response_fails() {
        let t = make_init_transport().any("?"); // not *
        let mut p = IsmatecPump::new(1).with_transport(Box::new(t));
        p.initialize().unwrap();
        assert!(p.start().is_err());
    }

    #[test]
    fn no_transport_error() { assert!(IsmatecPump::new(1).initialize().is_err()); }
}
