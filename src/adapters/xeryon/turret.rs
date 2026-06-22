/// Squid+ objective turret driven by a Xeryon XLS-1250-3N linear stage.
///
/// The turret has two discrete positions mapped to physical offsets on the
/// linear stage:
///   Position 0 → −19.0 mm
///   Position 1 → +19.0 mm
///
/// Protocol is the same multi-axis TAG=VALUE used by the XY stage, but only
/// the X axis is used.
///
/// NOTE: The Python squid-control reference implementation uses `findIndex`
/// (X:INDX) for homing rather than the approach used in the original C++
/// MicroManager Xeryon adapter.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

const NUM_POSITIONS: u64 = 2;

/// Physical positions in millimetres for each turret slot.
const POS_MM: [f64; 2] = [-19.0, 19.0];

/// Default encoder resolution for XLS-1250-3N (nm per count).
const DEFAULT_ENCODER_RES_NM: f64 = 1250.0;

/// Speed multiplier when sending SSPD.
const SPEED_MULTIPLIER: i64 = 1000;

/// Default movement speed in mm/s.
const DEFAULT_SPEED_MM_S: f64 = 1.0;

pub struct SquidPlusObjectiveTurret {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position: u64,
    labels: Vec<String>,
    encoder_res_nm: f64,
}

impl SquidPlusObjectiveTurret {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "EncoderResolution",
                PropertyValue::String("XLS3=1251".into()),
                false,
            )
            .unwrap();
        props
            .define_property(
                "Speed_mm_per_s",
                PropertyValue::Float(DEFAULT_SPEED_MM_S),
                false,
            )
            .unwrap();

        let labels = vec!["Pos-1".to_string(), "Pos-2".to_string()];
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("Label", PropertyValue::String(labels[0].clone()), false)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            position: 0,
            labels,
            encoder_res_nm: DEFAULT_ENCODER_RES_NM,
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

    fn axis_cmd(&mut self, tag: &str, value: i64) -> MmResult<String> {
        let full = format!("X:{}={}\n", tag, value);
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }

    fn axis_send(&mut self, tag: &str, value: i64) -> MmResult<()> {
        let full = format!("X:{}={}\n", tag, value);
        self.call_transport(|t| t.send(&full))
    }

    fn raw_cmd(&mut self, cmd: &str) -> MmResult<()> {
        let full = format!("{}\n", cmd);
        self.call_transport(|t| t.send(&full))
    }

    fn parse_response(resp: &str) -> Option<i64> {
        resp.split('=').nth(1).and_then(|s| s.trim().parse().ok())
    }

    fn mm_to_counts(&self, mm: f64) -> i64 {
        // mm → nm → counts
        (mm * 1_000_000.0 / self.encoder_res_nm).round() as i64
    }

    fn move_to_mm(&mut self, target_mm: f64) -> MmResult<()> {
        let counts = self.mm_to_counts(target_mm);
        self.axis_cmd("DPOS", counts)?;
        Ok(())
    }

    /// Look up encoder resolution in nm from the resolution command string.
    fn resolution_from_cmd(cmd: &str) -> f64 {
        if let Some(val_str) = cmd.split('=').nth(1) {
            match val_str.trim() {
                "312" => 312.5,
                "1250" | "1251" => 1250.0,
                "78" => 78.125,
                "5" => 5.0,
                "1" => 1.0,
                other => other.parse::<f64>().unwrap_or(DEFAULT_ENCODER_RES_NM),
            }
        } else {
            DEFAULT_ENCODER_RES_NM
        }
    }
}

impl Default for SquidPlusObjectiveTurret {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for SquidPlusObjectiveTurret {
    fn name(&self) -> &str {
        "SquidPlusObjectiveTurret"
    }
    fn description(&self) -> &str {
        "Squid+ objective turret (Xeryon XLS-1250-3N)"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Determine encoder resolution from property
        let res_cmd = self.props.get("EncoderResolution")?.as_str().to_string();
        self.encoder_res_nm = Self::resolution_from_cmd(&res_cmd);

        // Reset axis
        self.axis_send("RSET", 0)?;

        // Send encoder resolution command
        self.raw_cmd(&res_cmd)?;

        // Set speed
        let speed = self
            .props
            .get("Speed_mm_per_s")?
            .as_f64()
            .unwrap_or(DEFAULT_SPEED_MM_S);
        let speed_raw = (speed * SPEED_MULTIPLIER as f64) as i64;
        self.axis_send("SSPD", speed_raw)?;

        // Home via findIndex (X:INDX=0).
        // NOTE: The Python squid-control implementation uses findIndex for homing,
        // which deviates from the C++ MicroManager Xeryon adapter reference.
        self.axis_cmd("INDX", 0)?;

        // Read current encoder position and snap to nearest discrete position
        let resp = self.axis_cmd("EPOS", 0)?;
        let counts = Self::parse_response(&resp).unwrap_or(0);
        let current_mm = counts as f64 * self.encoder_res_nm / 1_000_000.0;

        // Find closest position
        let mut best = 0u64;
        let mut best_dist = f64::MAX;
        for (i, &pos_mm) in POS_MM.iter().enumerate() {
            let dist = (current_mm - pos_mm).abs();
            if dist < best_dist {
                best_dist = dist;
                best = i as u64;
            }
        }
        self.position = best;

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.axis_send("STOP", 0);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::Integer(self.position as i64)),
            "Label" => Ok(PropertyValue::String(
                self.labels[self.position as usize].clone(),
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u64;
                self.set_position(pos)
            }
            "Label" => {
                let label = val.as_str().to_string();
                self.set_position_by_label(&label)
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
        DeviceType::State
    }

    fn busy(&self) -> bool {
        false
    }
}

impl StateDevice for SquidPlusObjectiveTurret {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= NUM_POSITIONS {
            return Err(MmError::UnknownPosition);
        }
        self.move_to_mm(POS_MM[pos as usize])?;
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        Ok(self.position)
    }

