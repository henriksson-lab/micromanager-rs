/// Raw `extern "C"` bindings to the Andor SDK3 shim (`src/shim.c`).

#![allow(dead_code)]

use std::ffi::c_char;
use std::os::raw::{c_double, c_int};

/// Opaque camera context managed by the shim.
#[repr(C)]
pub struct Andor3Ctx {
    _private: [u8; 0],
}

extern "C" {
    // SDK lifecycle (global)
    pub fn andor3_sdk_open() -> c_int;
    pub fn andor3_sdk_close();
    pub fn andor3_get_device_count() -> c_int;

    // Open / close
    pub fn andor3_open(camera_index: c_int) -> *mut Andor3Ctx;
    pub fn andor3_close(ctx: *mut Andor3Ctx);

    // Geometry
    pub fn andor3_get_image_width(ctx: *mut Andor3Ctx) -> c_int;
    pub fn andor3_get_image_height(ctx: *mut Andor3Ctx) -> c_int;
    pub fn andor3_get_bytes_per_pixel(ctx: *mut Andor3Ctx) -> c_int;
    pub fn andor3_get_bit_depth(ctx: *mut Andor3Ctx) -> c_int;
    pub fn andor3_get_sensor_width(ctx: *mut Andor3Ctx) -> c_int;
    pub fn andor3_get_sensor_height(ctx: *mut Andor3Ctx) -> c_int;

    // AOI
    pub fn andor3_set_aoi(ctx: *mut Andor3Ctx, left: c_int, top: c_int, w: c_int, h: c_int) -> c_int;
    pub fn andor3_clear_aoi(ctx: *mut Andor3Ctx) -> c_int;
    pub fn andor3_get_aoi(ctx: *mut Andor3Ctx, left: *mut c_int, top: *mut c_int, w: *mut c_int, h: *mut c_int) -> c_int;

    // Exposure (seconds)
    pub fn andor3_get_exposure_s(ctx: *mut Andor3Ctx) -> c_double;
    pub fn andor3_set_exposure_s(ctx: *mut Andor3Ctx, seconds: c_double) -> c_int;

    // Temperature
    pub fn andor3_get_temperature(ctx: *mut Andor3Ctx) -> c_double;

    // Generic feature access (narrow strings)
    pub fn andor3_get_string(ctx: *mut Andor3Ctx, feature: *const c_char, buf: *mut c_char, len: c_int) -> c_int;
    pub fn andor3_get_enum(ctx: *mut Andor3Ctx, feature: *const c_char, buf: *mut c_char, len: c_int) -> c_int;
    pub fn andor3_set_enum(ctx: *mut Andor3Ctx, feature: *const c_char, value: *const c_char) -> c_int;
    pub fn andor3_enum_values(ctx: *mut Andor3Ctx, feature: *const c_char, buf: *mut c_char, len: c_int) -> c_int;

    // Snap (blocking, single frame)
    pub fn andor3_snap(ctx: *mut Andor3Ctx, timeout_ms: c_int) -> c_int;
    pub fn andor3_get_frame_ptr(ctx: *mut Andor3Ctx) -> *const u8;
    pub fn andor3_get_frame_bytes(ctx: *mut Andor3Ctx) -> c_int;

    // Continuous acquisition
    pub fn andor3_start_cont(ctx: *mut Andor3Ctx) -> c_int;
    pub fn andor3_get_next_frame(ctx: *mut Andor3Ctx, timeout_ms: c_int) -> c_int;
    pub fn andor3_stop_cont(ctx: *mut Andor3Ctx) -> c_int;
}
