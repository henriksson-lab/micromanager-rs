#![allow(dead_code)]

/// Raw `extern "C"` bindings to the JAI shim (`src/shim.cpp`).
///
/// All types are opaque — only pointers to them are used on the Rust side.
use std::ffi::c_char;
use std::os::raw::{c_int, c_uint};

// ── Opaque types ──────────────────────────────────────────────────────────────

/// Opaque handle to a JaiSystem (wraps PvSystem + device list cache).
#[repr(C)]
pub struct JaiSystem {
    _private: [u8; 0],
}

/// Opaque handle to a connected JaiDevice (wraps PvDevice).
#[repr(C)]
pub struct JaiDevice {
    _private: [u8; 0],
}

/// Opaque handle to an open JaiStream (wraps PvStream).
#[repr(C)]
pub struct JaiStream {
    _private: [u8; 0],
}

/// Opaque handle to a PvBuffer wrapper.
#[repr(C)]
pub struct JaiBuffer {
    _private: [u8; 0],
}

// ── Extern declarations ───────────────────────────────────────────────────────

extern "C" {
    // System
    pub fn jai_system_new() -> *mut JaiSystem;
    pub fn jai_system_free(s: *mut JaiSystem);
    pub fn jai_system_find(s: *mut JaiSystem) -> c_int;
    pub fn jai_system_get_device_id(
        s: *mut JaiSystem,
        idx: c_int,
        buf: *mut c_char,
        len: c_int,
    ) -> c_int;
    pub fn jai_system_get_device_serial(
        s: *mut JaiSystem,
        idx: c_int,
        buf: *mut c_char,
        len: c_int,
    ) -> c_int;

    // Device
    pub fn jai_device_connect(connection_id: *const c_char) -> *mut JaiDevice;
    pub fn jai_device_free(d: *mut JaiDevice);
    pub fn jai_device_get_int(d: *mut JaiDevice, name: *const c_char, out: *mut i64) -> c_int;
    pub fn jai_device_get_int_increment(
        d: *mut JaiDevice,
        name: *const c_char,
        out: *mut i64,
    ) -> c_int;
    pub fn jai_device_set_int(d: *mut JaiDevice, name: *const c_char, value: i64) -> c_int;
    pub fn jai_device_get_float(d: *mut JaiDevice, name: *const c_char, out: *mut f64) -> c_int;
    pub fn jai_device_set_float(d: *mut JaiDevice, name: *const c_char, value: f64) -> c_int;
    pub fn jai_device_get_string(
        d: *mut JaiDevice,
        name: *const c_char,
        buf: *mut c_char,
        len: c_int,
    ) -> c_int;
    pub fn jai_device_get_enum(
        d: *mut JaiDevice,
        name: *const c_char,
        buf: *mut c_char,
        len: c_int,
    ) -> c_int;
    pub fn jai_device_get_enum_entries(
        d: *mut JaiDevice,
        name: *const c_char,
        buf: *mut c_char,
        len: c_int,
    ) -> c_int;
    pub fn jai_device_set_enum(
        d: *mut JaiDevice,
        name: *const c_char,
        value: *const c_char,
    ) -> c_int;
    pub fn jai_device_execute(d: *mut JaiDevice, name: *const c_char) -> c_int;
    pub fn jai_device_payload_size(d: *mut JaiDevice) -> u64;
    pub fn jai_device_stream_enable(d: *mut JaiDevice) -> c_int;
    pub fn jai_device_stream_disable(d: *mut JaiDevice) -> c_int;
    pub fn jai_device_get_connection_id(d: *mut JaiDevice, buf: *mut c_char, len: c_int) -> c_int;

    // Stream
    pub fn jai_stream_open(connection_id: *const c_char) -> *mut JaiStream;
    pub fn jai_stream_free(s: *mut JaiStream);
    pub fn jai_stream_queue(s: *mut JaiStream, buf: *mut JaiBuffer) -> c_int;
    pub fn jai_stream_retrieve(s: *mut JaiStream, timeout_ms: c_uint) -> *mut JaiBuffer;
    pub fn jai_stream_requeue(s: *mut JaiStream, buf: *mut JaiBuffer) -> c_int;
    pub fn jai_stream_abort(s: *mut JaiStream) -> c_int;

    // Buffer
    pub fn jai_buffer_alloc(size: u64) -> *mut JaiBuffer;
    pub fn jai_buffer_free(buf: *mut JaiBuffer);
    pub fn jai_buffer_width(buf: *mut JaiBuffer) -> c_uint;
    pub fn jai_buffer_height(buf: *mut JaiBuffer) -> c_uint;
    pub fn jai_buffer_bits_per_pixel(buf: *mut JaiBuffer) -> c_uint;
    pub fn jai_buffer_bits_per_component(buf: *mut JaiBuffer) -> c_uint;
    pub fn jai_buffer_padding_x(buf: *mut JaiBuffer) -> c_uint;
    pub fn jai_buffer_is_color(buf: *mut JaiBuffer) -> c_int;
    pub fn jai_buffer_data(buf: *mut JaiBuffer) -> *const u8;
    pub fn jai_buffer_data_size(buf: *mut JaiBuffer) -> u64;
}
