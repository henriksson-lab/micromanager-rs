/// Device type enumeration mirroring MM::DeviceType
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Unknown,
    Any,
    Camera,
    Shutter,
    State,
    Stage,
    XYStage,
    Serial,
    Generic,
    AutoFocus,
    Core,
    ImageProcessor,
    SignalIO,
    Magnifier,
    SLM,
    Hub,
    Galvo,
    PressurePump,
    VolumetricPump,
}

/// Property type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyType {
    Undef,
    String,
    Float,
    Integer,
}

/// A typed property value
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    String(String),
    Float(f64),
    Integer(i64),
}

impl PropertyValue {
    pub fn as_str(&self) -> &str {
        match self {
            PropertyValue::String(s) => s.as_str(),
            _ => "",
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            PropertyValue::Float(v) => Some(*v),
            PropertyValue::Integer(v) => Some(*v as f64),
            PropertyValue::String(s) => s.parse().ok(),
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            PropertyValue::Integer(v) => Some(*v),
            PropertyValue::Float(v) => Some(*v as i64),
            PropertyValue::String(s) => s.parse().ok(),
        }
    }

    pub fn property_type(&self) -> PropertyType {
        match self {
            PropertyValue::String(_) => PropertyType::String,
            PropertyValue::Float(_) => PropertyType::Float,
            PropertyValue::Integer(_) => PropertyType::Integer,
        }
    }
}

impl From<&str> for PropertyValue {
    fn from(s: &str) -> Self {
        PropertyValue::String(s.to_string())
    }
}

impl From<String> for PropertyValue {
    fn from(s: String) -> Self {
        PropertyValue::String(s)
    }
}

impl From<f64> for PropertyValue {
    fn from(v: f64) -> Self {
        PropertyValue::Float(v)
    }
}

impl From<i64> for PropertyValue {
    fn from(v: i64) -> Self {
        PropertyValue::Integer(v)
    }
}

impl std::fmt::Display for PropertyValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PropertyValue::String(s) => write!(f, "{}", s),
            PropertyValue::Float(v) => write!(f, "{}", v),
            PropertyValue::Integer(v) => write!(f, "{}", v),
        }
    }
}

/// Focus direction for a Z stage
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Unknown,
    TowardSample,
    AwayFromSample,
}

/// Region of interest for camera
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageRoi {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl ImageRoi {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }
}
