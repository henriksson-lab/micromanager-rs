use crate::error::MmResult;
use crate::types::{DeviceType, FocusDirection, ImageRoi, PropertyValue};

// ─── Base device trait ──────────────────────────────────────────────────────

/// Base trait that every device must implement.
/// Mirrors `MM::Device` from MMDevice.h.
pub trait Device: Send {
    fn name(&self) -> &str;
    fn description(&self) -> &str;

    fn initialize(&mut self) -> MmResult<()>;
    fn shutdown(&mut self) -> MmResult<()>;

    fn get_property(&self, name: &str) -> MmResult<PropertyValue>;
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()>;
    fn property_names(&self) -> Vec<String>;
    fn has_property(&self, name: &str) -> bool;
    fn is_property_read_only(&self, name: &str) -> bool;

    fn device_type(&self) -> DeviceType;
    fn busy(&self) -> bool;
}

// ─── Camera ─────────────────────────────────────────────────────────────────

/// Camera device trait mirroring `MM::Camera`.
pub trait Camera: Device {
    /// Perform exposure and block until the exposure is done.
    fn snap_image(&mut self) -> MmResult<()>;

    /// Return the image data captured by the last `snap_image` call.
    fn get_image_buffer(&self) -> MmResult<&[u8]>;

    fn get_image_width(&self) -> u32;
    fn get_image_height(&self) -> u32;
    fn get_image_bytes_per_pixel(&self) -> u32;
    fn get_bit_depth(&self) -> u32;
    fn get_number_of_components(&self) -> u32;
    fn get_number_of_channels(&self) -> u32;

    fn get_exposure(&self) -> f64;
    fn set_exposure(&mut self, exp_ms: f64);

    fn get_binning(&self) -> i32;
    fn set_binning(&mut self, bin: i32) -> MmResult<()>;

    fn get_roi(&self) -> MmResult<ImageRoi>;
    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()>;
    fn clear_roi(&mut self) -> MmResult<()>;

    fn start_sequence_acquisition(&mut self, count: i64, interval_ms: f64) -> MmResult<()>;
    fn stop_sequence_acquisition(&mut self) -> MmResult<()>;
    fn is_capturing(&self) -> bool;
}

// ─── Stage (single-axis Z) ───────────────────────────────────────────────────

/// Single-axis focus/Z stage mirroring `MM::Stage`.
pub trait Stage: Device {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()>;
    fn get_position_um(&self) -> MmResult<f64>;
    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()>;
    fn home(&mut self) -> MmResult<()>;
    fn stop(&mut self) -> MmResult<()>;
    fn get_limits(&self) -> MmResult<(f64, f64)>;
    fn get_focus_direction(&self) -> FocusDirection;
    fn is_continuous_focus_drive(&self) -> bool;
}

// ─── XYStage ─────────────────────────────────────────────────────────────────

/// Dual-axis XY stage mirroring `MM::XYStage`.
pub trait XYStage: Device {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()>;
    fn get_xy_position_um(&self) -> MmResult<(f64, f64)>;
    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()>;
    fn home(&mut self) -> MmResult<()>;
    fn stop(&mut self) -> MmResult<()>;
    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)>;
    fn get_step_size_um(&self) -> (f64, f64);
    fn set_origin(&mut self) -> MmResult<()>;
}

// ─── Shutter ─────────────────────────────────────────────────────────────────

/// Shutter device mirroring `MM::Shutter`.
pub trait Shutter: Device {
    fn set_open(&mut self, open: bool) -> MmResult<()>;
    fn get_open(&self) -> MmResult<bool>;
    fn fire(&mut self, delta_t: f64) -> MmResult<()>;
}

// ─── State device (filter wheel, etc.) ────────────────────────────────────────

/// State device (filter wheel, objective turret) mirroring `MM::State`.
pub trait StateDevice: Device {
    fn set_position(&mut self, pos: u64) -> MmResult<()>;
    fn get_position(&self) -> MmResult<u64>;
    fn get_number_of_positions(&self) -> u64;
    fn get_position_label(&self, pos: u64) -> MmResult<String>;
    fn set_position_by_label(&mut self, label: &str) -> MmResult<()>;
    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()>;
    fn set_gate_open(&mut self, open: bool) -> MmResult<()>;
    fn get_gate_open(&self) -> MmResult<bool>;
}

// ─── Hub ──────────────────────────────────────────────────────────────────────

/// Hub device mirroring `MM::Hub`.
pub trait Hub: Device {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>>;
}

// ─── AutoFocus ────────────────────────────────────────────────────────────────

/// Auto-focus device mirroring `MM::AutoFocus`.
pub trait AutoFocus: Device {
    fn set_continuous_focusing(&mut self, state: bool) -> MmResult<()>;
    fn get_continuous_focusing(&self) -> MmResult<bool>;
    fn is_continuous_focus_locked(&self) -> bool;
    fn full_focus(&mut self) -> MmResult<()>;
    fn incremental_focus(&mut self) -> MmResult<()>;
    fn get_last_focus_score(&self) -> MmResult<f64>;
    fn get_current_focus_score(&self) -> MmResult<f64>;
    fn get_offset(&self) -> MmResult<f64>;
    fn set_offset(&mut self, offset: f64) -> MmResult<()>;
}

