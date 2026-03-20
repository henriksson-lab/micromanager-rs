use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Shutter};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

const RESPONSE_TERMINATOR: &str = "-==-";
const MIN_INTERFACE_VERSION: f64 = 2.30;
const MAX_INTERFACE_VERSION: f64 = 10.0;

/// Illumination pattern selection.
#[derive(Debug, Clone, PartialEq)]
enum Pattern {
    Brightfield,
    Darkfield,
    Dpc,
    ColorDpc,
    ColorDarkfield,
    Annulus,
    HalfAnnulus,
    Center,
    Manual,
}

impl Pattern {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "Brightfield"    => Some(Self::Brightfield),
            "Darkfield"      => Some(Self::Darkfield),
            "DPC"            => Some(Self::Dpc),
            "ColorDPC"       => Some(Self::ColorDpc),
            "ColorDarkfield" => Some(Self::ColorDarkfield),
            "Annulus"        => Some(Self::Annulus),
            "HalfAnnulus"    => Some(Self::HalfAnnulus),
            "Center"         => Some(Self::Center),
            "Manual"         => Some(Self::Manual),
            _                => None,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Brightfield    => "Brightfield",
            Self::Darkfield      => "Darkfield",
            Self::Dpc            => "DPC",
            Self::ColorDpc       => "ColorDPC",
            Self::ColorDarkfield => "ColorDarkfield",
            Self::Annulus        => "Annulus",
            Self::HalfAnnulus    => "HalfAnnulus",
            Self::Center         => "Center",
            Self::Manual         => "Manual",
        }
    }
}

pub struct IlluminateLedArray {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    open: bool,

    // Current settings
    pattern: Pattern,
    brightness: u8,          // 0–255
    numerical_aperture: f64, // e.g. 0.50
    inner_na: f64,           // e.g. 0.25
    array_distance_mm: f64,  // mm
    color_r: u8,
    color_g: u8,
    color_b: u8,
    dpc_direction: String,   // "top"/"bottom"/"left"/"right" or degree string
    annulus_start_na: f64,
    annulus_width_na: f64,
    half_annulus_direction: String, // "t"/"b"/"l"/"r"
    led_indices: String,     // comma-separated, e.g. "0,1,5,10"

    // Device metadata (from pprops)
    led_count: u64,
    trigger_input_count: u64,
    trigger_output_count: u64,
    interface_version: f64,
}

