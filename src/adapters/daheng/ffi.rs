//! Raw FFI bindings to the Daheng Galaxy C API (GxIAPI).
//!
//! Only the subset needed by the Camera adapter is declared here.
//! The full API has hundreds of functions; we bind the minimum viable set.
#![allow(non_camel_case_types, dead_code)]

use std::os::raw::{c_char, c_double, c_int, c_uint, c_ulonglong, c_void};

/// Opaque device handle.
pub type GX_DEV_HANDLE = *mut c_void;

// ─── Status codes ────────────────────────────────────────────────────────────

pub const GX_STATUS_SUCCESS: c_int = 0;
pub const GX_STATUS_ERROR: c_int = -1;
pub const GX_STATUS_NOT_FOUND_DEVICE: c_int = -3;
pub const GX_STATUS_INVALID_PARAMETER: c_int = -5;
pub const GX_STATUS_INVALID_HANDLE: c_int = -6;
pub const GX_STATUS_TIMEOUT: c_int = -14;

// ─── Feature IDs ─────────────────────────────────────────────────────────────

// Integer features
pub const GX_INT_SENSOR_WIDTH: c_int = 0x100003E8;
pub const GX_INT_SENSOR_HEIGHT: c_int = 0x100003E9;
pub const GX_INT_WIDTH_MAX: c_int = 0x100003EA;
pub const GX_INT_HEIGHT_MAX: c_int = 0x100003EB;
pub const GX_INT_WIDTH: c_int = 0x100003EE;
pub const GX_INT_HEIGHT: c_int = 0x100003EF;
pub const GX_INT_OFFSET_X: c_int = 0x100003EC;
pub const GX_INT_OFFSET_Y: c_int = 0x100003ED;
pub const GX_INT_BINNING_HORIZONTAL: c_int = 0x100003F0;
pub const GX_INT_BINNING_VERTICAL: c_int = 0x100003F1;

// Float features
pub const GX_FLOAT_EXPOSURE_TIME: c_int = 0x20000BC1;
pub const GX_FLOAT_TRIGGER_FILTER_RAISING: c_int = 0x20000BC3;
pub const GX_FLOAT_TRIGGER_DELAY: c_int = 0x20000BC8;
pub const GX_FLOAT_ACQUISITION_FRAME_RATE: c_int = 0x20000BCF;
pub const GX_FLOAT_GAIN: c_int = 0x20001393;

// String features
pub const GX_STRING_DEVICE_SERIAL_NUMBER: c_int = 0x50000004;

// Enum features
pub const GX_ENUM_PIXEL_FORMAT: c_int = 0x300003F6;
pub const GX_ENUM_TRIGGER_MODE: c_int = 0x30000BBD;
pub const GX_ENUM_TRIGGER_ACTIVATION: c_int = 0x30000BBF;
pub const GX_ENUM_TRIGGER_SOURCE: c_int = 0x30000BC5;
pub const GX_ENUM_TRIGGER_SELECTOR: c_int = 0x30000BC7;
pub const GX_ENUM_ACQUISITION_FRAME_RATE_MODE: c_int = 0x30000BCE;
pub const GX_ENUM_USER_OUTPUT_SELECTOR: c_int = 0x30000FA0;
pub const GX_ENUM_LINE_SELECTOR: c_int = 0x30000FA1;
pub const GX_ENUM_LINE_MODE: c_int = 0x30000FA2;
pub const GX_ENUM_LINE_SOURCE: c_int = 0x30000FA3;

// Command features
pub const GX_COMMAND_TRIGGER_SOFTWARE: c_int = 0x70000BBE;

// Trigger mode values
pub const GX_TRIGGER_MODE_OFF: i64 = 0;
pub const GX_TRIGGER_MODE_ON: i64 = 1;

// Trigger source values
pub const GX_TRIGGER_SOURCE_SOFTWARE: i64 = 0;
pub const GX_TRIGGER_SOURCE_LINE0: i64 = 1;
pub const GX_TRIGGER_SOURCE_LINE1: i64 = 2;
pub const GX_TRIGGER_SOURCE_LINE2: i64 = 3;
pub const GX_TRIGGER_SOURCE_LINE3: i64 = 4;

// Trigger activation values
pub const GX_TRIGGER_ACTIVATION_FALLING_EDGE: i64 = 0;
pub const GX_TRIGGER_ACTIVATION_RISING_EDGE: i64 = 1;

// Trigger selector values
pub const GX_TRIGGER_SELECTOR_FRAME_START: i64 = 1;

// Acquisition frame rate mode values
pub const GX_ACQUISITION_FRAME_RATE_MODE_OFF: i64 = 0;
pub const GX_ACQUISITION_FRAME_RATE_MODE_ON: i64 = 1;

// User output selector values
pub const GX_USER_OUTPUT_SELECTOR_OUTPUT0: i64 = 1;
pub const GX_USER_OUTPUT_SELECTOR_OUTPUT1: i64 = 2;
pub const GX_USER_OUTPUT_SELECTOR_OUTPUT2: i64 = 4;

// Line selector values
pub const GX_LINE_SELECTOR_LINE0: i64 = 0;
pub const GX_LINE_SELECTOR_LINE1: i64 = 1;

// Line mode values
pub const GX_LINE_MODE_INPUT: i64 = 0;
pub const GX_LINE_MODE_OUTPUT: i64 = 1;

// Line source values
pub const GX_LINE_SOURCE_OFF: i64 = 0;
pub const GX_LINE_SOURCE_USER_OUTPUT0: i64 = 2;
pub const GX_LINE_SOURCE_EXPOSURE_ACTIVE: i64 = 5;

// ─── Pixel format values ─────────────────────────────────────────────────────

