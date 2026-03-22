/// Thorlabs Elliptec Linear Stage (ELL17/ELL20).
///
/// Protocol (TX/RX `\r`):
///   `<ch>in\r`         → `<ch>IN<id><travel><pulses>`  device info
///   `<ch>gp\r`         → `<ch>PO<8-hex>`               get position in pulses
///   `<ch>ma<8-hex>\r`  → `<ch>MA<8-hex>`               move to absolute position
///   `<ch>pc\r`         → `<ch>PC`                       set as origin (zero)
///
/// Channel: single hex digit ('0'–'F').
/// Position encoding: signed 32-bit big-endian as 8 uppercase hex chars.
/// Conversion: position_um = (pulses * 1000) / pulses_per_mm.
/// The `info` response contains a 4-byte (8-hex) travel range and 4-byte pulses-per-mm.
///
/// Error: response last char 'N' or status code indicates fault.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

pub struct ElliptecStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    channel: char,
    pulses_per_mm: u32,
    position_um: f64,
}

impl ElliptecStage {
    pub fn new(channel: char) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Channel", PropertyValue::String(channel.to_string()), false).unwrap();
        props.define_property("PulsesPerMm", PropertyValue::Integer(0), true).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            channel,
            pulses_per_mm: 10000, // default
            position_um: 0.0,
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
        let full = format!("{}{}\r", self.channel, command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    /// Parse 8-char hex as signed i32 position in pulses.
    fn parse_pos_hex(hex: &str) -> i32 {
        u32::from_str_radix(hex.trim(), 16).unwrap_or(0) as i32
    }

    fn pulses_to_um(&self, pulses: i32) -> f64 {
        if self.pulses_per_mm == 0 { return 0.0; }
        (pulses as f64 * 1000.0) / self.pulses_per_mm as f64
    }

    fn um_to_pulses(&self, um: f64) -> i32 {
        ((um * self.pulses_per_mm as f64) / 1000.0).round() as i32
    }
}

impl Default for ElliptecStage { fn default() -> Self { Self::new('0') } }

impl Device for ElliptecStage {
    fn name(&self) -> &str { "ElliptecStage" }
    fn description(&self) -> &str { "Thorlabs Elliptec Linear Stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Get device info to read pulses-per-mm (last 8 hex chars = pulses field)
        let info = self.cmd("in")?;
        // Response: "<ch>IN<id (4)><travel (8)><pulses (8)>" — skip prefix
        let prefix = format!("{}IN", self.channel);
        let data = info.strip_prefix(&prefix).unwrap_or(&info);
        // data: id(4 chars) + travel(8 chars) + pulses(8 chars)
        if data.len() >= 20 {
            let pulses_hex = &data[12..20];
            self.pulses_per_mm = u32::from_str_radix(pulses_hex, 16).unwrap_or(10000);
        }
        self.props.entry_mut("PulsesPerMm").map(|e| e.value = PropertyValue::Integer(self.pulses_per_mm as i64));
        // Query current position
        let gp = self.cmd("gp")?;
        let prefix = format!("{}PO", self.channel);
        let hex = gp.strip_prefix(&prefix).unwrap_or(&gp);
        let pulses = Self::parse_pos_hex(hex);
        self.position_um = self.pulses_to_um(pulses);
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> { self.props.set(name, val) }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Stage }
    fn busy(&self) -> bool { false }
}

impl Stage for ElliptecStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let pulses = self.um_to_pulses(pos);
        let cmd = format!("ma{:08X}", pulses as u32);
        self.cmd(&cmd)?;
        self.position_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.position_um) }

    fn home(&mut self) -> MmResult<()> {
        self.cmd("ho0")?; // home
        self.position_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        self.cmd("st")?;
        Ok(())
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let new_pos = self.position_um + d;
        self.set_position_um(new_pos)
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        // Max travel: 106mm for ELL17, 205mm for ELL20
        let max = if self.pulses_per_mm > 0 { 106000.0 } else { 0.0 };
        Ok((0.0, max))
    }

    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        // "0in" → "0IN" + id(4) + travel(8) + pulses(8)
        // id="ABCD", travel=0x00000000, pulses=0x00002710 (10000 pulses/mm)
        // Total after "0IN": 4+8+8 = 20 chars = "ABCD0000000000002710"
        MockTransport::new()
            .expect("0in\r", "0INABCD0000000000002710")
            .expect("0gp\r", "0PO00000000")
    }

    #[test]
    fn initialize() {
        let mut s = ElliptecStage::new('0').with_transport(Box::new(make_init_transport()));
        s.initialize().unwrap();
        assert_eq!(s.pulses_per_mm, 10000);
        assert!((s.position_um - 0.0).abs() < 0.001);
    }

    #[test]
    fn set_position() {
        let t = make_init_transport()
            // move to 1000 µm = 1.0 mm = 10000 pulses = 0x00002710
            .expect("0ma00002710\r", "0MA00002710");
        let mut s = ElliptecStage::new('0').with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(1000.0).unwrap();
        assert!((s.get_position_um().unwrap() - 1000.0).abs() < 0.01);
    }

    #[test]
    fn home() {
        let t = make_init_transport()
            .expect("0ho0\r", "0HO");
        let mut s = ElliptecStage::new('0').with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert!((s.get_position_um().unwrap()).abs() < 0.01);
    }

    #[test]
    fn no_transport_error() { assert!(ElliptecStage::new('0').initialize().is_err()); }
}
