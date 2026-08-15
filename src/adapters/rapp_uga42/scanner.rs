use crate::error::{MmError, MmResult};
use crate::minicore::{GalvoPoint, GalvoSequenceCapability, TimingCapability};
use crate::property::PropertyMap;
use crate::traits::{Device, DeviceTriggerEdge, Galvo};
use crate::types::{DeviceType, PropertyValue};

use super::DEVICE_NAME;

const DESCRIPTION: &str = "Rapp UGA-42 Scanner";
const DEFAULT_DIGITAL_RISE_TIME_US: i64 = 1;
const DEFAULT_DIGITAL_FALL_TIME_US: i64 = 1;
const DEFAULT_ANALOG_CHANGE_TIME_US: i64 = 1;
const DEFAULT_MIN_INTENSITY: i64 = 0;
const DEFAULT_MAX_INTENSITY: i64 = 10000;
const DEFAULT_LASER_FREQUENCY_HZ: i64 = 0;
const DEFAULT_TICK_TIME_US: i64 = 50;
const DEFAULT_SPOT_SIZE: i64 = 50;
const DEFAULT_LASER_INTENSITY: i64 = 5000;
const MAX_DEVICE_COORDINATES_XY: f64 = 65535.0;
const DEFAULT_TTL_TRIGGER_MODE: &str = "None";
const DEFAULT_TTL_TRIGGER_BEHAVIOR: &str = "Rising";
const DEFAULT_LASER_TYPE: &str = "Continuous";
const DEFAULT_LASER_PORT: &str = "RMIPort1";

pub(crate) trait RappBackend: Send {
    fn connect(&mut self) -> MmResult<()>;
    fn shutdown(&mut self) -> MmResult<()>;
    fn set_scan_mode(&mut self, mode: ScanMode) -> MmResult<()>;
    fn set_spot_size(&mut self, size: i64) -> MmResult<()>;
    fn set_laser_intensity(&mut self, intensity: i64) -> MmResult<()>;
    fn set_laser_frequency(&mut self, hz: i64) -> MmResult<()>;
    fn set_timing(
        &mut self,
        tick_time_us: i64,
        digital_rise_time_us: i64,
        digital_fall_time_us: i64,
        analog_change_time_us: i64,
    ) -> MmResult<()>;
    fn move_absolute(&mut self, x: f64, y: f64) -> MmResult<()>;
    fn set_illumination(&mut self, on: bool) -> MmResult<()>;
    fn load_points(&mut self, points: &[GalvoPoint]) -> MmResult<()>;
    fn arm_sequence(&mut self, trigger: DeviceTriggerEdge) -> MmResult<()>;
    fn stop_sequence(&mut self) -> MmResult<()>;
}

struct MissingRappBackend;

impl RappBackend for MissingRappBackend {
    fn connect(&mut self) -> MmResult<()> {
        Err(MmError::NotConnected)
    }

    fn shutdown(&mut self) -> MmResult<()> {
        Ok(())
    }

    fn set_scan_mode(&mut self, _mode: ScanMode) -> MmResult<()> {
        Err(MmError::NotConnected)
    }

    fn set_spot_size(&mut self, _size: i64) -> MmResult<()> {
        Err(MmError::NotConnected)
    }

    fn set_laser_intensity(&mut self, _intensity: i64) -> MmResult<()> {
        Err(MmError::NotConnected)
    }

    fn set_laser_frequency(&mut self, _hz: i64) -> MmResult<()> {
        Err(MmError::NotConnected)
    }

    fn set_timing(
        &mut self,
        _tick_time_us: i64,
        _digital_rise_time_us: i64,
        _digital_fall_time_us: i64,
        _analog_change_time_us: i64,
    ) -> MmResult<()> {
        Err(MmError::NotConnected)
    }

    fn move_absolute(&mut self, _x: f64, _y: f64) -> MmResult<()> {
        Err(MmError::NotConnected)
    }

    fn set_illumination(&mut self, _on: bool) -> MmResult<()> {
        Err(MmError::NotConnected)
    }

    fn load_points(&mut self, _points: &[GalvoPoint]) -> MmResult<()> {
        Err(MmError::NotConnected)
    }

    fn arm_sequence(&mut self, _trigger: DeviceTriggerEdge) -> MmResult<()> {
        Err(MmError::NotConnected)
    }

