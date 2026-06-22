#![allow(dead_code)]

/// Raw `extern "C"` bindings to the TWAIN shim (`src/shim.c`).
use std::ffi::c_char;
use std::os::raw::c_int;

/// Opaque per-source context managed by the shim.
#[repr(C)]
pub struct TwainCtx {
    _private: [u8; 0],
}

extern "C" {
    // DSM lifecycle (global)
    pub fn twain_init() -> c_int;
    pub fn twain_close_dsm();

    // Source enumeration (newline-separated ProductNames)
    pub fn twain_find_sources(buf: *mut c_char, len: c_int) -> c_int;

    // Open / close per-source
    pub fn twain_open(source_name: *const c_char) -> *mut TwainCtx;
    pub fn twain_close(ctx: *mut TwainCtx);

    // Property getters
    pub fn twain_get_image_width(ctx: *mut TwainCtx) -> c_int;
    pub fn twain_get_image_height(ctx: *mut TwainCtx) -> c_int;
    pub fn twain_get_bytes_per_pixel(ctx: *mut TwainCtx) -> c_int;
    pub fn twain_get_bit_depth(ctx: *mut TwainCtx) -> c_int;
    pub fn twain_get_source_name(ctx: *mut TwainCtx) -> *const c_char;

    // Snap (blocking)
    pub fn twain_snap(ctx: *mut TwainCtx, timeout_ms: c_int) -> c_int;
    pub fn twain_get_frame_ptr(ctx: *mut TwainCtx) -> *const u8;
    pub fn twain_get_frame_bytes(ctx: *mut TwainCtx) -> c_int;
}