impl IlluminateLedArray {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port",            PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Pattern",         PropertyValue::String("Brightfield".into()), false).unwrap();
        props.set_allowed_values("Pattern", &[
            "Brightfield", "Darkfield", "DPC", "ColorDPC", "ColorDarkfield",
            "Annulus", "HalfAnnulus", "Center", "Manual",
        ]).unwrap();
        props.define_property("Brightness",      PropertyValue::Integer(127), false).unwrap();
        props.define_property("NumericalAperture",      PropertyValue::Float(0.50), false).unwrap();
        props.define_property("InnerNumericalAperture", PropertyValue::Float(0.0), false).unwrap();
        props.define_property("ArrayDistanceMm",        PropertyValue::Float(100.0), false).unwrap();
        props.define_property("ColorR",          PropertyValue::Integer(255), false).unwrap();
        props.define_property("ColorG",          PropertyValue::Integer(255), false).unwrap();
        props.define_property("ColorB",          PropertyValue::Integer(255), false).unwrap();
        props.define_property("DpcDirection",    PropertyValue::String("top".into()), false).unwrap();
        props.set_allowed_values("DpcDirection", &["top", "bottom", "left", "right"]).unwrap();
        props.define_property("AnnulusStartNA",  PropertyValue::Float(0.25), false).unwrap();
        props.define_property("AnnulusWidthNA",  PropertyValue::Float(0.10), false).unwrap();
        props.define_property("HalfAnnulusDir",  PropertyValue::String("t".into()), false).unwrap();
        props.set_allowed_values("HalfAnnulusDir", &["t", "b", "l", "r"]).unwrap();
        props.define_property("LedIndices",      PropertyValue::String("0".into()), false).unwrap();
        // Read-only device info populated after init
        props.define_property("LedCount",             PropertyValue::Integer(0), true).unwrap();
        props.define_property("TriggerInputCount",    PropertyValue::Integer(0), true).unwrap();
        props.define_property("TriggerOutputCount",   PropertyValue::Integer(0), true).unwrap();
        props.define_property("InterfaceVersion",     PropertyValue::Float(0.0), true).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            open: false,
            pattern: Pattern::Brightfield,
            brightness: 127,
            numerical_aperture: 0.50,
            inner_na: 0.0,
            array_distance_mm: 100.0,
            color_r: 255,
            color_g: 255,
            color_b: 255,
            dpc_direction: "top".into(),
            annulus_start_na: 0.25,
            annulus_width_na: 0.10,
            half_annulus_direction: "t".into(),
            led_indices: "0".into(),
            led_count: 0,
            trigger_input_count: 0,
            trigger_output_count: 0,
            interface_version: 0.0,
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
            None    => Err(MmError::NotConnected),
        }
    }

    /// Send command with no response expected.
    fn send_cmd(&mut self, cmd: &str) -> MmResult<()> {
        let full = format!("{}\n", cmd);
        self.call_transport(|t| t.send(&full))
    }

    /// Send command and collect response lines until `-==-`.
    /// Returns the concatenated non-terminator lines (typically one JSON line).
    fn send_recv_terminated(&mut self, cmd: &str) -> MmResult<String> {
        let full = format!("{}\n", cmd);
        self.call_transport(|t| {
            t.send(&full)?;
            let mut content = String::new();
            loop {
                let line = t.receive_line()?;
                let trimmed = line.trim();
                if trimmed == RESPONSE_TERMINATOR { break; }
                if !trimmed.is_empty() {
                    if !content.is_empty() { content.push('\n'); }
                    content.push_str(trimmed);
                }
            }
            Ok(content)
        })
    }

    /// Build the illumination command for the current pattern.
    fn pattern_cmd(&self) -> String {
        match &self.pattern {
            Pattern::Brightfield    => "bf".into(),
            Pattern::Darkfield      => "df".into(),
            Pattern::Dpc            => format!("dpc.{}", dpc_dir_to_angle(&self.dpc_direction)),
            Pattern::ColorDpc       => "cdpc".into(),
            Pattern::ColorDarkfield => "cdf".into(),
            Pattern::Annulus        => format!("an.{}.{}",
                na_to_int(self.annulus_start_na),
                na_to_int(self.annulus_width_na)),
            Pattern::HalfAnnulus    => format!("ha.{}.{}.{}",
                self.half_annulus_direction,
                na_to_int(self.annulus_start_na),
                na_to_int(self.annulus_width_na)),
            Pattern::Center         => "l.0".into(),
            Pattern::Manual         => {
                let indices = self.led_indices.split(',')
                    .map(|s| s.trim())
                    .collect::<Vec<_>>()
                    .join(".");
                format!("l.{}", indices)
            }
        }
    }

    fn apply_brightness(&mut self) -> MmResult<()> {
        self.send_cmd(&format!("sb.{}", self.brightness))
    }

    fn apply_color(&mut self) -> MmResult<()> {
        self.send_cmd(&format!("sc.{}.{}.{}", self.color_r, self.color_g, self.color_b))
    }

    fn apply_na(&mut self) -> MmResult<()> {
        self.send_cmd(&format!("na.{}", na_to_int(self.numerical_aperture)))
    }
}

impl Default for IlluminateLedArray {
    fn default() -> Self { Self::new() }
}

/// Convert NA float (0.0–1.0+) to integer scaled by 100.
fn na_to_int(na: f64) -> i64 {
    (na * 100.0).round() as i64
}

/// Map DPC direction string to angle in degrees.
fn dpc_dir_to_angle(dir: &str) -> i32 {
    match dir {
        "top"    => 0,
        "right"  => 90,
        "bottom" => 180,
        "left"   => 270,
        _        => dir.parse().unwrap_or(0),
    }
}

impl Device for IlluminateLedArray {
    fn name(&self) -> &str { "IlluminateLEDArray" }
    fn description(&self) -> &str { "Illuminate LED Array illumination device" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }

        // Switch to machine-readable JSON mode
        self.send_cmd("machine")?;