    fn stop_sequence(&mut self) -> MmResult<()> {
        Err(MmError::NotConnected)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScanMode {
    Accurate,
    Fast,
}

impl ScanMode {
    fn as_property(self) -> &'static str {
        match self {
            Self::Accurate => "Accurate",
            Self::Fast => "Fast",
        }
    }

    fn from_property(value: &str) -> MmResult<Self> {
        match value {
            "Accurate" => Ok(Self::Accurate),
            "Fast" => Ok(Self::Fast),
            _ => Err(MmError::InvalidPropertyValue),
        }
    }
}

pub struct RappUga42Scanner {
    props: PropertyMap,
    backend: Box<dyn RappBackend>,
    initialized: bool,
    debug_mode: bool,
    virtual_com_port: String,
    scan_mode: ScanMode,
    spot_size: i64,
    laser_intensity: i64,
    laser_frequency_hz: i64,
    ttl_trigger_mode: String,
    ttl_trigger_behavior: String,
    laser_type: String,
    laser_port: String,
    tick_time_us: i64,
    digital_rise_time_us: i64,
    digital_fall_time_us: i64,
    analog_change_time_us: i64,
    min_intensity: i64,
    max_intensity: i64,
    polygon_repetitions: i64,
    polygon_illumination_repeats: i64,
    x: f64,
    y: f64,
    loaded_points: Vec<GalvoPoint>,
    polygons_loaded: bool,
    sequence_running: bool,
}

impl RappUga42Scanner {
    pub fn new() -> Self {
        Self::with_backend(Box::new(MissingRappBackend))
    }

    pub(crate) fn with_backend(backend: Box<dyn RappBackend>) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("DebugMode", PropertyValue::String("False".into()))
            .unwrap();
        props
            .set_allowed_values("DebugMode", &["True", "False"])
            .unwrap();
        props
            .define_pre_init_property("VirtualComPort", PropertyValue::String(String::new()))
            .unwrap();

        Self {
            props,
            backend,
            initialized: false,
            debug_mode: false,
            virtual_com_port: String::new(),
            scan_mode: ScanMode::Accurate,
            spot_size: DEFAULT_SPOT_SIZE,
            laser_intensity: DEFAULT_LASER_INTENSITY,
            laser_frequency_hz: DEFAULT_LASER_FREQUENCY_HZ,
            ttl_trigger_mode: DEFAULT_TTL_TRIGGER_MODE.into(),
            ttl_trigger_behavior: DEFAULT_TTL_TRIGGER_BEHAVIOR.into(),
            laser_type: DEFAULT_LASER_TYPE.into(),
            laser_port: DEFAULT_LASER_PORT.into(),
            tick_time_us: DEFAULT_TICK_TIME_US,
            digital_rise_time_us: DEFAULT_DIGITAL_RISE_TIME_US,
            digital_fall_time_us: DEFAULT_DIGITAL_FALL_TIME_US,
            analog_change_time_us: DEFAULT_ANALOG_CHANGE_TIME_US,
            min_intensity: DEFAULT_MIN_INTENSITY,
            max_intensity: DEFAULT_MAX_INTENSITY,
            polygon_repetitions: 1,
            polygon_illumination_repeats: 0,
            x: 0.0,
            y: 0.0,
            loaded_points: Vec::new(),
            polygons_loaded: false,
            sequence_running: false,
        }
    }

