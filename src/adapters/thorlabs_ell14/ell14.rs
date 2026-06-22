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
use crate::adapters::elliptec::status;
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::sync::Mutex;

pub struct ThorlabsEll14 {
    props: PropertyMap,
    transport: Option<Mutex<Box<dyn Transport>>>,
    initialized: bool,
    /// Channel address character ('0'–'F')
    channel: char,
    /// Pulses per full revolution (from device info)
    pulses_per_rev: f64,
    /// Current position in degrees [0, 360)
    position_deg: f64,
    offset_deg: f64,
    jog_step_deg: f64,
    relative_move_deg: f64,
    home_dir: &'static str,
    jog_dir: &'static str,
}

impl ThorlabsEll14 {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Channel", PropertyValue::String("0".into()), false)
            .unwrap();
        props
            .set_allowed_values(
                "Channel",
                &[
                    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F",
                ],
            )
            .unwrap();
        props
            .define_property("Name", PropertyValue::String(" ELL14".into()), true)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            channel: '0',
            pulses_per_rev: 143360.0, // ELL14 default
            position_deg: 0.0,
            offset_deg: 0.0,
            jog_step_deg: 90.0,
            relative_move_deg: 90.0,
            home_dir: "Clockwise",
            jog_dir: "Clockwise",
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(Mutex::new(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_ref() {
            Some(t) => {
                let mut guard = t
                    .lock()
                    .map_err(|_| MmError::LocallyDefined("ELL14 transport lock poisoned".into()))?;
                f(guard.as_mut())
            }
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
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
            return Err(Self::status_to_error(&msg[3..]));
        }
        if msg.len() < 11 || &msg[1..3] != "PO" {
            return Err(MmError::SerialInvalidResponse);
        }
        self.pulses_to_deg(&msg[3..11])
    }

    /// Query current position from the device.
    fn query_position(&self) -> MmResult<f64> {
        let cmd = self.channel_cmd("gp");
        let resp = self.cmd(&cmd)?;
        self.parse_position_reply(&resp)
    }

    /// Query device info and extract pulsesPerRev.
    fn query_info(&self) -> MmResult<(String, f64)> {
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
        if &msg[3..5] != "0E" {
            return Err(MmError::LocallyDefined(format!(
                "Wrong ELL14 module code: {}",
                &msg[3..5]
            )));
        }
        let ppr_str = &msg[25..33];
        let ppr = i32::from_str_radix(ppr_str, 16)
            .map_err(|_| MmError::LocallyDefined(format!("Bad ppr hex: {}", ppr_str)))?;
        Ok((msg[3..18].to_string(), ppr as f64))
    }

    fn query_status_busy(&self) -> MmResult<bool> {
        let resp = self.cmd(&self.channel_cmd("gs"))?;
        let msg = resp.trim_start_matches('\n');
        if msg.len() < 5 || &msg[1..3] != "GS" {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(&msg[3..5] != "00")
    }

    fn query_offset(&self) -> MmResult<f64> {
        let resp = self.cmd(&self.channel_cmd("go"))?;
        self.parse_value_reply(&resp, "HO")
    }

    fn query_jog_step(&self) -> MmResult<f64> {
        let resp = self.cmd(&self.channel_cmd("gj"))?;
        self.parse_value_reply(&resp, "GJ")
    }

    fn parse_value_reply(&self, resp: &str, code: &str) -> MmResult<f64> {
        let msg = resp.trim_start_matches('\n');
        if msg.len() >= 3 && &msg[1..3] == "GS" {
            return Err(Self::status_to_error(&msg[3..]));
        }
        if msg.len() < 11 || &msg[1..3] != code {
            return Err(MmError::SerialInvalidResponse);
        }
        self.pulses_to_deg(&msg[3..11])
    }

    fn expect_status_ok(&self, resp: &str) -> MmResult<()> {
        let msg = resp.trim_start_matches('\n');
        if msg.len() < 5 || &msg[1..3] != "GS" {
            return Err(MmError::SerialInvalidResponse);
        }
        match &msg[3..5] {
            "00" => Ok(()),
            code => Err(Self::status_to_error(code)),
        }
    }

    fn status_to_error(code: &str) -> MmError {
        status::status_code_to_error(code, "ELL14")
    }

    fn ensure_runtime_properties(&mut self, id: String) -> MmResult<()> {
        let definitions = [
            ("ID", PropertyValue::String(id), true),
            ("Position", PropertyValue::Float(self.position_deg), false),
            ("Home offset", PropertyValue::Float(self.offset_deg), false),
            ("Home", PropertyValue::String(self.home_dir.into()), false),
            (
                "Relative Move",
                PropertyValue::Float(self.relative_move_deg),
                false,
            ),
            ("Jog Step", PropertyValue::Float(self.jog_step_deg), false),
            ("Jog", PropertyValue::String(self.jog_dir.into()), false),
            (
                "Frequencies Optimization",
                PropertyValue::String("No Action".into()),
                false,
            ),
        ];
        for (name, value, read_only) in definitions {
            if self.props.has_property(name) {
                if let Some(entry) = self.props.entry_mut(name) {
                    entry.value = value;
                }
            } else {
                self.props.define_property(name, value, read_only)?;
            }
        }
        for name in ["Home", "Jog"] {
            self.props
                .set_allowed_values(name, &["Clockwise", "Counterclockwise"])?;
        }
        self.props.set_property_limits("Position", 0.0, 359.99)?;
        self.props.set_property_limits("Home offset", 0.0, 359.99)?;
        self.props
            .set_property_limits("Relative Move", -359.99, 359.99)?;
        self.props.set_property_limits("Jog Step", 0.0, 359.99)?;
        self.props.set_allowed_values(
            "Frequencies Optimization",
            &["No Action", "Launch Research"],
        )?;
        Ok(())
    }
}

fn modulo360(angle: f64) -> f64 {
    let r = angle % 360.0;
    if r < 0.0 {
        r + 360.0
    } else {
        r
    }
}

impl Default for ThorlabsEll14 {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ThorlabsEll14 {
    fn name(&self) -> &str {
        " ELL14"
    }

    fn description(&self) -> &str {
        "Thorlab's ELL14 Rotation Mount"
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

        let (id, ppr) = self.query_info()?;
        self.pulses_per_rev = ppr;
        self.position_deg = self.query_position()?;
        self.offset_deg = self.query_offset()?;
        self.jog_step_deg = self.query_jog_step()?;
        self.ensure_runtime_properties(id)?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Position" => Ok(PropertyValue::Float(self.query_position()?)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Position" => {
                let deg = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0.0..=359.99).contains(&deg) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.set_position_um(deg as i64 as f64)
            }
            "Home offset" => {
                let deg = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0.0..=359.99).contains(&deg) {
                    return Err(MmError::InvalidPropertyValue);
                }
                let resp =
                    self.cmd(&self.channel_cmd(&format!("so{}", self.deg_to_pulses_hex(deg))))?;
                self.expect_status_ok(&resp)?;
                self.offset_deg = deg;
                self.position_deg = self.query_position()?;
                self.props.set(name, PropertyValue::Float(deg))
            }
            "Home" => {
                let dir = val.as_str().to_string();
                let suffix = match dir.as_str() {
                    "Clockwise" => "ho0",
                    "Counterclockwise" => "ho1",
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                let resp = self.cmd(&self.channel_cmd(suffix))?;
                self.position_deg = self.parse_position_reply(&resp)?;
                self.home_dir = if dir == "Clockwise" {
                    "Clockwise"
                } else {
                    "Counterclockwise"
                };
                self.props.set(name, PropertyValue::String(dir))
            }
            "Relative Move" => {
                let deg = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(-359.99..=359.99).contains(&deg) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.set_relative_position_um(deg)?;
                self.relative_move_deg = deg;
                self.props.set(name, PropertyValue::Float(deg))
            }
            "Jog Step" => {
                let deg = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0.0..=359.99).contains(&deg) {
                    return Err(MmError::InvalidPropertyValue);
                }
                let resp =
                    self.cmd(&self.channel_cmd(&format!("sj{}", self.deg_to_pulses_hex(deg))))?;
                self.expect_status_ok(&resp)?;
                self.jog_step_deg = deg;
                self.props.set(name, PropertyValue::Float(deg))
            }
            "Jog" => {
                let dir = val.as_str().to_string();
                let suffix = match dir.as_str() {
                    "Clockwise" => "fw",
                    "Counterclockwise" => "bw",
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                let resp = self.cmd(&self.channel_cmd(suffix))?;
                self.position_deg = self.parse_position_reply(&resp)?;
                self.jog_dir = if dir == "Clockwise" {
                    "Clockwise"
                } else {
                    "Counterclockwise"
                };
                self.props.set(name, PropertyValue::String(dir))
            }
            "Frequencies Optimization" => {
                let action = val.as_str().to_string();
                if action == "No Action" {
                    return self.props.set(name, PropertyValue::String(action));
                }
                if action != "Launch Research" {
                    return Err(MmError::InvalidPropertyValue);
                }
                let resp1 = self.cmd(&self.channel_cmd("s1"))?;
                self.expect_status_ok(&resp1)?;
                let resp2 = self.cmd(&self.channel_cmd("s2"))?;
                self.expect_status_ok(&resp2)?;
                self.props
                    .set(name, PropertyValue::String("No Action".into()))
            }
            "Port" | "Channel" if self.initialized => Err(MmError::InvalidPropertyValue),
            "Channel" => {
                let s = val.as_str().to_string();
                let ch = s.chars().next().ok_or(MmError::InvalidPropertyValue)?;
                if !ch.is_ascii_hexdigit() {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.channel = ch.to_ascii_uppercase();
                self.props
                    .set(name, PropertyValue::String(self.channel.to_string()))
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
        self.query_status_busy().unwrap_or(true)
    }
}

impl Generic for ThorlabsEll14 {}

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
        self.query_position()
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
        Err(MmError::UnsupportedCommand)
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
    use crate::transport::MockTransport;

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
            .expect("0gp", po_resp_0())
            .expect("0go", "0HO00000000")
            .expect("0gj", "0GJ00008C00");
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
        assert!(d.has_property("ID"));
        assert!(d.has_property("Home offset"));
        assert!(d.has_property("Relative Move"));
        assert!(d.has_property("Frequencies Optimization"));
    }

    #[test]
    fn no_transport_error() {
        assert!(ThorlabsEll14::new().initialize().is_err());
    }

    #[test]
    fn initialize_rejects_non_ell14_module_code() {
        let t = MockTransport::new().expect("0in", "0IN0D1234567822001000000000023000");
        let mut d = ThorlabsEll14::new().with_transport(Box::new(t));
        assert!(d.initialize().is_err());
    }

    #[test]
    fn set_position_sends_ma_command() {
        // After init, move to 90°: pulses = 90/360 * 143360 = 35840 = 0x8C00
        let t = MockTransport::new()
            .expect("0in", idn_resp())
            .expect("0gp", po_resp_0())
            .expect("0go", "0HO00000000")
            .expect("0gj", "0GJ00008C00")
            .expect("0ma00008C00", "0PO00008C00")
            .expect("0gp", "0PO00008C00");
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
            .expect("0go", "0HO00000000")
            .expect("0gj", "0GJ00008C00")
            .expect("0ho0", "0PO00000000");
        let mut d = ThorlabsEll14::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.home().unwrap();
        assert!((d.position_deg - 0.0).abs() < 0.01);
    }

    #[test]
    fn device_type_is_stage() {
        assert_eq!(ThorlabsEll14::new().device_type(), DeviceType::Generic);
    }

    #[test]
    fn channel_property_updates_before_init_and_is_locked_after_init() {
        let t = MockTransport::new()
            .expect("Ain", "AIN0E1234567822001000000000023000")
            .expect("Agp", "APO00000000")
            .expect("Ago", "AHO00000000")
            .expect("Agj", "AGJ00008C00");
        let mut d = ThorlabsEll14::new().with_transport(Box::new(t));
        d.set_property("Channel", PropertyValue::String("A".into()))
            .unwrap();
        d.initialize().unwrap();
        assert_eq!(
            d.set_property("Channel", PropertyValue::String("1".into()))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            d.set_property("Port", PropertyValue::String("COM2".into()))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn busy_polls_gs() {
        let t = MockTransport::new()
            .expect("0in", idn_resp())
            .expect("0gp", po_resp_0())
            .expect("0go", "0HO00000000")
            .expect("0gj", "0GJ00008C00")
            .expect("0gs", "0GS09");
        let mut d = ThorlabsEll14::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert!(d.busy());
    }

    #[test]
    fn property_actions_send_upstream_commands() {
        let t = MockTransport::new()
            .expect("0in", idn_resp())
            .expect("0gp", po_resp_0())
            .expect("0go", "0HO00000000")
            .expect("0gj", "0GJ00008C00")
            .expect("0so00004600", "0GS00")
            .expect("0gp", po_resp_0())
            .expect("0sj00004600", "0GS00")
            .expect("0bw", "0PO00000000")
            .expect("0s1", "0GS00")
            .expect("0s2", "0GS00");
        let mut d = ThorlabsEll14::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_property("Home offset", PropertyValue::Float(45.0))
            .unwrap();
        d.set_property("Jog Step", PropertyValue::Float(45.0))
            .unwrap();
        d.set_property("Jog", PropertyValue::String("Counterclockwise".into()))
            .unwrap();
        d.set_property(
            "Frequencies Optimization",
            PropertyValue::String("Launch Research".into()),
        )
        .unwrap();
    }

    #[test]
    fn bounded_properties_reject_out_of_range_values() {
        let t = MockTransport::new()
            .expect("0in", idn_resp())
            .expect("0gp", po_resp_0())
            .expect("0go", "0HO00000000")
            .expect("0gj", "0GJ00008C00");
        let mut d = ThorlabsEll14::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(
            d.set_property("Position", PropertyValue::Float(360.0))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            d.set_property("Home offset", PropertyValue::Float(-1.0))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            d.set_property("Relative Move", PropertyValue::Float(360.0))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
        assert_eq!(
            d.set_property("Jog Step", PropertyValue::Float(360.0))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn maps_extended_status_codes() {
        assert_eq!(ThorlabsEll14::status_to_error("01"), MmError::SerialTimeout);
        assert_eq!(
            ThorlabsEll14::status_to_error("0B"),
            MmError::LocallyDefined("ELL14 motor error".into())
        );
        assert_eq!(
            ThorlabsEll14::status_to_error("0C"),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn position_property_truncates_like_upstream_long_action() {
        let t = MockTransport::new()
            .expect("0in", idn_resp())
            .expect("0gp", po_resp_0())
            .expect("0go", "0HO00000000")
            .expect("0gj", "0GJ00008C00")
            .expect("0ma00004600", "0PO00004600");
        let mut d = ThorlabsEll14::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_property("Position", PropertyValue::Float(45.9))
            .unwrap();
        assert!((d.position_deg - 45.0).abs() < 0.1);
    }
}
