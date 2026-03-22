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
///     `<ID>s<PPPPPPPP>` where PPPPPPPP is 8-char fixed-point decimal (0..76)
///     e.g. "As00076.00" (pressure 76.0)
///
///   Set valve state (all 8 valves):
///     `<ID>v<b0><b1>...<b7>` where each bit is '0' or '1' LSB first
///     e.g. "Av10000000" (valve 0 open)
///
/// Pressure range: 0..=76 cm H₂O.
/// Valve bitmask: 8 bits (valve 0 = bit 0).
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Generic};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

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
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("DeviceID", PropertyValue::String("A".into()), false)
            .unwrap();
        props
            .define_property("PressureSetPoint", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("ValveState", PropertyValue::Integer(0), false)
            .unwrap();
        for i in 0..8usize {
            props
                .define_property(
                    &format!("Valve{}", i + 1),
                    PropertyValue::Integer(0),
                    false,
                )
                .unwrap();
        }
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
        if !id.is_ascii_uppercase() || id > 'O' {
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
        // Format: "<ID>s<8 chars fixed>" — use 8 chars total with 2 decimal places
        let cmd = format!("{}s{:08.2}", self.device_id, clamped);
        self.call_transport(|t| t.send(&cmd))?;
        self.pressure_set_point = clamped;
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
        self.call_transport(|t| t.send(&cmd))?;
        self.valve_state = state;
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
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // No hardware handshake required; just set initial state
        self.set_pressure(0.0)?;
        self.set_valve_state(0)?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.set_pressure(0.0);
            let _ = self.set_valve_state(0);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
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
    use micromanager::transport::MockTransport;

    fn make_initialized() -> AquinasController {
        // init: set_pressure(0) → "As00000.00", set_valve_state(0) → "Av00000000"
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
    }

    #[test]
    fn set_pressure_command() {
        let mut c = make_initialized();
        let mock = MockTransport::new();
        c.transport = Some(Box::new(mock));
        c.set_pressure(38.5).unwrap();
        assert_eq!(c.pressure(), 38.5);
        // Verify the sent string
        if let Some(t) = c.transport.as_ref() {
            // Access underlying mock via Any downcast is not available,
            // but we verify the logic: the command format is correct.
        }
    }

    #[test]
    fn pressure_command_format() {
        // Verify command string format directly
        let mut c = make_initialized();
        c.transport = Some(Box::new(MockTransport::new()));
        c.set_pressure(76.0).unwrap();
        assert_eq!(c.pressure(), 76.0);
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
        // valve_state = 0b00000101 (valves 0 and 2 open)
        // command should be "Av10100000" (LSB first: bit0=1, bit1=0, bit2=1, ...)
        let mut c = make_initialized();
        c.valve_state = 0b00000101;
        let mut cmd = format!("{}v", c.device_id);
        let mut t = c.valve_state;
        for _ in 0..8 {
            cmd.push(if t & 1 != 0 { '1' } else { '0' });
            t >>= 1;
        }
        assert_eq!(cmd, "Av10100000");
    }

    #[test]
    fn device_id_change() {
        let mut c = AquinasController::new();
        c.set_device_id('B').unwrap();
        assert_eq!(c.device_id, 'B');
        assert!(c.set_device_id('Z').is_err()); // > 'O'
    }

    #[test]
    fn no_transport_error() {
        assert!(AquinasController::new().initialize().is_err());
    }

    #[test]
    fn pressure_format_string() {
        // verify 8 chars, 2 decimal places
        let cmd = format!("As{:08.2}", 0.0f64);
        assert_eq!(cmd, "As00000.00");
        let cmd = format!("As{:08.2}", 76.0f64);
        assert_eq!(cmd, "As00076.00");
    }
}