    fn define_runtime_properties(&mut self) -> MmResult<()> {
        define_if_missing(
            &mut self.props,
            "ScanMode",
            self.scan_mode.as_property(),
            false,
        )?;
        self.props
            .set_allowed_values("ScanMode", &["Accurate", "Fast"])?;
        define_if_missing(&mut self.props, "SpotSize", self.spot_size, false)?;
        self.props.set_property_limits("SpotSize", 1.0, 1000.0)?;
        define_if_missing(
            &mut self.props,
            "LaserIntensity",
            self.laser_intensity,
            false,
        )?;
        self.props.set_property_limits(
            "LaserIntensity",
            self.min_intensity as f64,
            self.max_intensity as f64,
        )?;
        define_if_missing(
            &mut self.props,
            "TTLTriggerMode",
            self.ttl_trigger_mode.as_str(),
            false,
        )?;
        self.props.set_allowed_values(
            "TTLTriggerMode",
            &["None", "Port1", "Port2", "Port1_Once", "Port2_Once"],
        )?;
        define_if_missing(
            &mut self.props,
            "TTLTriggerBehavior",
            self.ttl_trigger_behavior.as_str(),
            false,
        )?;
        self.props
            .set_allowed_values("TTLTriggerBehavior", &["Rising", "Falling"])?;
        define_if_missing(
            &mut self.props,
            "LaserType",
            self.laser_type.as_str(),
            false,
        )?;
        self.props
            .set_allowed_values("LaserType", &["Continuous", "Pulsed"])?;
        define_if_missing(
            &mut self.props,
            "LaserFrequency",
            self.laser_frequency_hz,
            false,
        )?;
        self.props
            .set_property_limits("LaserFrequency", 0.0, 100000.0)?;
        define_if_missing(&mut self.props, "TickTime", self.tick_time_us, false)?;
        self.props.set_property_limits("TickTime", 40.0, 1000.0)?;
        define_if_missing(
            &mut self.props,
            "LaserPort",
            self.laser_port.as_str(),
            false,
        )?;
        self.props.set_allowed_values(
            "LaserPort",
            &["RMIPort1", "RMIPort2", "RMIPort3", "RMIPort4"],
        )?;
        define_if_missing(
            &mut self.props,
            "DigitalRiseTime",
            self.digital_rise_time_us,
            false,
        )?;
        self.props
            .set_property_limits("DigitalRiseTime", 0.0, 10000.0)?;
        define_if_missing(
            &mut self.props,
            "DigitalFallTime",
            self.digital_fall_time_us,
            false,
        )?;
        self.props
            .set_property_limits("DigitalFallTime", 0.0, 10000.0)?;
        define_if_missing(
            &mut self.props,
            "AnalogChangeTime",
            self.analog_change_time_us,
            false,
        )?;
        self.props
            .set_property_limits("AnalogChangeTime", 0.0, 10000.0)?;
        define_if_missing(&mut self.props, "MinIntensity", self.min_intensity, false)?;
        self.props
            .set_property_limits("MinIntensity", 0.0, 10000.0)?;
        define_if_missing(&mut self.props, "MaxIntensity", self.max_intensity, false)?;
        self.props
            .set_property_limits("MaxIntensity", 0.0, 10000.0)?;
        define_if_missing(
            &mut self.props,
            "PolygonRepetitions",
            self.polygon_repetitions,
            false,
        )?;
        self.props
            .set_property_limits("PolygonRepetitions", 0.0, f64::MAX)?;
        define_if_missing(
            &mut self.props,
            "PolygonIlluminationRepeats",
            self.polygon_illumination_repeats,
            false,
        )?;
        self.props
            .set_property_limits("PolygonIlluminationRepeats", 0.0, 1000.0)?;
        Ok(())
    }

    fn ensure_initialized(&self) -> MmResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(MmError::NotConnected)
        }
    }

    fn set_integer_property<F>(
        &mut self,
        name: &str,
        value: PropertyValue,
        mut apply: F,
    ) -> MmResult<()>
    where
        F: FnMut(&mut Self, i64) -> MmResult<()>,
    {
        let numeric = value.as_i64().ok_or(MmError::InvalidPropertyValue)?;
        self.props.set(name, PropertyValue::Integer(numeric))?;
        apply(self, numeric)
    }
}

impl Default for RappUga42Scanner {
    fn default() -> Self {
        Self::new()
    }
}

fn define_if_missing(
    props: &mut PropertyMap,
    name: &str,
    value: impl Into<PropertyValue>,
    read_only: bool,
) -> MmResult<()> {
    if props.has_property(name) {
        Ok(())
    } else {
        props.define_property(name, value, read_only)
    }
}

