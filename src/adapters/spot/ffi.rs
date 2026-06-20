#![allow(dead_code)]

//! Raw `extern "C"` bindings to the SpotCam shim (`src/shim.c`).

use std::ffi::c_char;
use std::os::raw::{c_double, c_float, c_int};

/// Opaque camera context managed by the shim.
#[repr(C)]
pub struct SpotCtx {
    _private: [u8; 0],
}

extern "C" {
    // Enumeration (no ctx needed — global API)
    pub fn spot_find_devices() -> c_int;
    pub fn spot_get_device_name(idx: c_int, buf: *mut c_char, len: c_int) -> c_int;
    pub fn spot_get_serial_number(idx: c_int, buf: *mut c_char, len: c_int) -> c_int;

    // Open / close
    pub fn spot_open(device_index: c_int) -> *mut SpotCtx;
    pub fn spot_close(ctx: *mut SpotCtx);

    // Image dimensions
    pub fn spot_get_image_width(ctx: *mut SpotCtx) -> c_int;
    pub fn spot_get_image_height(ctx: *mut SpotCtx) -> c_int;
    pub fn spot_get_bit_depth(ctx: *mut SpotCtx) -> c_int;

    // Exposure (milliseconds)
    pub fn spot_get_exposure_ms(ctx: *mut SpotCtx) -> c_double;
    pub fn spot_set_exposure_ms(ctx: *mut SpotCtx, ms: c_double) -> c_int;

    // Gain
    pub fn spot_get_gain(ctx: *mut SpotCtx) -> c_int;
    pub fn spot_set_gain(ctx: *mut SpotCtx, gain: c_int) -> c_int;
    pub fn spot_get_gain_max(ctx: *mut SpotCtx) -> c_int;

    // Binning
    pub fn spot_get_binning(ctx: *mut SpotCtx) -> c_int;
    pub fn spot_set_binning(ctx: *mut SpotCtx, bin: c_int) -> c_int;

    // Temperature (Celsius)
    pub fn spot_get_temperature_c(ctx: *mut SpotCtx) -> c_float;

    // ROI
    pub fn spot_set_roi(ctx: *mut SpotCtx, x: c_int, y: c_int, w: c_int, h: c_int) -> c_int;
    pub fn spot_clear_roi(ctx: *mut SpotCtx) -> c_int;

    // Snap (blocking)
    pub fn spot_snap(ctx: *mut SpotCtx, timeout_ms: c_int) -> c_int;
    pub fn spot_get_frame_ptr(ctx: *mut SpotCtx) -> *const u8;
    pub fn spot_get_frame_bytes(ctx: *mut SpotCtx) -> c_int;
}
