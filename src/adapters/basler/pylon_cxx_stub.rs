use std::fmt;
use std::marker::PhantomData;

#[derive(Debug, Clone)]
pub struct PylonError(String);

impl PylonError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for PylonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PylonError {}

pub trait HasProperties {
    fn property_value(&self, name: &str) -> Result<String, PylonError>;
}

pub struct Pylon;

impl Pylon {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Clone)]
pub struct DeviceInfo {
    serial_number: String,
}

impl HasProperties for DeviceInfo {
    fn property_value(&self, name: &str) -> Result<String, PylonError> {
        match name {
            "SerialNumber" => Ok(self.serial_number.clone()),
            _ => Err(PylonError::new("unknown property")),
        }
    }
}

pub struct Device;

pub struct TlFactory<'a> {
    _pylon: PhantomData<&'a Pylon>,
}

impl<'a> TlFactory<'a> {
    pub fn instance(_pylon: &'a Pylon) -> Self {
        Self {
            _pylon: PhantomData,
        }
    }

    pub fn enumerate_devices(&self) -> Result<Vec<DeviceInfo>, PylonError> {
        Ok(Vec::new())
    }

    pub fn create_device(&self, _info: &DeviceInfo) -> Result<Device, PylonError> {
        Err(PylonError::new("Pylon stub has no devices"))
    }
}

pub struct InstantCamera<'a> {
    _device: Device,
    _lifetime: PhantomData<&'a ()>,
}

impl<'a> InstantCamera<'a> {
    pub fn new(device: Device) -> Result<Self, PylonError> {
        Ok(Self {
            _device: device,
            _lifetime: PhantomData,
        })
    }

    pub fn open(&self) -> Result<(), PylonError> {
        Ok(())
    }

    pub fn close(&self) -> Result<(), PylonError> {
        Ok(())
    }

    pub fn node_map(&self) -> Result<NodeMap, PylonError> {
        Ok(NodeMap)
    }

    pub fn start_grabbing(&self, _options: &GrabOptions) -> Result<(), PylonError> {
        Ok(())
    }

    pub fn stop_grabbing(&self) -> Result<(), PylonError> {
        Ok(())
    }

    pub fn retrieve_result(
        &self,
        _timeout_ms: u64,
        _result: &mut GrabResult,
        _timeout_handling: TimeoutHandling,
    ) -> Result<(), PylonError> {
        Err(PylonError::new("Pylon stub has no frames"))
    }
}

pub struct NodeMap;

impl NodeMap {
    pub fn float_node(&self, name: &str) -> Result<FloatNode, PylonError> {
        match name {
            "ExposureTime" | "ExposureTimeAbs" => Ok(FloatNode {
                value: 10_000.0,
                min: 1.0,
                max: 1_000_000.0,
            }),
            "Gain" | "DeviceTemperature" => Ok(FloatNode {
                value: 0.0,
                min: 0.0,
                max: 12.0,
            }),
            _ => Err(PylonError::new("unknown float node")),
        }
    }

    pub fn integer_node(&self, name: &str) -> Result<IntegerNode, PylonError> {
        match name {
            "Width" => Ok(IntegerNode {
                value: 64,
                min: 1,
                max: 64,
            }),
            "Height" => Ok(IntegerNode {
                value: 48,
                min: 1,
                max: 48,
            }),
            "OffsetX" | "OffsetY" => Ok(IntegerNode {
                value: 0,
                min: 0,
                max: 0,
            }),
            "GainRaw" => Ok(IntegerNode {
                value: 0,
                min: 0,
                max: 12,
            }),
            "BinningHorizontal" | "BinningVertical" => Ok(IntegerNode {
                value: 1,
                min: 1,
                max: 4,
            }),
            _ => Err(PylonError::new("unknown integer node")),
        }
    }

    pub fn enum_node(&self, name: &str) -> Result<EnumNode, PylonError> {
        match name {
            "PixelFormat" => Ok(EnumNode {
                value: "Mono8".to_string(),
            }),
            _ => Err(PylonError::new("unknown enum node")),
        }
    }
}

pub struct FloatNode {
    value: f64,
    min: f64,
    max: f64,
}

impl FloatNode {
    pub fn value(&self) -> Result<f64, PylonError> {
        Ok(self.value)
    }

    pub fn min(&self) -> Result<f64, PylonError> {
        Ok(self.min)
    }

    pub fn max(&self) -> Result<f64, PylonError> {
        Ok(self.max)
    }

    pub fn set_value(&mut self, value: f64) -> Result<(), PylonError> {
        self.value = value.clamp(self.min, self.max);
        Ok(())
    }
}

pub struct IntegerNode {
    value: i64,
    min: i64,
    max: i64,
}

impl IntegerNode {
    pub fn value(&self) -> Result<i64, PylonError> {
        Ok(self.value)
    }

    pub fn min(&self) -> Result<i64, PylonError> {
        Ok(self.min)
    }

    pub fn max(&self) -> Result<i64, PylonError> {
        Ok(self.max)
    }

    pub fn set_value(&mut self, value: i64) -> Result<(), PylonError> {
        self.value = value.clamp(self.min, self.max);
        Ok(())
    }
}

pub struct EnumNode {
    value: String,
}

impl EnumNode {
    pub fn value(&self) -> Result<String, PylonError> {
        Ok(self.value.clone())
    }

    pub fn set_value(&mut self, value: &str) -> Result<(), PylonError> {
        self.value = value.to_string();
        Ok(())
    }

    pub fn settable_values(&self) -> Result<Vec<String>, PylonError> {
        Ok(vec![
            "Mono8".to_string(),
            "Mono12".to_string(),
            "RGB8".to_string(),
            "BayerGR8".to_string(),
        ])
    }
}

#[derive(Default)]
pub struct GrabOptions {
    count: Option<u64>,
}

impl GrabOptions {
    pub fn count(mut self, count: u64) -> Self {
        self.count = Some(count);
        self
    }
}

pub struct GrabResult {
    width: u32,
    height: u32,
    buffer: Vec<u8>,
}

impl GrabResult {
    pub fn new() -> Result<Self, PylonError> {
        Ok(Self {
            width: 64,
            height: 48,
            buffer: vec![0; 64 * 48],
        })
    }

    pub fn grab_succeeded(&self) -> Result<bool, PylonError> {
        Ok(true)
    }

    pub fn buffer(&self) -> Result<&[u8], PylonError> {
        Ok(&self.buffer)
    }

    pub fn width(&self) -> Result<u32, PylonError> {
        Ok(self.width)
    }

    pub fn height(&self) -> Result<u32, PylonError> {
        Ok(self.height)
    }
}

pub enum TimeoutHandling {
    ThrowException,
}