// ─── ImageProcessor ───────────────────────────────────────────────────────────

/// Image processor mirroring `MM::ImageProcessor`.
pub trait ImageProcessor: Device {
    fn process(&mut self, buffer: &mut [u8], width: u32, height: u32, byte_depth: u32) -> MmResult<()>;
}

// ─── SignalIO (DAC/ADC) ───────────────────────────────────────────────────────

/// Analog I/O device mirroring `MM::SignalIO`.
pub trait SignalIO: Device {
    fn set_gate_open(&mut self, open: bool) -> MmResult<()>;
    fn get_gate_open(&self) -> MmResult<bool>;
    fn set_signal(&mut self, volts: f64) -> MmResult<()>;
    fn get_signal(&self) -> MmResult<f64>;
    fn get_limits(&self) -> MmResult<(f64, f64)>;
}

// ─── Magnifier ────────────────────────────────────────────────────────────────

/// Magnifier device mirroring `MM::MagnifierDevice`.
pub trait MagnifierDevice: Device {
    fn get_magnification(&self) -> MmResult<f64>;
}

// ─── SLM ──────────────────────────────────────────────────────────────────────

/// Spatial light modulator mirroring `MM::SLM`.
pub trait Slm: Device {
    fn set_image(&mut self, pixels: &[u8]) -> MmResult<()>;
    fn display_image(&mut self) -> MmResult<()>;
    fn set_exposure(&mut self, exp_ms: f64) -> MmResult<()>;
    fn get_exposure(&self) -> MmResult<f64>;
    fn get_width(&self) -> u32;
    fn get_height(&self) -> u32;
    fn get_number_of_components(&self) -> u32;
    fn get_bytes_per_pixel(&self) -> u32;
}

// ─── Galvo ────────────────────────────────────────────────────────────────────

/// Galvo device mirroring `MM::Galvo`.
pub trait Galvo: Device {
    fn set_position(&mut self, x: f64, y: f64) -> MmResult<()>;
    fn get_position(&self) -> MmResult<(f64, f64)>;
    fn set_illumination_state(&mut self, on: bool) -> MmResult<()>;
    fn get_x_range(&self) -> MmResult<(f64, f64)>;
    fn get_y_range(&self) -> MmResult<(f64, f64)>;
}

// ─── Generic ──────────────────────────────────────────────────────────────────

/// Generic device (no extra methods beyond `Device`).
pub trait Generic: Device {}

// ─── Serial ───────────────────────────────────────────────────────────────────

/// Serial port device mirroring `MM::Serial`.
pub trait SerialPort: Device {
    fn send_command(&mut self, command: &str, terminator: &str) -> MmResult<()>;
    fn get_answer(&mut self, max_chars: usize, terminator: &str) -> MmResult<String>;
    fn write_raw(&mut self, buf: &[u8]) -> MmResult<()>;
    fn read_raw(&mut self, buf: &mut [u8]) -> MmResult<usize>;
    fn purge(&mut self) -> MmResult<()>;
}

// ─── Pressure / Volumetric pump ───────────────────────────────────────────────

/// Pressure pump device mirroring `MM::PressurePump`.
pub trait PressurePump: Device {
    fn set_pressure(&mut self, pressure: f64) -> MmResult<()>;
    fn get_pressure(&self) -> MmResult<f64>;
    fn start(&mut self) -> MmResult<()>;
    fn stop(&mut self) -> MmResult<()>;
}

/// Volumetric pump device mirroring `MM::VolumetricPump`.
pub trait VolumetricPump: Device {
    fn set_volume_ul(&mut self, volume: f64) -> MmResult<()>;
    fn get_volume_ul(&self) -> MmResult<f64>;
    fn set_flow_rate(&mut self, rate_ul_per_s: f64) -> MmResult<()>;
    fn get_flow_rate(&self) -> MmResult<f64>;
    fn start(&mut self) -> MmResult<()>;
    fn stop(&mut self) -> MmResult<()>;
    fn is_running(&self) -> bool;
}

// ─── AnyDevice enum (solves the downcast problem) ────────────────────────────

/// Type-safe wrapper for all device variants.
/// This is the type stored in `DeviceManager`, allowing typed dispatch without
/// `Any`-based downcasting.
pub enum AnyDevice {
    Camera(Box<dyn Camera>),
    Stage(Box<dyn Stage>),
    XYStage(Box<dyn XYStage>),
    Shutter(Box<dyn Shutter>),
    StateDevice(Box<dyn StateDevice>),
    Hub(Box<dyn Hub>),
    AutoFocus(Box<dyn AutoFocus>),
    ImageProcessor(Box<dyn ImageProcessor>),
    SignalIO(Box<dyn SignalIO>),
    MagnifierDevice(Box<dyn MagnifierDevice>),
    Slm(Box<dyn Slm>),
    Galvo(Box<dyn Galvo>),
    Generic(Box<dyn Generic>),
    SerialPort(Box<dyn SerialPort>),
    PressurePump(Box<dyn PressurePump>),
    VolumetricPump(Box<dyn VolumetricPump>),
}