        // Query device properties
        let json_str = self.send_recv_terminated("pprops")?;
        let props: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| MmError::LocallyDefined(format!("pprops JSON parse error: {}", e)))?;

        // Validate interface version
        self.interface_version = props["interface_version"]
            .as_f64().unwrap_or(0.0);
        if self.interface_version < MIN_INTERFACE_VERSION
            || self.interface_version > MAX_INTERFACE_VERSION
        {
            return Err(MmError::LocallyDefined(format!(
                "Illuminate firmware interface version {:.2} not in supported range {:.2}–{:.2}",
                self.interface_version, MIN_INTERFACE_VERSION, MAX_INTERFACE_VERSION
            )));
        }

        self.led_count            = props["led_count"].as_u64().unwrap_or(0);
        self.trigger_input_count  = props["trigger_input_count"].as_u64().unwrap_or(0);
        self.trigger_output_count = props["trigger_output_count"].as_u64().unwrap_or(0);

        // Update read-only info properties
        self.props.entry_mut("LedCount")
            .map(|e| e.value = PropertyValue::Integer(self.led_count as i64));
        self.props.entry_mut("TriggerInputCount")
            .map(|e| e.value = PropertyValue::Integer(self.trigger_input_count as i64));
        self.props.entry_mut("TriggerOutputCount")
            .map(|e| e.value = PropertyValue::Integer(self.trigger_output_count as i64));
        self.props.entry_mut("InterfaceVersion")
            .map(|e| e.value = PropertyValue::Float(self.interface_version));

        // Apply initial settings
        self.apply_brightness()?;
        self.apply_color()?;
        self.apply_na()?;

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send_cmd("x"); // clear all LEDs on shutdown
        }
        self.initialized = false;
        self.open = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Pattern"                => Ok(PropertyValue::String(self.pattern.as_str().into())),
            "Brightness"             => Ok(PropertyValue::Integer(self.brightness as i64)),
            "NumericalAperture"      => Ok(PropertyValue::Float(self.numerical_aperture)),
            "InnerNumericalAperture" => Ok(PropertyValue::Float(self.inner_na)),
            "ArrayDistanceMm"        => Ok(PropertyValue::Float(self.array_distance_mm)),
            "ColorR"                 => Ok(PropertyValue::Integer(self.color_r as i64)),
            "ColorG"                 => Ok(PropertyValue::Integer(self.color_g as i64)),
            "ColorB"                 => Ok(PropertyValue::Integer(self.color_b as i64)),
            "DpcDirection"           => Ok(PropertyValue::String(self.dpc_direction.clone())),
            "AnnulusStartNA"         => Ok(PropertyValue::Float(self.annulus_start_na)),
            "AnnulusWidthNA"         => Ok(PropertyValue::Float(self.annulus_width_na)),
            "HalfAnnulusDir"         => Ok(PropertyValue::String(self.half_annulus_direction.clone())),
            "LedIndices"             => Ok(PropertyValue::String(self.led_indices.clone())),
            _                        => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Pattern" => {
                let s = val.as_str().to_string();
                self.pattern = Pattern::from_str(&s).ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::String(s))
            }
            "Brightness" => {
                let b = val.as_i64().ok_or(MmError::InvalidPropertyValue)?.clamp(0, 255) as u8;
                self.brightness = b;
                if self.initialized { self.apply_brightness()?; }
                self.props.set(name, PropertyValue::Integer(b as i64))
            }
            "NumericalAperture" => {
                self.numerical_aperture = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized { self.apply_na()?; }
                self.props.set(name, val)
            }
            "InnerNumericalAperture" => {
                self.inner_na = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.send_cmd(&format!("nai.{}", na_to_int(self.inner_na)))?;
                }
                self.props.set(name, val)
            }
            "ArrayDistanceMm" => {
                self.array_distance_mm = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.send_cmd(&format!("sad.{}", self.array_distance_mm as i64))?;
                }
                self.props.set(name, val)
            }
            "ColorR" => {
                self.color_r = val.as_i64().ok_or(MmError::InvalidPropertyValue)?.clamp(0, 255) as u8;
                if self.initialized { self.apply_color()?; }
                self.props.set(name, PropertyValue::Integer(self.color_r as i64))
            }
            "ColorG" => {
                self.color_g = val.as_i64().ok_or(MmError::InvalidPropertyValue)?.clamp(0, 255) as u8;
                if self.initialized { self.apply_color()?; }
                self.props.set(name, PropertyValue::Integer(self.color_g as i64))
            }
            "ColorB" => {
                self.color_b = val.as_i64().ok_or(MmError::InvalidPropertyValue)?.clamp(0, 255) as u8;
                if self.initialized { self.apply_color()?; }
                self.props.set(name, PropertyValue::Integer(self.color_b as i64))
            }
            "DpcDirection" => {
                self.dpc_direction = val.as_str().to_string();
                self.props.set(name, val)
            }
            "AnnulusStartNA" => {
                self.annulus_start_na = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, val)
            }
            "AnnulusWidthNA" => {
                self.annulus_width_na = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, val)
            }
            "HalfAnnulusDir" => {
                self.half_annulus_direction = val.as_str().to_string();
                self.props.set(name, val)
            }
            "LedIndices" => {
                self.led_indices = val.as_str().to_string();
                self.props.set(name, val)
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Shutter }
    fn busy(&self) -> bool { false }
}

