/// Thorlabs Elliptec ELL14 rotation mount adapter.
///
/// ASCII serial protocol (CR terminated).  Each command is prefixed with the
/// device channel (hex char '0'–'F').
///
/// Commands:
///   `{ch}in`           → get device info (response `{ch}IN{module}{serial}{year}{firmware}`)
///   `{ch}gp`           → get position (response `{ch}PO{hex8}`)
///   `{ch}ma{hex8}`     → move to absolute position (pulse count)
///   `{ch}mr{hex8}`     → move relative (signed pulse count)
///   `{ch}ho{dir}`      → home (dir: '0'=CW, '1'=CCW)
///   `{ch}fw`           → jog forward (CW)
///   `{ch}bw`           → jog backward (CCW)
///   `{ch}gs`           → get status (response `{ch}GS{hex2}` where 00=OK)
///   `{ch}go`           → get home offset (response `{ch}HO{hex8}`)
///   `{ch}so{hex8}`     → set home offset
///   `{ch}gj`           → get jog step (response `{ch}GJ{hex8}`)
///   `{ch}sj{hex8}`     → set jog step
///
/// Position encoding: 32-bit signed hex (8 uppercase chars).
/// Conversion: degrees = pulses * 360 / pulsesPerRev.
/// ELL14 pulsesPerRev is read from device info during `initialize`.
///
/// This adapter implements `Stage` (treating the rotation angle in degrees as
/// the stage position).  For mm-device's `Stage`, position is in µm — we store
/// degrees and satisfy the trait; limits are [0, 360).
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

pub struct ThorlabsEll14 {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    /// Channel address character ('0'–'F')
    channel: char,
    /// Pulses per full revolution (from device info)
    pulses_per_rev: f64,
    /// Current position in degrees [0, 360)
    position_deg: f64,
}

impl ThorlabsEll14 {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "Channel",
                PropertyValue::String("0".into()),
                false,
            )
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            channel: '0',
            pulses_per_rev: 143360.0, // ELL14 default
            position_deg: 0.0,
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
        let cmd = command.to_string();
        self.call_transport(|t| Ok(t.send_recv(&cmd)?.trim().to_string()))
    }

    /// Build a command prefixed with the channel char.
    fn channel_cmd(&self, suffix: &str) -> String {
        format!("{}{}", self.channel, suffix)
    }

    /// Convert pulse count (hex string) → degrees.
    fn pulses_to_deg(&self, hex: &str) -> MmResult<f64> {
        let n = i32::from_str_radix(hex, 16)
            .map_err(|_| MmError::LocallyDefined(format!("Bad hex pos: {}", hex)))?;
        Ok(modulo360(n as f64 * 360.0 / self.pulses_per_rev))
    }

    /// Convert degrees → 8-char uppercase hex pulse count.
    fn deg_to_pulses_hex(&self, deg: f64) -> String {
        let pulses = (deg / 360.0 * self.pulses_per_rev) as i32;
        format!("{:08X}", pulses as u32)
    }

    /// Parse a position reply `{ch}PO{hex8}` and return degrees.
    fn parse_position_reply(&self, resp: &str) -> MmResult<f64> {
        // Strip leading newline if present
        let msg = resp.trim_start_matches('\n');
        // Check for status reply {ch}GS{code}
        if msg.len() >= 3 && &msg[1..3] == "GS" {
            let code = &msg[3..];
            return Err(MmError::LocallyDefined(format!(
                "ELL14 status error: {}",
                code
            )));
        }
        if msg.len() < 11 || &msg[1..3] != "PO" {
            return Err(MmError::SerialInvalidResponse);
        }
        self.pulses_to_deg(&msg[3..11])
    }

    /// Query current position from the device.
    fn query_position(&mut self) -> MmResult<f64> {
        let cmd = self.channel_cmd("gp");
        let resp = self.cmd(&cmd)?;
        self.parse_position_reply(&resp)
    }

    /// Query device info and extract pulsesPerRev.
    fn query_info(&mut self) -> MmResult<f64> {
        let cmd = self.channel_cmd("in");
        let resp = self.cmd(&cmd)?;
        let msg = resp.trim_start_matches('\n');
        if msg.len() < 3 || &msg[1..3] != "IN" {
            return Err(MmError::SerialInvalidResponse);
        }
        // pulsesPerRev is at offset 25, 8 chars (see ELL14.cpp)
        if msg.len() < 33 {
            return Err(MmError::SerialInvalidResponse);
        }
        let ppr_str = &msg[25..33];
        let ppr = i32::from_str_radix(ppr_str, 16)
            .map_err(|_| MmError::LocallyDefined(format!("Bad ppr hex: {}", ppr_str)))?;
        Ok(ppr as f64)
    }
}

fn modulo360(angle: f64) -> f64 {
    let r = angle % 360.0;
    if r < 0.0 { r + 360.0 } else { r }
}