pub const GX_PIXEL_FORMAT_MONO8: i64 = 0x01080001;
pub const GX_PIXEL_FORMAT_MONO10: i64 = 0x01100003;
pub const GX_PIXEL_FORMAT_MONO12: i64 = 0x01100005;
pub const GX_PIXEL_FORMAT_MONO16: i64 = 0x01100007;
pub const GX_PIXEL_FORMAT_MONO14: i64 = 0x01100025;
pub const GX_PIXEL_FORMAT_BAYER_GR8: i64 = 0x01080008;
pub const GX_PIXEL_FORMAT_BAYER_RG8: i64 = 0x01080009;
pub const GX_PIXEL_FORMAT_BAYER_GB8: i64 = 0x0108000A;
pub const GX_PIXEL_FORMAT_BAYER_BG8: i64 = 0x0108000B;
pub const GX_PIXEL_FORMAT_BAYER_GR10: i64 = 0x0110000C;
pub const GX_PIXEL_FORMAT_BAYER_RG10: i64 = 0x0110000D;
pub const GX_PIXEL_FORMAT_BAYER_GB10: i64 = 0x0110000E;
pub const GX_PIXEL_FORMAT_BAYER_BG10: i64 = 0x0110000F;
pub const GX_PIXEL_FORMAT_BAYER_GR12: i64 = 0x01100010;
pub const GX_PIXEL_FORMAT_BAYER_RG12: i64 = 0x01100011;
pub const GX_PIXEL_FORMAT_BAYER_GB12: i64 = 0x01100012;
pub const GX_PIXEL_FORMAT_BAYER_BG12: i64 = 0x01100013;
pub const GX_PIXEL_FORMAT_BAYER_GR16: i64 = 0x0110002E;
pub const GX_PIXEL_FORMAT_BAYER_RG16: i64 = 0x0110002F;
pub const GX_PIXEL_FORMAT_BAYER_GB16: i64 = 0x01100030;
pub const GX_PIXEL_FORMAT_BAYER_BG16: i64 = 0x01100031;

// ─── Open mode ───────────────────────────────────────────────────────────────

pub const GX_OPEN_SN: c_uint = 0;
pub const GX_OPEN_INDEX: c_uint = 3;

pub const GX_ACCESS_EXCLUSIVE: c_uint = 4;

// ─── Structures ──────────────────────────────────────────────────────────────

#[repr(C)]
pub struct GxOpenParam {
    pub content: *const c_char,
    pub open_mode: c_uint,
    pub access_mode: c_uint,
}

#[repr(C)]
pub struct GxFrameData {
    pub status: c_int,
    pub image_buf: *mut c_void,
    pub width: c_int,
    pub height: c_int,
    pub pixel_format: c_int,
    pub image_size: c_int,
    pub frame_id: c_ulonglong,
    pub timestamp: c_ulonglong,
    pub offset_x: c_int,
    pub offset_y: c_int,
    pub reserved: [c_int; 1],
}

impl Default for GxFrameData {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct GxIntRange {
    pub min: i64,
    pub max: i64,
    pub inc: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GxDeviceBaseInfo {
    pub vendor_name: [c_char; 32],
    pub model_name: [c_char; 32],
    pub serial_number: [c_char; 32],
    pub display_name: [c_char; 132],
    pub device_id: [c_char; 68],
    pub user_id: [c_char; 68],
    pub access_status: c_int,
    pub device_class: c_int,
    pub reserved: [c_char; 300],
}

impl Default for GxDeviceBaseInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

// ─── C API functions ─────────────────────────────────────────────────────────

#[link(name = "gxiapi")]
extern "C" {
    pub fn GXInitLib() -> c_int;
    pub fn GXCloseLib() -> c_int;

    pub fn GXUpdateDeviceList(device_num: *mut c_uint, timeout: c_uint) -> c_int;
    pub fn GXGetAllDeviceBaseInfo(
        device_info: *mut GxDeviceBaseInfo,
        buffer_size: *mut usize,
    ) -> c_int;
    pub fn GXOpenDevice(param: *const GxOpenParam, handle: *mut GX_DEV_HANDLE) -> c_int;
    pub fn GXCloseDevice(handle: GX_DEV_HANDLE) -> c_int;

    pub fn GXGetString(
        handle: GX_DEV_HANDLE,
        feature_id: c_int,
        content: *mut c_char,
        size: *mut usize,
    ) -> c_int;
    pub fn GXGetFloat(handle: GX_DEV_HANDLE, feature_id: c_int, value: *mut c_double) -> c_int;
    pub fn GXSetFloat(handle: GX_DEV_HANDLE, feature_id: c_int, value: c_double) -> c_int;
    pub fn GXGetIntRange(handle: GX_DEV_HANDLE, feature_id: c_int, range: *mut GxIntRange)
        -> c_int;
    pub fn GXGetInt(handle: GX_DEV_HANDLE, feature_id: c_int, value: *mut i64) -> c_int;
    pub fn GXSetInt(handle: GX_DEV_HANDLE, feature_id: c_int, value: i64) -> c_int;
    pub fn GXGetEnum(handle: GX_DEV_HANDLE, feature_id: c_int, value: *mut i64) -> c_int;
    pub fn GXSetEnum(handle: GX_DEV_HANDLE, feature_id: c_int, value: i64) -> c_int;
    pub fn GXSendCommand(handle: GX_DEV_HANDLE, feature_id: c_int) -> c_int;

    pub fn GXStreamOn(handle: GX_DEV_HANDLE) -> c_int;
    pub fn GXStreamOff(handle: GX_DEV_HANDLE) -> c_int;
    pub fn GXGetImage(
        handle: GX_DEV_HANDLE,
        frame_data: *mut GxFrameData,
        timeout: c_uint,
    ) -> c_int;
}
