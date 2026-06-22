//! Raw `extern "C"` bindings to the TSI shim (`src/shim.c`).

#![allow(dead_code)]

use std::ffi::c_char;
use std::os::raw::{c_double, c_int, c_longlong};

/// Opaque camera context managed by the shim.
#[repr(C)]
pub struct TsiCtx {
    _private: [u8; 0],
}

extern "C" {
    // SDK lifecycle
    pub fn tsi_sdk_open() -> c_int;
    pub fn tsi_sdk_close();

    // Discovery: fills `buf` with space-separated camera IDs; returns count or -1
    pub fn tsi_discover_cameras(buf: *mut c_char, len: c_int) -> c_int;

    // Open / close
    pub fn tsi_open_camera(camera_id: *const c_char) -> *mut TsiCtx;
    pub fn tsi_close_camera(ctx: *mut TsiCtx);

    // Sensor / image properties
    pub fn tsi_get_image_width(ctx: *mut TsiCtx) -> c_int;
    pub fn tsi_get_image_height(ctx: *mut TsiCtx) -> c_int;
    pub fn tsi_get_sensor_width(ctx: *mut TsiCtx) -> c_int;
    pub fn tsi_get_sensor_height(ctx: *mut TsiCtx) -> c_int;
    pub fn tsi_get_bit_depth(ctx: *mut TsiCtx) -> c_int;
    pub fn tsi_get_bytes_per_pixel(ctx: *mut TsiCtx) -> c_int;
    /// 0 = monochrome, 1 = Bayer, 2 = polarized
    pub fn tsi_get_sensor_type(ctx: *mut TsiCtx) -> c_int;
    pub fn tsi_get_serial_number(ctx: *mut TsiCtx, buf: *mut c_char, len: c_int) -> c_int;
    pub fn tsi_get_firmware_version(ctx: *mut TsiCtx, buf: *mut c_char, len: c_int) -> c_int;

    // Exposure (microseconds)
    pub fn tsi_get_exposure_us(ctx: *mut TsiCtx) -> c_longlong;
    pub fn tsi_set_exposure_us(ctx: *mut TsiCtx, us: c_longlong) -> c_int;
    pub fn tsi_get_exposure_range_us(
        ctx: *mut TsiCtx,
        min_out: *mut c_longlong,
        max_out: *mut c_longlong,
    ) -> c_int;

    // Trigger control.  Mode codes: 1 = Software, 2 = HardwareEdge,
    // 3 = HardwareDuration.  Polarity codes: 1 = Positive, 0 = Negative.
    pub fn tsi_is_operation_mode_supported(ctx: *mut TsiCtx, mode: c_int) -> c_int;
    pub fn tsi_get_operation_mode(ctx: *mut TsiCtx) -> c_int;
    pub fn tsi_set_operation_mode(ctx: *mut TsiCtx, mode: c_int) -> c_int;
    pub fn tsi_get_trigger_polarity(ctx: *mut TsiCtx) -> c_int;
    pub fn tsi_set_trigger_polarity(ctx: *mut TsiCtx, polarity: c_int) -> c_int;

    // EEP / hot-pixel correction / gain.
    pub fn tsi_is_eep_supported(ctx: *mut TsiCtx) -> c_int;
    pub fn tsi_get_eep_enabled(ctx: *mut TsiCtx) -> c_int;
    pub fn tsi_set_eep_enabled(ctx: *mut TsiCtx, enabled: c_int) -> c_int;
    pub fn tsi_get_hot_pixel_threshold_range(
        ctx: *mut TsiCtx,
        min_out: *mut c_int,
        max_out: *mut c_int,
    ) -> c_int;
    pub fn tsi_get_hot_pixel_enabled(ctx: *mut TsiCtx) -> c_int;
    pub fn tsi_set_hot_pixel_enabled(ctx: *mut TsiCtx, enabled: c_int) -> c_int;
    pub fn tsi_get_hot_pixel_threshold(ctx: *mut TsiCtx) -> c_int;
    pub fn tsi_set_hot_pixel_threshold(ctx: *mut TsiCtx, threshold: c_int) -> c_int;
    pub fn tsi_get_gain_range(ctx: *mut TsiCtx, min_out: *mut c_int, max_out: *mut c_int) -> c_int;
    pub fn tsi_convert_gain_to_db(
        ctx: *mut TsiCtx,
        gain_index: c_int,
        gain_db: *mut c_double,
    ) -> c_int;
    pub fn tsi_get_gain_db(ctx: *mut TsiCtx, gain_db: *mut c_double) -> c_int;
    pub fn tsi_set_gain_db(ctx: *mut TsiCtx, gain_db: c_double) -> c_int;

    // ROI
    pub fn tsi_set_roi(ctx: *mut TsiCtx, x: c_int, y: c_int, w: c_int, h: c_int) -> c_int;
    pub fn tsi_clear_roi(ctx: *mut TsiCtx) -> c_int;
    pub fn tsi_get_roi(
        ctx: *mut TsiCtx,
        x: *mut c_int,
        y: *mut c_int,
        w: *mut c_int,
        h: *mut c_int,
    ) -> c_int;

    // Binning
    pub fn tsi_get_binx(ctx: *mut TsiCtx) -> c_int;
    pub fn tsi_get_biny(ctx: *mut TsiCtx) -> c_int;
    pub fn tsi_set_binx(ctx: *mut TsiCtx, val: c_int) -> c_int;
    pub fn tsi_set_biny(ctx: *mut TsiCtx, val: c_int) -> c_int;
    pub fn tsi_get_binx_range(ctx: *mut TsiCtx, min_out: *mut c_int, max_out: *mut c_int) -> c_int;
    pub fn tsi_get_biny_range(ctx: *mut TsiCtx, min_out: *mut c_int, max_out: *mut c_int) -> c_int;

    // Snap (blocking)
    pub fn tsi_snap(ctx: *mut TsiCtx, timeout_ms: c_int) -> c_int;
    pub fn tsi_get_frame_ptr(ctx: *mut TsiCtx) -> *const u16;
    pub fn tsi_get_frame_bytes(ctx: *mut TsiCtx) -> c_int;

    // Continuous
    pub fn tsi_start_cont(ctx: *mut TsiCtx, frame_count: c_int) -> c_int;
    pub fn tsi_get_next_frame(ctx: *mut TsiCtx, timeout_ms: c_int) -> c_int;
    pub fn tsi_stop_cont(ctx: *mut TsiCtx) -> c_int;
}
