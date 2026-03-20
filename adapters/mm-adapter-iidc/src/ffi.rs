//! Minimal hand-written FFI bindings to libdc1394 2.x.
//!
//! Only the subset needed by the MicroManager adapter is declared here.
//! All integer constants match the dc1394 enum values from libdc1394 ≥ 2.2.

#![allow(non_camel_case_types, non_upper_case_globals, dead_code)]

use std::os::raw::{c_int, c_uint};

// ─── Basic types ─────────────────────────────────────────────────────────────

pub type dc1394error_t = c_int;
pub const DC1394_SUCCESS: dc1394error_t = 0;

pub type dc1394bool_t = c_int;

pub type dc1394switch_t = c_uint;
pub const DC1394_OFF: dc1394switch_t = 0;
pub const DC1394_ON: dc1394switch_t = 1;

// ─── Video modes ─────────────────────────────────────────────────────────────

pub type dc1394video_mode_t = c_uint;

pub const DC1394_VIDEO_MODE_160x120_YUV444: dc1394video_mode_t = 64;
pub const DC1394_VIDEO_MODE_320x240_YUV422: dc1394video_mode_t = 65;
pub const DC1394_VIDEO_MODE_640x480_YUV411: dc1394video_mode_t = 66;
pub const DC1394_VIDEO_MODE_640x480_YUV422: dc1394video_mode_t = 67;
pub const DC1394_VIDEO_MODE_640x480_RGB8: dc1394video_mode_t = 68;
pub const DC1394_VIDEO_MODE_640x480_MONO8: dc1394video_mode_t = 69;
pub const DC1394_VIDEO_MODE_640x480_MONO16: dc1394video_mode_t = 70;
pub const DC1394_VIDEO_MODE_800x600_YUV422: dc1394video_mode_t = 71;
pub const DC1394_VIDEO_MODE_800x600_RGB8: dc1394video_mode_t = 72;
pub const DC1394_VIDEO_MODE_800x600_MONO8: dc1394video_mode_t = 73;
pub const DC1394_VIDEO_MODE_1024x768_YUV422: dc1394video_mode_t = 74;
pub const DC1394_VIDEO_MODE_1024x768_RGB8: dc1394video_mode_t = 75;
pub const DC1394_VIDEO_MODE_1024x768_MONO8: dc1394video_mode_t = 76;
pub const DC1394_VIDEO_MODE_800x600_MONO16: dc1394video_mode_t = 77;
pub const DC1394_VIDEO_MODE_1024x768_MONO16: dc1394video_mode_t = 78;
pub const DC1394_VIDEO_MODE_1280x960_YUV422: dc1394video_mode_t = 79;
pub const DC1394_VIDEO_MODE_1280x960_RGB8: dc1394video_mode_t = 80;
pub const DC1394_VIDEO_MODE_1280x960_MONO8: dc1394video_mode_t = 81;
pub const DC1394_VIDEO_MODE_1600x1200_YUV422: dc1394video_mode_t = 82;
pub const DC1394_VIDEO_MODE_1600x1200_RGB8: dc1394video_mode_t = 83;
pub const DC1394_VIDEO_MODE_1600x1200_MONO8: dc1394video_mode_t = 84;
pub const DC1394_VIDEO_MODE_1280x960_MONO16: dc1394video_mode_t = 85;
pub const DC1394_VIDEO_MODE_1600x1200_MONO16: dc1394video_mode_t = 86;
// 87 = EXIF (still image, not supported here)
pub const DC1394_VIDEO_MODE_FORMAT7_0: dc1394video_mode_t = 88;
/// Total number of video mode enum values (64..=95 inclusive).
pub const DC1394_VIDEO_MODE_NUM: usize = 32;

// ─── ISO speed ───────────────────────────────────────────────────────────────

pub type dc1394speed_t = c_uint;
pub const DC1394_ISO_SPEED_100: dc1394speed_t = 0;
pub const DC1394_ISO_SPEED_200: dc1394speed_t = 1;
pub const DC1394_ISO_SPEED_400: dc1394speed_t = 2;
pub const DC1394_ISO_SPEED_800: dc1394speed_t = 3;

// ─── Capture policy ──────────────────────────────────────────────────────────

pub type dc1394capture_policy_t = c_uint;
pub const DC1394_CAPTURE_POLICY_WAIT: dc1394capture_policy_t = 672;
pub const DC1394_CAPTURE_POLICY_POLL: dc1394capture_policy_t = 673;

/// Default capture flags: allocate ISO channel + bandwidth.
pub const DC1394_CAPTURE_FLAGS_DEFAULT: u32 = 0x00000004;

// ─── Features ────────────────────────────────────────────────────────────────

pub type dc1394feature_t = c_uint;
pub const DC1394_FEATURE_SHUTTER: dc1394feature_t = 421;
pub const DC1394_FEATURE_GAIN: dc1394feature_t = 422;

pub type dc1394feature_mode_t = c_uint;
pub const DC1394_FEATURE_MODE_MANUAL: dc1394feature_mode_t = 736;

// ─── Color codings ───────────────────────────────────────────────────────────

pub type dc1394color_coding_t = c_uint;
pub const DC1394_COLOR_CODING_MONO8: dc1394color_coding_t = 352;
pub const DC1394_COLOR_CODING_YUV411: dc1394color_coding_t = 353;
pub const DC1394_COLOR_CODING_YUV422: dc1394color_coding_t = 354;
pub const DC1394_COLOR_CODING_YUV444: dc1394color_coding_t = 355;
pub const DC1394_COLOR_CODING_RGB8: dc1394color_coding_t = 356;
pub const DC1394_COLOR_CODING_MONO16: dc1394color_coding_t = 357;
pub const DC1394_COLOR_CODING_RGB16: dc1394color_coding_t = 358;
pub const DC1394_COLOR_CODING_RAW8: dc1394color_coding_t = 361;
pub const DC1394_COLOR_CODING_RAW16: dc1394color_coding_t = 362;