impl Device for RappUga42Scanner {
    fn name(&self) -> &str {
        DEVICE_NAME
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.initialized {
            return Ok(());
        }
        self.backend.connect()?;
        self.define_runtime_properties()?;
        self.backend.set_scan_mode(self.scan_mode)?;
        self.backend.set_spot_size(self.spot_size)?;
        self.backend.set_laser_intensity(self.laser_intensity)?;
        self.backend.set_laser_frequency(self.laser_frequency_hz)?;
        self.backend.set_timing(
            self.tick_time_us,
            self.digital_rise_time_us,
            self.digital_fall_time_us,
            self.analog_change_time_us,
        )?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.sequence_running = false;
        self.polygons_loaded = false;
        self.loaded_points.clear();
        self.initialized = false;
        self.backend.shutdown()
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "DebugMode" => Ok(PropertyValue::String(
                if self.debug_mode { "True" } else { "False" }.into(),
            )),
            "VirtualComPort" => Ok(PropertyValue::String(self.virtual_com_port.clone())),
            "ScanMode" => Ok(PropertyValue::String(self.scan_mode.as_property().into())),
            "SpotSize" => Ok(PropertyValue::Integer(self.spot_size)),
            "LaserIntensity" => Ok(PropertyValue::Integer(self.laser_intensity)),
            "TTLTriggerMode" => Ok(PropertyValue::String(self.ttl_trigger_mode.clone())),
            "TTLTriggerBehavior" => Ok(PropertyValue::String(self.ttl_trigger_behavior.clone())),
            "LaserType" => Ok(PropertyValue::String(self.laser_type.clone())),
            "LaserFrequency" => Ok(PropertyValue::Integer(self.laser_frequency_hz)),
            "TickTime" => Ok(PropertyValue::Integer(self.tick_time_us)),
            "LaserPort" => Ok(PropertyValue::String(self.laser_port.clone())),
            "DigitalRiseTime" => Ok(PropertyValue::Integer(self.digital_rise_time_us)),
            "DigitalFallTime" => Ok(PropertyValue::Integer(self.digital_fall_time_us)),
            "AnalogChangeTime" => Ok(PropertyValue::Integer(self.analog_change_time_us)),
            "MinIntensity" => Ok(PropertyValue::Integer(self.min_intensity)),
            "MaxIntensity" => Ok(PropertyValue::Integer(self.max_intensity)),
            "PolygonRepetitions" => Ok(PropertyValue::Integer(self.polygon_repetitions)),
            "PolygonIlluminationRepeats" => {
                Ok(PropertyValue::Integer(self.polygon_illumination_repeats))
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, value: PropertyValue) -> MmResult<()> {
        match name {
            "DebugMode" => {
                if self.initialized {
                    return Err(MmError::CanNotSetProperty);
                }
                self.props.set(name, value.clone())?;
                self.debug_mode = match value.as_str() {
                    "True" => true,
                    "False" => false,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                Ok(())
            }
            "VirtualComPort" => {
                if self.initialized {
                    return Err(MmError::CanNotSetProperty);
                }
                self.props.set(name, value.clone())?;
                self.virtual_com_port = value.as_str().to_string();
                Ok(())
            }
            "ScanMode" => {
                let mode = ScanMode::from_property(value.as_str())?;
                self.props
                    .set(name, PropertyValue::String(mode.as_property().into()))?;
                if self.initialized {
                    self.backend.set_scan_mode(mode)?;
                }
                self.scan_mode = mode;
                Ok(())
            }
            "SpotSize" => self.set_integer_property(name, value, |this, numeric| {
                if this.initialized {
                    this.backend.set_spot_size(numeric)?;
                }
                this.spot_size = numeric;
                Ok(())
            }),
            "LaserIntensity" => self.set_integer_property(name, value, |this, numeric| {
                if numeric < this.min_intensity || numeric > this.max_intensity {
                    return Err(MmError::InvalidPropertyValue);
                }
                if this.initialized {
                    this.backend.set_laser_intensity(numeric)?;
                }
                this.laser_intensity = numeric;
                Ok(())
            }),
            "TTLTriggerMode" => {
                self.props.set(name, value.clone())?;
                self.ttl_trigger_mode = value.as_str().to_string();
                Ok(())
            }
            "TTLTriggerBehavior" => {
                self.props.set(name, value.clone())?;
                self.ttl_trigger_behavior = value.as_str().to_string();
                Ok(())
            }
            "LaserType" => {
                self.props.set(name, value.clone())?;
                self.laser_type = value.as_str().to_string();
                if self.laser_type == "Continuous" {
                    self.laser_frequency_hz = 0;
                    if self.props.has_property("LaserFrequency") {
                        self.props
                            .set("LaserFrequency", PropertyValue::Integer(0))?;
                    }
                    if self.initialized {
                        self.backend.set_laser_frequency(0)?;
                    }
                }
                Ok(())
            }
            "LaserFrequency" => self.set_integer_property(name, value, |this, numeric| {
                if this.initialized {
                    this.backend.set_laser_frequency(numeric)?;
                }
                this.laser_frequency_hz = numeric;
                if numeric > 0 {
                    this.laser_type = "Pulsed".into();
                    if this.props.has_property("LaserType") {
                        this.props
                            .set("LaserType", PropertyValue::String("Pulsed".into()))?;
                    }
                }
                Ok(())
            }),
            "LaserPort" => {
                self.props.set(name, value.clone())?;
                self.laser_port = value.as_str().to_string();
                Ok(())
            }
            "TickTime" => self.set_integer_property(name, value, |this, numeric| {
                this.tick_time_us = numeric;
                if this.initialized {
                    this.backend.set_timing(
                        this.tick_time_us,
                        this.digital_rise_time_us,
                        this.digital_fall_time_us,
                        this.analog_change_time_us,
                    )?;
                }
                Ok(())
            }),
            "DigitalRiseTime" => self.set_integer_property(name, value, |this, numeric| {
                this.digital_rise_time_us = numeric;
                if this.initialized {
                    this.backend.set_timing(
                        this.tick_time_us,
                        this.digital_rise_time_us,
                        this.digital_fall_time_us,
                        this.analog_change_time_us,
                    )?;
                }
                Ok(())
            }),
            "DigitalFallTime" => self.set_integer_property(name, value, |this, numeric| {
                this.digital_fall_time_us = numeric;
                if this.initialized {
                    this.backend.set_timing(
                        this.tick_time_us,
                        this.digital_rise_time_us,
                        this.digital_fall_time_us,
                        this.analog_change_time_us,
                    )?;
                }
                Ok(())
            }),
            "AnalogChangeTime" => self.set_integer_property(name, value, |this, numeric| {
                this.analog_change_time_us = numeric;
                if this.initialized {
                    this.backend.set_timing(
                        this.tick_time_us,
                        this.digital_rise_time_us,
                        this.digital_fall_time_us,
                        this.analog_change_time_us,
                    )?;
                }
                Ok(())
            }),
            "MinIntensity" => self.set_integer_property(name, value, |this, numeric| {
                if numeric > this.max_intensity {
                    return Err(MmError::InvalidPropertyValue);
                }
                this.min_intensity = numeric;
                Ok(())
            }),
            "MaxIntensity" => self.set_integer_property(name, value, |this, numeric| {
                if numeric < this.min_intensity {
                    return Err(MmError::InvalidPropertyValue);
                }
                this.max_intensity = numeric;
                Ok(())
            }),
            "PolygonRepetitions" => self.set_integer_property(name, value, |this, numeric| {
                this.polygon_repetitions = numeric;
                Ok(())
            }),
            "PolygonIlluminationRepeats" => {
                self.set_integer_property(name, value, |this, numeric| {
                    this.polygon_illumination_repeats = numeric;
                    Ok(())
                })
            }
            _ => self.props.set(name, value),
        }
    }

    fn property_names(&self) -> Vec<String> {
        self.props.property_names().to_vec()
    }

    fn has_property(&self, name: &str) -> bool {
        self.props.has_property(name)
    }

    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).is_some_and(|entry| entry.read_only)
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Galvo
    }