impl Shutter for IlluminateLedArray {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        if open {
            let cmd = self.pattern_cmd();
            self.send_cmd(&cmd)?;
        } else {
            self.send_cmd("x")?; // clear all LEDs
        }
        self.open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.open) }

    fn fire(&mut self, delta_t: f64) -> MmResult<()> {
        // Trigger a timed exposure: open, sleep (caller responsibility), close.
        // Here we just send the pattern; timing is handled externally.
        let _ = delta_t;
        self.set_open(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    const PPROPS_JSON: &str = r#"{"led_count":64,"trigger_input_count":2,"trigger_output_count":2,"part_number":1,"serial_number":42,"bit_depth":8,"color_channel_count":1,"interface_version":3.0,"mac_address":"00:11:22:33","device_name":"TestArray"}"#;

    fn make_device() -> IlluminateLedArray {
        // Only send_recv_terminated calls need script entries; send_cmd calls
        // only invoke t.send() and never call receive_line().
        let t = MockTransport::new()
            .expect("pprops\n", PPROPS_JSON)  // receive_line #1: JSON body
            .any("-==-");                      // receive_line #2: terminator
        IlluminateLedArray::new().with_transport(Box::new(t))
    }

    #[test]
    fn initialize_parses_pprops() {
        let mut d = make_device();
        d.initialize().unwrap();
        assert_eq!(d.led_count, 64);
        assert_eq!(d.trigger_input_count, 2);
        assert_eq!(d.interface_version, 3.0);
        assert_eq!(d.get_property("LedCount").unwrap(), PropertyValue::Integer(64));
    }

    #[test]
    fn set_open_sends_pattern_command() {
        let mut d = make_device();
        d.initialize().unwrap();

        let t = MockTransport::new()
            .expect("bf\n", "");
        d.transport = Some(Box::new(t));

        d.set_open(true).unwrap();
        assert!(d.get_open().unwrap());
    }

    #[test]
    fn set_closed_clears_leds() {
        let mut d = make_device();
        d.initialize().unwrap();

        let t = MockTransport::new()
            .expect("x\n", "");
        d.transport = Some(Box::new(t));

        d.set_open(false).unwrap();
        assert!(!d.get_open().unwrap());
    }

    #[test]
    fn darkfield_pattern() {
        let mut d = make_device();
        d.initialize().unwrap();
        d.set_property("Pattern", PropertyValue::String("Darkfield".into())).unwrap();

        let t = MockTransport::new()
            .expect("df\n", "");
        d.transport = Some(Box::new(t));
        d.set_open(true).unwrap();
    }

    #[test]
    fn dpc_pattern_direction() {
        let mut d = make_device();
        d.initialize().unwrap();
        d.set_property("Pattern", PropertyValue::String("DPC".into())).unwrap();
        d.set_property("DpcDirection", PropertyValue::String("bottom".into())).unwrap();

        let t = MockTransport::new()
            .expect("dpc.180\n", "");
        d.transport = Some(Box::new(t));
        d.set_open(true).unwrap();
    }

    #[test]
    fn annulus_pattern() {
        let mut d = make_device();
        d.initialize().unwrap();
        d.set_property("Pattern", PropertyValue::String("Annulus".into())).unwrap();
        d.set_property("AnnulusStartNA", PropertyValue::Float(0.3)).unwrap();
        d.set_property("AnnulusWidthNA", PropertyValue::Float(0.1)).unwrap();

        let t = MockTransport::new()
            .expect("an.30.10\n", "");
        d.transport = Some(Box::new(t));
        d.set_open(true).unwrap();
    }

    #[test]
    fn manual_led_pattern() {
        let mut d = make_device();
        d.initialize().unwrap();
        d.set_property("Pattern", PropertyValue::String("Manual".into())).unwrap();
        d.set_property("LedIndices", PropertyValue::String("0,5,10".into())).unwrap();

        let t = MockTransport::new()
            .expect("l.0.5.10\n", "");
        d.transport = Some(Box::new(t));
        d.set_open(true).unwrap();
    }

    #[test]
    fn brightness_applied_immediately_after_init() {
        let mut d = make_device();
        d.initialize().unwrap();

        let t = MockTransport::new()
            .expect("sb.200\n", "");
        d.transport = Some(Box::new(t));
        d.set_property("Brightness", PropertyValue::Integer(200)).unwrap();
        assert_eq!(d.brightness, 200);
    }

    #[test]
    fn invalid_interface_version_rejected() {
        let bad_json = r#"{"led_count":10,"trigger_input_count":0,"trigger_output_count":0,"interface_version":1.0}"#;
        let t = MockTransport::new()
            .expect("pprops\n", bad_json)
            .any("-==-");
        let mut d = IlluminateLedArray::new().with_transport(Box::new(t));
        assert!(d.initialize().is_err());
    }

    #[test]
    fn no_transport_error() {
        let mut d = IlluminateLedArray::new();
        assert!(d.initialize().is_err());
    }
}