// ─── Opaque / semi-opaque structs ────────────────────────────────────────────

/// Opaque libdc1394 context.
#[repr(C)]
pub struct dc1394_t {
    _private: [u8; 0],
}

/// Opaque camera handle.  Fields exist in the real struct but we only hold
/// a pointer, so treat it as opaque.
#[repr(C)]
pub struct dc1394camera_t {
    _private: [u8; 0],
}

#[repr(C)]
pub struct dc1394camera_id_t {
    pub guid: u64,
    pub unit: c_int,
}

#[repr(C)]
pub struct dc1394camera_list_t {
    pub num: u32,
    pub ids: *mut dc1394camera_id_t,
}

/// Frame buffer returned by `dc1394_capture_dequeue`.
///
/// Layout matches `dc1394video_frame_t` from libdc1394 ≥ 2.2 on 64-bit
/// platforms.  `#[repr(C)]` ensures the same padding rules as the C struct.
#[repr(C)]
pub struct dc1394video_frame_t {
    pub image: *mut u8,
    pub size: [u32; 2],              // [width, height]
    pub position: [u32; 2],          // [x, y]
    pub color_filter: u32,
    pub stride: u32,                 // bytes per row
    pub color_coding: dc1394color_coding_t,
    pub data_depth: u32,             // bits per component
    pub id: u32,
    // implicit 4-byte pad here (C aligns u64 to 8 bytes)
    pub allocated_image_bytes: u64,
    pub total_bytes: u64,
    pub padding_bytes: u32,
    pub packet_size: u32,
    pub packets_per_frame: u32,
    // implicit 4-byte pad here
    pub timestamp: u64,
    pub frames_behind: u32,
    pub video_mode: dc1394video_mode_t,
    pub allocated_image_bytes_backup: u64,
    pub little_endian: dc1394bool_t,
    pub data_in_padding: dc1394bool_t,
}

/// List of supported video modes returned by `dc1394_video_get_supported_modes`.
#[repr(C)]
pub struct dc1394video_modes_t {
    pub num: u32,
    pub modes: [dc1394video_mode_t; DC1394_VIDEO_MODE_NUM],
}

// ─── Extern functions ────────────────────────────────────────────────────────

extern "C" {
    pub fn dc1394_new() -> *mut dc1394_t;
    pub fn dc1394_free(dc: *mut dc1394_t);

    pub fn dc1394_camera_enumerate(
        dc: *mut dc1394_t,
        list: *mut *mut dc1394camera_list_t,
    ) -> dc1394error_t;
    pub fn dc1394_camera_free_list(list: *mut dc1394camera_list_t);

    pub fn dc1394_camera_new_unit(
        dc: *mut dc1394_t,
        guid: u64,
        unit: c_int,
    ) -> *mut dc1394camera_t;
    pub fn dc1394_camera_free(camera: *mut dc1394camera_t);
    pub fn dc1394_camera_reset(camera: *mut dc1394camera_t) -> dc1394error_t;

    pub fn dc1394_video_get_supported_modes(
        camera: *mut dc1394camera_t,
        modes: *mut dc1394video_modes_t,
    ) -> dc1394error_t;
    pub fn dc1394_video_set_mode(
        camera: *mut dc1394camera_t,
        mode: dc1394video_mode_t,
    ) -> dc1394error_t;
    pub fn dc1394_video_get_mode(
        camera: *mut dc1394camera_t,
        mode: *mut dc1394video_mode_t,
    ) -> dc1394error_t;
    pub fn dc1394_video_set_iso_speed(
        camera: *mut dc1394camera_t,
        speed: dc1394speed_t,
    ) -> dc1394error_t;
    pub fn dc1394_video_set_transmission(
        camera: *mut dc1394camera_t,
        pwr: dc1394switch_t,
    ) -> dc1394error_t;

    pub fn dc1394_capture_setup(
        camera: *mut dc1394camera_t,
        num_dma_buffers: u32,
        flags: u32,
    ) -> dc1394error_t;
    pub fn dc1394_capture_dequeue(
        camera: *mut dc1394camera_t,
        policy: dc1394capture_policy_t,
        frame: *mut *mut dc1394video_frame_t,
    ) -> dc1394error_t;
    pub fn dc1394_capture_enqueue(
        camera: *mut dc1394camera_t,
        frame: *mut dc1394video_frame_t,
    ) -> dc1394error_t;
    pub fn dc1394_capture_stop(camera: *mut dc1394camera_t) -> dc1394error_t;

    pub fn dc1394_get_image_size_from_video_mode(
        camera: *mut dc1394camera_t,
        video_mode: dc1394video_mode_t,
        width: *mut u32,
        height: *mut u32,
    ) -> dc1394error_t;

    pub fn dc1394_feature_set_value(
        camera: *mut dc1394camera_t,
        feature: dc1394feature_t,
        value: u32,
    ) -> dc1394error_t;
    pub fn dc1394_feature_get_value(
        camera: *mut dc1394camera_t,
        feature: dc1394feature_t,
        value: *mut u32,
    ) -> dc1394error_t;
    pub fn dc1394_feature_set_mode(
        camera: *mut dc1394camera_t,
        feature: dc1394feature_t,
        mode: dc1394feature_mode_t,
    ) -> dc1394error_t;
}