    fn timing_capability(&self) -> Option<TimingCapability> {
        Some(TimingCapability {
            external_trigger: true,
            software_trigger: true,
            property_sequence: false,
            signal_sequence: false,
            galvo_sequence: true,
            waveform_upload: false,
            max_sequence_len: None,
        })
    }

    fn galvo_sequence_capability(&self) -> Option<GalvoSequenceCapability> {
        Some(GalvoSequenceCapability {
            max_len: usize::MAX,
            min_x: 0.0,
            max_x: MAX_DEVICE_COORDINATES_XY,
            min_y: 0.0,
            max_y: MAX_DEVICE_COORDINATES_XY,
            min_dwell_ms: Some(0.0),
            max_dwell_ms: None,
            requires_trigger: false,
        })
    }

    fn busy(&self) -> bool {
        self.sequence_running
    }
}

impl Galvo for RappUga42Scanner {
    fn set_position(&mut self, x: f64, y: f64) -> MmResult<()> {
        self.ensure_initialized()?;
        validate_coordinate(x)?;
        validate_coordinate(y)?;
        self.backend.move_absolute(x, y)?;
        self.x = x;
        self.y = y;
        Ok(())
    }

    fn get_position(&self) -> MmResult<(f64, f64)> {
        self.ensure_initialized()?;
        Ok((self.x, self.y))
    }

    fn set_illumination_state(&mut self, on: bool) -> MmResult<()> {
        self.ensure_initialized()?;
        self.backend.set_illumination(on)
    }

