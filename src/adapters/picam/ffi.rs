#![allow(dead_code)]

/// Raw `extern "C"` bindings to the PVCAM shim (`src/shim.c`).
use std::ffi::c_char;
use std::os::raw::{c_double, c_int, c_uint};

/// Opaque camera context (wraps PVCAM camera handle + internal buffers).
#[repr(C)]
pub struct PvcamCtx {
    _private: [u8; 0],
}

extern "C" {
    // Library lifecycle
    pub fn pvcam_init() -> c_int;
    pub fn pvcam_uninit();

    // Enumeration
    pub fn pvcam_get_camera_count() -> c_int;
    pub fn pvcam_get_camera_name(idx: c_int, buf: *mut c_char, len: c_int) -> c_int;

    // Open / close
    pub fn pvcam_open(name: *const c_char) -> *mut PvcamCtx;
    pub fn pvcam_close(ctx: *mut PvcamCtx);

    // Sensor / image info
    pub fn pvcam_get_sensor_width(ctx: *mut PvcamCtx) -> u16;
    pub fn pvcam_get_sensor_height(ctx: *mut PvcamCtx) -> u16;
    pub fn pvcam_get_image_width(ctx: *mut PvcamCtx) -> u16;
    pub fn pvcam_get_image_height(ctx: *mut PvcamCtx) -> u16;
    pub fn pvcam_get_bit_depth(ctx: *mut PvcamCtx) -> c_int;
    pub fn pvcam_get_serial_number(ctx: *mut PvcamCtx, buf: *mut c_char, len: c_int) -> c_int;
    pub fn pvcam_get_chip_name(ctx: *mut PvcamCtx, buf: *mut c_char, len: c_int) -> c_int;

    // Gain
    pub fn pvcam_get_gain_index(ctx: *mut PvcamCtx) -> c_int;
    pub fn pvcam_get_gain_max(ctx: *mut PvcamCtx) -> c_int;
    pub fn pvcam_set_gain_index(ctx: *mut PvcamCtx, idx: c_int) -> c_int;

    // Temperature
    pub fn pvcam_get_temperature(ctx: *mut PvcamCtx) -> c_double;
    pub fn pvcam_get_temp_setpoint(ctx: *mut PvcamCtx) -> c_double;
    pub fn pvcam_set_temp_setpoint(ctx: *mut PvcamCtx, celsius: c_double) -> c_int;

    // ROI
    pub fn pvcam_set_roi(ctx: *mut PvcamCtx, x: u16, y: u16, w: u16, h: u16, xbin: u16, ybin: u16);
    pub fn pvcam_clear_roi(ctx: *mut PvcamCtx);

    // Snap
    pub fn pvcam_snap(ctx: *mut PvcamCtx, exp_ms: c_uint, timeout_ms: c_uint) -> c_int;
    pub fn pvcam_get_snap_frame(ctx: *mut PvcamCtx) -> *const u8;
    pub fn pvcam_get_frame_size(ctx: *mut PvcamCtx) -> c_uint;

    // Continuous
    pub fn pvcam_start_cont(ctx: *mut PvcamCtx, exp_ms: c_uint, num_frames: c_int) -> c_int;
    pub fn pvcam_get_frame_cont(ctx: *mut PvcamCtx, frame_out: *mut *const u8) -> c_int;
    pub fn pvcam_release_frame_cont(ctx: *mut PvcamCtx) -> c_int;
    pub fn pvcam_stop_cont(ctx: *mut PvcamCtx) -> c_int;

    // Error
    pub fn pvcam_get_error_message(buf: *mut c_char, len: c_int) -> c_int;
}