    fn get_number_of_positions(&self) -> u64 {
        NUM_POSITIONS
    }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels
            .get(pos as usize)
            .cloned()
            .ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self
            .labels
            .iter()
            .position(|l| l == label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64;
        self.set_position(pos)
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        if pos >= NUM_POSITIONS {
            return Err(MmError::UnknownPosition);
        }
        self.labels[pos as usize] = label.to_string();
        Ok(())
    }

    fn set_gate_open(&mut self, _open: bool) -> MmResult<()> {
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    /// Build a MockTransport that scripts the full initialization sequence.
    fn make_init_transport() -> MockTransport {
        // axis_cmd("INDX", 0) → send_recv
        // axis_cmd("EPOS", 0) → send_recv
        MockTransport::new()
            .expect("X:INDX=0\n", "X:INDX=OK")
            .expect("X:EPOS=0\n", "X:EPOS=0")
    }

    #[test]
    fn initialize() {
        let mut dev =
            SquidPlusObjectiveTurret::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        assert!(dev.initialized);
        // At EPOS=0 (home), closest position is 0 (−19mm is 19mm away, +19mm is 19mm away),
        // but both are equidistant — first one wins, so position 0.
        assert_eq!(dev.get_position().unwrap(), 0);
    }

    #[test]
    fn set_position_0() {
        // -19.0 mm → -19.0 * 1e6 / 1250.0 = -15200 counts
        let t = make_init_transport().expect("X:DPOS=-15200\n", "X:DPOS=-15200");
        let mut dev = SquidPlusObjectiveTurret::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_position(0).unwrap();
        assert_eq!(dev.get_position().unwrap(), 0);
    }

    #[test]
    fn set_position_1() {
        // +19.0 mm → 19.0 * 1e6 / 1250.0 = 15200 counts
        let t = make_init_transport().expect("X:DPOS=15200\n", "X:DPOS=15200");
        let mut dev = SquidPlusObjectiveTurret::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_position(1).unwrap();
        assert_eq!(dev.get_position().unwrap(), 1);
    }

    #[test]
    fn out_of_range() {
        let mut dev =
            SquidPlusObjectiveTurret::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        assert!(dev.set_position(2).is_err());
        assert!(dev.set_position(99).is_err());
    }

    #[test]
    fn labels() {
        let mut dev =
            SquidPlusObjectiveTurret::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();

        assert_eq!(dev.get_position_label(0).unwrap(), "Pos-1");
        assert_eq!(dev.get_position_label(1).unwrap(), "Pos-2");

        dev.set_position_label(0, "20x").unwrap();
        dev.set_position_label(1, "4x").unwrap();
        assert_eq!(dev.get_position_label(0).unwrap(), "20x");

        // Navigate by label
        let t2 = make_init_transport().expect("X:DPOS=15200\n", "X:DPOS=15200");
        let mut dev2 = SquidPlusObjectiveTurret::new().with_transport(Box::new(t2));
        dev2.initialize().unwrap();
        dev2.set_position_label(1, "4x").unwrap();
        dev2.set_position_by_label("4x").unwrap();
        assert_eq!(dev2.get_position().unwrap(), 1);
    }

    #[test]
    fn no_transport_error() {
        assert!(SquidPlusObjectiveTurret::new().initialize().is_err());
    }

    #[test]
    fn encoder_resolution_parsing_accepts_validated_3n_command() {
        assert!((SquidPlusObjectiveTurret::resolution_from_cmd("XLS3=1251") - 1250.0).abs() < 0.01);
    }

    #[test]
    fn state_and_label_are_advertised_properties() {
        let dev = SquidPlusObjectiveTurret::new();
        assert!(dev.has_property("State"));
        assert!(dev.has_property("Label"));
        assert!(dev.property_names().contains(&"State".to_string()));
        assert!(dev.property_names().contains(&"Label".to_string()));
    }
}