impl AnyDevice {
    /// Delegate to the inner `Device` trait implementation.
    pub fn as_device(&self) -> &dyn Device {
        match self {
            AnyDevice::Camera(d) => d.as_ref(),
            AnyDevice::Stage(d) => d.as_ref(),
            AnyDevice::XYStage(d) => d.as_ref(),
            AnyDevice::Shutter(d) => d.as_ref(),
            AnyDevice::StateDevice(d) => d.as_ref(),
            AnyDevice::Hub(d) => d.as_ref(),
            AnyDevice::AutoFocus(d) => d.as_ref(),
            AnyDevice::ImageProcessor(d) => d.as_ref(),
            AnyDevice::SignalIO(d) => d.as_ref(),
            AnyDevice::MagnifierDevice(d) => d.as_ref(),
            AnyDevice::Slm(d) => d.as_ref(),
            AnyDevice::Galvo(d) => d.as_ref(),
            AnyDevice::Generic(d) => d.as_ref(),
            AnyDevice::SerialPort(d) => d.as_ref(),
            AnyDevice::PressurePump(d) => d.as_ref(),
            AnyDevice::VolumetricPump(d) => d.as_ref(),
        }
    }

    pub fn as_device_mut(&mut self) -> &mut dyn Device {
        match self {
            AnyDevice::Camera(d) => d.as_mut(),
            AnyDevice::Stage(d) => d.as_mut(),
            AnyDevice::XYStage(d) => d.as_mut(),
            AnyDevice::Shutter(d) => d.as_mut(),
            AnyDevice::StateDevice(d) => d.as_mut(),
            AnyDevice::Hub(d) => d.as_mut(),
            AnyDevice::AutoFocus(d) => d.as_mut(),
            AnyDevice::ImageProcessor(d) => d.as_mut(),
            AnyDevice::SignalIO(d) => d.as_mut(),
            AnyDevice::MagnifierDevice(d) => d.as_mut(),
            AnyDevice::Slm(d) => d.as_mut(),
            AnyDevice::Galvo(d) => d.as_mut(),
            AnyDevice::Generic(d) => d.as_mut(),
            AnyDevice::SerialPort(d) => d.as_mut(),
            AnyDevice::PressurePump(d) => d.as_mut(),
            AnyDevice::VolumetricPump(d) => d.as_mut(),
        }
    }

    pub fn as_camera(&self) -> Option<&dyn Camera> {
        if let AnyDevice::Camera(d) = self { Some(d.as_ref()) } else { None }
    }

    pub fn as_camera_mut(&mut self) -> Option<&mut dyn Camera> {
        if let AnyDevice::Camera(d) = self { Some(d.as_mut()) } else { None }
    }

    pub fn as_stage(&self) -> Option<&dyn Stage> {
        if let AnyDevice::Stage(d) = self { Some(d.as_ref()) } else { None }
    }

    pub fn as_stage_mut(&mut self) -> Option<&mut dyn Stage> {
        if let AnyDevice::Stage(d) = self { Some(d.as_mut()) } else { None }
    }

    pub fn as_xystage(&self) -> Option<&dyn XYStage> {
        if let AnyDevice::XYStage(d) = self { Some(d.as_ref()) } else { None }
    }

    pub fn as_xystage_mut(&mut self) -> Option<&mut dyn XYStage> {
        if let AnyDevice::XYStage(d) = self { Some(d.as_mut()) } else { None }
    }

    pub fn as_shutter(&self) -> Option<&dyn Shutter> {
        if let AnyDevice::Shutter(d) = self { Some(d.as_ref()) } else { None }
    }

    pub fn as_shutter_mut(&mut self) -> Option<&mut dyn Shutter> {
        if let AnyDevice::Shutter(d) = self { Some(d.as_mut()) } else { None }
    }

    pub fn as_state_device(&self) -> Option<&dyn StateDevice> {
        if let AnyDevice::StateDevice(d) = self { Some(d.as_ref()) } else { None }
    }

    pub fn as_state_device_mut(&mut self) -> Option<&mut dyn StateDevice> {
        if let AnyDevice::StateDevice(d) = self { Some(d.as_mut()) } else { None }
    }
}

// ─── Adapter module registration ─────────────────────────────────────────────

/// Static descriptor of a single device exported by an adapter crate.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub device_type: DeviceType,
}

/// Trait implemented by each adapter crate's top-level registration type.
pub trait AdapterModule: Send + Sync {
    fn module_name(&self) -> &'static str;
    fn devices(&self) -> &'static [DeviceInfo];
    fn create_device(&self, name: &str) -> Option<AnyDevice>;
}