impl Default for ThorlabsEll14 {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ThorlabsEll14 {
    fn name(&self) -> &str {
        "ThorlabsEll14"
    }

    fn description(&self) -> &str {
        "Thorlabs ELL14 rotation mount"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Set channel from property
        let channel_char = if let Ok(PropertyValue::String(s)) = self.props.get("Channel") {
            s.chars().next().unwrap_or('0')
        } else {
            '0'
        };
        self.channel = channel_char;

        // Get device info to read pulsesPerRev
        self.pulses_per_rev = self.query_info()?;
        // Read current position
        self.position_deg = self.query_position()?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Position" => Ok(PropertyValue::Float(self.position_deg)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Position" => {
                let deg = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_position_um(deg)
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

impl Stage for ThorlabsEll14 {
    /// `pos` is treated as degrees (mapped to [0, 360)).
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let deg = modulo360(pos);
        let hex = self.deg_to_pulses_hex(deg);
        let cmd = self.channel_cmd(&format!("ma{}", hex));
        let resp = self.cmd(&cmd)?;
        self.position_deg = self.parse_position_reply(&resp)?;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        Ok(self.position_deg)
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let hex = self.deg_to_pulses_hex(d);
        let cmd = self.channel_cmd(&format!("mr{}", hex));
        let resp = self.cmd(&cmd)?;
        self.position_deg = self.parse_position_reply(&resp)?;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        // Home clockwise (direction '0')
        let cmd = self.channel_cmd("ho0");
        let resp = self.cmd(&cmd)?;
        self.position_deg = self.parse_position_reply(&resp)?;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        // ELL14 has no explicit stop command — not supported
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((0.0, 359.99))
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
    use micromanager::transport::MockTransport;

    /// Build a device info response for channel '0', pulsesPerRev=143360 (0x23000).
    /// ELL14 reply format (33 chars total):
    ///   [0]     channel addr
    ///   [1..3]  "IN"
    ///   [3..5]  module type "0E" (ELL14)
    ///   [5..13] serial number (8 chars)
    ///   [13..15] year (2 chars)
    ///   [15..18] firmware (3 chars)
    ///   [18..25] reserved/thread (7 chars)
    ///   [25..33] pulsesPerRev hex (8 chars)  ← used by positionFromHex in C++
    ///
    /// "0" + "IN" + "0E" + "12345678" + "22" + "001" + "0000000" + "00023000"
    ///  1     2      2       8            2      3        7           8   = 33 chars
    fn idn_resp() -> &'static str {
        // Exactly 33 chars: ch(1)+IN(2)+0E(2)+serial(8)+year(2)+fw(3)+reserved(7)+ppr(8)
        // ppr = 0x23000 = 143360 at [25..33]
        "0IN0E1234567822001000000000023000"
    }

    fn po_resp_0() -> &'static str {
        "0PO00000000"
    }

    fn make_initialized() -> ThorlabsEll14 {
        let t = MockTransport::new()
            .expect("0in", idn_resp())
            .expect("0gp", po_resp_0());
        let mut d = ThorlabsEll14::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d
    }

    #[test]
    fn initialize_reads_ppr_and_position() {
        let d = make_initialized();
        assert!(d.initialized);
        assert!((d.pulses_per_rev - 0x23000 as f64).abs() < 1.0);
        assert!((d.position_deg - 0.0).abs() < 0.01);
    }

    #[test]
    fn no_transport_error() {
        assert!(ThorlabsEll14::new().initialize().is_err());
    }

    #[test]
    fn set_position_sends_ma_command() {
        // After init, move to 90°: pulses = 90/360 * 143360 = 35840 = 0x8C00
        let t = MockTransport::new()
            .expect("0in", idn_resp())
            .expect("0gp", po_resp_0())
            .expect("0ma00008C00", "0PO00008C00");
        let mut d = ThorlabsEll14::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position_um(90.0).unwrap();
        let pos = d.get_position_um().unwrap();
        assert!((pos - 90.0).abs() < 0.1, "pos={}", pos);
    }

    #[test]
    fn get_limits_returns_360_range() {
        let d = ThorlabsEll14::new();
        let (lo, hi) = d.get_limits().unwrap();
        assert!((lo - 0.0).abs() < 0.01);
        assert!((hi - 359.99).abs() < 0.01);
    }

    #[test]
    fn modulo360_wraps() {
        assert!((modulo360(370.0) - 10.0).abs() < 0.001);
        assert!((modulo360(-10.0) - 350.0).abs() < 0.001);
    }

    #[test]
    fn home_returns_position() {
        let t = MockTransport::new()
            .expect("0in", idn_resp())
            .expect("0gp", po_resp_0())
            .expect("0ho0", "0PO00000000");
        let mut d = ThorlabsEll14::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.home().unwrap();
        assert!((d.position_deg - 0.0).abs() < 0.01);
    }

    #[test]
    fn device_type_is_stage() {
        assert_eq!(ThorlabsEll14::new().device_type(), DeviceType::Stage);
    }
}
