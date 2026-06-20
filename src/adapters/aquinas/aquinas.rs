/// Aquinas Microfluidics Controller adapter.
///
/// ASCII serial protocol (no response terminator — the C++ source notes
/// "TODO: read back the answer - ugly since it does not have a terminator").
/// Commands are sent without expecting a response line.
///
/// Device ID is a single letter 'A'–'O' (pre-init property).
///
/// Commands (sent via `SendSerialCommand` with empty terminator ""):
///
///   Set pressure:
///     `<ID>s<pressure>` formatted like C++ `std::fixed << setw(8)` with
///     default precision 6, e.g. "As76.000000" (pressure 76.0)
///
///   Set valve state (all 8 valves):
///     `<ID>v<b0><b1>...<b7>` where each bit is '0' or '1' LSB first
///     e.g. "Av10000000" (valve 0 open)
///
/// Pressure range: 0..=76 cm H₂O.
/// Valve bitmask: 8 bits (valve 0 = bit 0).
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct AquinasController {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    /// Device ID character ('A'–'O')
    device_id: char,
    pressure_set_point: f64,
    valve_state: u8,
}

impl AquinasController {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_pre_init_property("Device ID", PropertyValue::String("A".into()))
            .unwrap();
        props
            .set_allowed_values(
                "Device ID",
                &[
                    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O",
                ],
            )
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            device_id: 'A',
            pressure_set_point: 0.0,
            valve_state: 0,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    /// Set the device ID ('A'–'O').
    pub fn set_device_id(&mut self, id: char) -> MmResult<()> {
        if !('A'..='O').contains(&id) {
            return Err(MmError::InvalidInputParam);
        }
        self.device_id = id;
        Ok(())
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

    /// Set pressure in cm H₂O (0–76).
    pub fn set_pressure(&mut self, pressure: f64) -> MmResult<()> {
        let clamped = pressure.max(0.0).min(76.0);
        // C++ uses std::fixed with default precision 6 and setw(8).
        let cmd = format!("{}s{:08.6}", self.device_id, clamped);
        self.pressure_set_point = clamped;
        self.call_transport(|t| t.send(&cmd))?;
        Ok(())
    }

    /// Set the full valve bitmask (8 bits, bit 0 = valve 1).
    pub fn set_valve_state(&mut self, state: u8) -> MmResult<()> {
        // Format: "<ID>v<b0><b1>...<b7>" LSB first as '0'/'1'
        let mut cmd = format!("{}v", self.device_id);
        let mut t = state;
        for _ in 0..8 {
            cmd.push(if t & 1 != 0 { '1' } else { '0' });
            t >>= 1;
        }
        self.valve_state = state;
        self.call_transport(|t| t.send(&cmd))?;
        Ok(())
    }

    /// Open or close a single valve (0-based index 0..7).
    pub fn set_valve(&mut self, valve: usize, open: bool) -> MmResult<()> {
        if valve >= 8 {
            return Err(MmError::InvalidInputParam);
        }
        if open {
            self.valve_state |= 1 << valve;
        } else {
            self.valve_state &= !(1 << valve);
        }
        let new_state = self.valve_state;
        self.set_valve_state(new_state)
    }

    pub fn pressure(&self) -> f64 {
        self.pressure_set_point
    }

    pub fn valve_state(&self) -> u8 {
        self.valve_state
    }
}

impl Default for AquinasController {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for AquinasController {
    fn name(&self) -> &str {
        "Aquinas Controller"
    }
    fn description(&self) -> &str {
        "Aquinas MicroFluidics Controller"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.props.has_property("Pressure Set Point") {
            self.props
                .define_property("Pressure Set Point", PropertyValue::Float(0.0), false)
                .unwrap();
        }
        self.props
            .set_property_limits("Pressure Set Point", 0.0, 76.0)?;
        if !self.props.has_property("Valve State") {
            self.props
                .define_property("Valve State", PropertyValue::Integer(0), false)
                .unwrap();
        }
        self.props.set_property_limits("Valve State", 0.0, 255.0)?;
        for i in 0..8usize {
            let name = format!("Valve nr. {}", i + 1);
            if !self.props.has_property(&name) {
                self.props
                    .define_property(&name, PropertyValue::Integer(0), false)
                    .unwrap();
            }
            self.props.set_property_limits(&name, 0.0, 1.0)?;
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
            "Device ID" => Ok(PropertyValue::String(self.device_id.to_string())),
            "Pressure Set Point" => Ok(PropertyValue::Float(self.pressure_set_point)),
            "Valve State" => Ok(PropertyValue::Integer(self.valve_state as i64)),
            name if name.starts_with("Valve nr. ") && self.props.has_property(name) => {
                let suffix = name
                    .strip_prefix("Valve nr. ")
                    .ok_or(MmError::InvalidPropertyValue)?;
                let valve = suffix
                    .parse::<usize>()
                    .map_err(|_| MmError::InvalidPropertyValue)?;
                if !(1..=8).contains(&valve) {
                    return Err(MmError::InvalidPropertyValue);
                }
                let open = (self.valve_state >> (valve - 1)) & 1;
                Ok(PropertyValue::Integer(open as i64))
            }
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "Device ID" => {
                let id = val.as_str();
                let mut chars = id.chars();
                let ch = chars.next().ok_or(MmError::InvalidPropertyValue)?;
                if chars.next().is_some() {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.set_device_id(ch)?;
                self.props.set(name, PropertyValue::String(id.to_string()))
            }
            "Pressure Set Point" => {
                let pressure = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0.0..=76.0).contains(&pressure) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.set_pressure(pressure)?;
                self.props
                    .set(name, PropertyValue::Float(self.pressure_set_point))
            }
            "Valve State" => {
                let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0..=255).contains(&state) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.set_valve_state(state as u8)?;
                self.props
                    .set(name, PropertyValue::Integer(self.valve_state as i64))
            }
            name if name.starts_with("Valve nr. ") => {
                let suffix = name
                    .strip_prefix("Valve nr. ")
                    .ok_or(MmError::InvalidPropertyValue)?;
                let valve = suffix
                    .parse::<usize>()
                    .map_err(|_| MmError::InvalidPropertyValue)?;
                if !(1..=8).contains(&valve) {
                    return Err(MmError::InvalidPropertyValue);
                }
                let open = match val.as_i64().ok_or(MmError::InvalidPropertyValue)? {
                    0 => false,
                    1 => true,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.set_valve(valve - 1, open)?;
                self.props.set(name, PropertyValue::Integer(open as i64))
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

impl Generic for AquinasController {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;
    use std::sync::{Arc, Mutex};

    struct RecordingTransport {
        sent: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingTransport {
        fn new(sent: Arc<Mutex<Vec<String>>>) -> Self {
            Self { sent }
        }
    }

    impl Transport for RecordingTransport {
        fn send(&mut self, cmd: &str) -> MmResult<()> {
            self.sent.lock().unwrap().push(cmd.to_string());
            Ok(())
        }

        fn receive_line(&mut self) -> MmResult<String> {
            Err(MmError::SerialTimeout)
        }

        fn purge(&mut self) -> MmResult<()> {
            Ok(())
        }
    }

    struct FailingTransport;

    impl Transport for FailingTransport {
        fn send(&mut self, _cmd: &str) -> MmResult<()> {
            Err(MmError::SerialCommandFailed)
        }

        fn receive_line(&mut self) -> MmResult<String> {
            Err(MmError::SerialTimeout)
        }

        fn purge(&mut self) -> MmResult<()> {
            Ok(())
        }
    }

    fn make_initialized() -> AquinasController {
        let t = MockTransport::new();
        let mut c = AquinasController::new().with_transport(Box::new(t));
        c.initialize().unwrap();
        c
    }

    #[test]
    fn initialize_succeeds() {
        let c = make_initialized();
        assert!(c.initialized);
        assert_eq!(c.pressure(), 0.0);
        assert_eq!(c.valve_state(), 0);
        assert!(c.has_property("Device ID"));
        assert!(c.has_property("Pressure Set Point"));
        assert!(c.has_property("Valve State"));
        assert!(c.has_property("Valve nr. 1"));
        assert!(c.has_property("Valve nr. 8"));
        assert!(c.props.entry("Port").unwrap().pre_init);
        assert!(c.props.entry("Device ID").unwrap().pre_init);
        assert!(!c.props.entry("Pressure Set Point").unwrap().pre_init);
        assert!(!c.has_property("DeviceID"));
        assert!(!c.has_property("PressureSetPoint"));
        assert!(!c.has_property("ValveState"));
        assert!(!c.has_property("Valve1"));
    }

    #[test]
    fn set_pressure_command() {
        let mut c = make_initialized();
        let mock = MockTransport::new();
        c.transport = Some(Box::new(mock));
        c.set_pressure(38.5).unwrap();
        assert_eq!(c.pressure(), 38.5);
    }

    #[test]
    fn pressure_command_format() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let mut c = make_initialized();
        c.transport = Some(Box::new(RecordingTransport::new(sent.clone())));
        c.set_pressure(76.0).unwrap();
        assert_eq!(c.pressure(), 76.0);
        assert_eq!(sent.lock().unwrap().as_slice(), &["As76.000000"]);
    }

    #[test]
    fn pressure_clamped() {
        let mut c = make_initialized();
        c.transport = Some(Box::new(MockTransport::new()));
        c.set_pressure(100.0).unwrap();
        assert_eq!(c.pressure(), 76.0);

        c.transport = Some(Box::new(MockTransport::new()));
        c.set_pressure(-10.0).unwrap();
        assert_eq!(c.pressure(), 0.0);
    }

    #[test]
    fn set_valve_single() {
        let mut c = make_initialized();
        c.transport = Some(Box::new(MockTransport::new()));
        c.set_valve(0, true).unwrap();
        assert_eq!(c.valve_state(), 0b00000001);

        c.transport = Some(Box::new(MockTransport::new()));
        c.set_valve(7, true).unwrap();
        assert_eq!(c.valve_state(), 0b10000001);

        c.transport = Some(Box::new(MockTransport::new()));
        c.set_valve(0, false).unwrap();
        assert_eq!(c.valve_state(), 0b10000000);
    }

    #[test]
    fn set_valve_out_of_range() {
        let mut c = make_initialized();
        assert!(c.set_valve(8, true).is_err());
    }

    #[test]
    fn valve_state_bitmask_format() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let mut c = make_initialized();
        c.transport = Some(Box::new(RecordingTransport::new(sent.clone())));
        c.set_valve_state(0b00000101).unwrap();
        assert_eq!(sent.lock().unwrap().as_slice(), &["Av10100000"]);
    }

    #[test]
    fn cached_state_updates_before_send_like_upstream() {
        let mut c = make_initialized();
        c.transport = Some(Box::new(FailingTransport));
        assert_eq!(
            c.set_pressure(12.5).unwrap_err(),
            MmError::SerialCommandFailed
        );
        assert_eq!(c.pressure(), 12.5);

        c.transport = Some(Box::new(FailingTransport));
        assert_eq!(
            c.set_valve_state(0b00000101).unwrap_err(),
            MmError::SerialCommandFailed
        );
        assert_eq!(c.valve_state(), 0b00000101);
    }

    #[test]
    fn device_id_change() {
        let mut c = AquinasController::new();
        c.set_device_id('B').unwrap();
        assert_eq!(c.device_id, 'B');
        c.set_device_id('O').unwrap();
        assert_eq!(c.device_id, 'O');
        assert!(c.set_device_id('Z').is_err());
    }

    #[test]
    fn initialize_does_not_require_transport_like_upstream() {
        let mut c = AquinasController::new();
        c.initialize().unwrap();
        assert!(c.initialized);
        c.shutdown().unwrap();
        assert!(!c.initialized);
    }

    #[test]
    fn property_setters_use_upstream_command_paths() {
        let mut c = make_initialized();
        c.transport = Some(Box::new(MockTransport::new()));

        c.set_property("Pressure Set Point", PropertyValue::Float(12.5))
            .unwrap();
        assert_eq!(c.pressure(), 12.5);

        c.transport = Some(Box::new(MockTransport::new()));
        c.set_property("Valve State", PropertyValue::Integer(5))
            .unwrap();
        assert_eq!(c.valve_state(), 5);

        c.transport = Some(Box::new(MockTransport::new()));
        c.set_property("Valve nr. 2", PropertyValue::Integer(1))
            .unwrap();
        assert_eq!(c.valve_state() & 0b10, 0b10);
    }

    #[test]
    fn property_getters_reflect_upstream_before_get_state() {
        let mut c = make_initialized();
        c.transport = Some(Box::new(MockTransport::new()));
        c.set_property("Valve State", PropertyValue::Integer(5))
            .unwrap();

        assert_eq!(
            c.get_property("Valve State").unwrap(),
            PropertyValue::Integer(5)
        );
        assert_eq!(
            c.get_property("Valve nr. 1").unwrap(),
            PropertyValue::Integer(1)
        );
        assert_eq!(
            c.get_property("Valve nr. 2").unwrap(),
            PropertyValue::Integer(0)
        );
        assert_eq!(
            c.get_property("Valve nr. 3").unwrap(),
            PropertyValue::Integer(1)
        );
    }

    #[test]
    fn property_limits_follow_upstream() {
        let mut c = make_initialized();
        c.transport = Some(Box::new(MockTransport::new()));

        assert!(c
            .set_property("Pressure Set Point", PropertyValue::Float(76.1))
            .is_err());
        assert!(c
            .set_property("Valve State", PropertyValue::Integer(256))
            .is_err());
        assert!(c
            .set_property("Device ID", PropertyValue::String("Z".into()))
            .is_err());
    }
}