    fn get_x_range(&self) -> MmResult<(f64, f64)> {
        Ok((0.0, MAX_DEVICE_COORDINATES_XY))
    }

    fn get_y_range(&self) -> MmResult<(f64, f64)> {
        Ok((0.0, MAX_DEVICE_COORDINATES_XY))
    }

    fn load_galvo_sequence(&mut self, points: &[GalvoPoint]) -> MmResult<()> {
        self.ensure_initialized()?;
        for point in points {
            validate_coordinate(point.x)?;
            validate_coordinate(point.y)?;
            if !point.dwell_ms.is_finite() || point.dwell_ms < 0.0 {
                return Err(MmError::InvalidPropertyValue);
            }
        }
        self.backend.load_points(points)?;
        self.loaded_points = points.to_vec();
        self.polygons_loaded = !self.loaded_points.is_empty();
        Ok(())
    }

    fn arm_galvo_sequence(&mut self, trigger: DeviceTriggerEdge) -> MmResult<()> {
        self.ensure_initialized()?;
        if !self.polygons_loaded || self.loaded_points.is_empty() {
            return Err(MmError::InvalidPropertyValue);
        }
        self.backend.arm_sequence(trigger)?;
        self.sequence_running = true;
        Ok(())
    }

    fn stop_galvo_sequence(&mut self) -> MmResult<()> {
        self.ensure_initialized()?;
        self.backend.stop_sequence()?;
        self.sequence_running = false;
        Ok(())
    }
}

fn validate_coordinate(value: f64) -> MmResult<()> {
    if value.is_finite() && (0.0..=MAX_DEVICE_COORDINATES_XY).contains(&value) {
        Ok(())
    } else {
        Err(MmError::InvalidPropertyValue)
    }
}

#[cfg(test)]
#[derive(Default, Debug)]
pub(crate) struct MockRappBackend {
    shared: std::sync::Arc<std::sync::Mutex<MockRappBackendState>>,
}

#[cfg(test)]
impl MockRappBackend {
    pub(crate) fn shared(&self) -> std::sync::Arc<std::sync::Mutex<MockRappBackendState>> {
        self.shared.clone()
    }
}

#[cfg(test)]
#[derive(Default, Debug)]
pub(crate) struct MockRappBackendState {
    pub(crate) connected: bool,
    pub(crate) moves: Vec<(f64, f64)>,
    pub(crate) illumination: Vec<bool>,
    pub(crate) loaded_points: Vec<GalvoPoint>,
    pub(crate) armed_edges: Vec<DeviceTriggerEdge>,
    pub(crate) stopped: bool,
}

#[cfg(test)]
impl RappBackend for MockRappBackend {
    fn connect(&mut self) -> MmResult<()> {
        self.shared.lock().unwrap().connected = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.shared.lock().unwrap().connected = false;
        Ok(())
    }

    fn set_scan_mode(&mut self, _mode: ScanMode) -> MmResult<()> {
        Ok(())
    }

    fn set_spot_size(&mut self, _size: i64) -> MmResult<()> {
        Ok(())
    }

    fn set_laser_intensity(&mut self, _intensity: i64) -> MmResult<()> {
        Ok(())
    }

    fn set_laser_frequency(&mut self, _hz: i64) -> MmResult<()> {
        Ok(())
    }

    fn set_timing(
        &mut self,
        _tick_time_us: i64,
        _digital_rise_time_us: i64,
        _digital_fall_time_us: i64,
        _analog_change_time_us: i64,
    ) -> MmResult<()> {
        Ok(())
    }

    fn move_absolute(&mut self, x: f64, y: f64) -> MmResult<()> {
        self.shared.lock().unwrap().moves.push((x, y));
        Ok(())
    }

    fn set_illumination(&mut self, on: bool) -> MmResult<()> {
        self.shared.lock().unwrap().illumination.push(on);
        Ok(())
    }

    fn load_points(&mut self, points: &[GalvoPoint]) -> MmResult<()> {
        self.shared.lock().unwrap().loaded_points = points.to_vec();
        Ok(())
    }

    fn arm_sequence(&mut self, trigger: DeviceTriggerEdge) -> MmResult<()> {
        self.shared.lock().unwrap().armed_edges.push(trigger);
        Ok(())
    }

    fn stop_sequence(&mut self) -> MmResult<()> {
        self.shared.lock().unwrap().stopped = true;
        Ok(())
    }
}
