/**
 * shim.c — Thin C wrapper around the Thorlabs Scientific Camera SDK3 C API.
 *
 * Exposes a simplified opaque-context API suitable for Rust FFI.  The SDK3
 * API is callback-based; this shim uses a volatile flag + polling sleep to
 * provide synchronous snap semantics without platform-specific event objects.
 *
 * Exposure time unit: milliseconds in the public API, microseconds internally
 * (TSI SDK3 uses µs).
 */

/* ── Platform includes ────────────────────────────────────────────────────── */

#ifdef _WIN32
#  define WIN32_LEAN_AND_MEAN
#  include <windows.h>
   static void shim_sleep_ms(int ms) { Sleep((DWORD)ms); }
#else
#  include <unistd.h>
   static void shim_sleep_ms(int ms) { usleep((useconds_t)(ms) * 1000u); }
#endif

#include "tl_camera_sdk.h"

#include <stdlib.h>
#include <string.h>
#include <stdint.h>

static int sdk_open_count = 0;

/* ── Context struct ───────────────────────────────────────────────────────── */

typedef struct TsiCtx {
    void*     handle;           /* TSI SDK camera handle */

    /* Cached sensor properties (read once on open). */
    int       sensor_type;      /* 0 = mono, 1 = bayer, 2 = polarized */
    int       bit_depth;
    int       bytes_per_pixel;  /* always 2 for SDK3 uint16 output */
    int       sensor_width;     /* full sensor, no binning */
    int       sensor_height;

    /* Current image dimensions (after ROI + binning). */
    int       img_width;
    int       img_height;

    /* Internal frame buffer (owned). */
    uint16_t* image_buf;
    size_t    image_buf_bytes;
    int       frame_bytes;      /* bytes in the last delivered frame */

    /* Synchronisation: callback increments frame_ready; waiter polls. */
    volatile int frame_ready;

    /* State flags. */
    int       in_sequence;      /* 1 = continuous mode active */
    int       operation_mode;   /* 1 = software, 2 = hardware edge, 3 = bulb */
} TsiCtx;

/* ── Frame-available callback ─────────────────────────────────────────────── */

static void frame_callback(
        void*           sender,
        unsigned short* image_buffer,
        int             frame_count,
        unsigned char*  metadata,
        int             metadata_size,
        void*           context)
{
    TsiCtx* ctx = (TsiCtx*)context;
    if (!ctx || !image_buffer) return;

    /* Compute output frame size from current dimensions. */
    int pixels = ctx->img_width * ctx->img_height;
    int fb = pixels * ctx->bytes_per_pixel;
    if (pixels <= 0 || fb <= 0) return;

    /* (Re-)allocate internal buffer if needed. */
    if (fb > (int)ctx->image_buf_bytes) {
        free(ctx->image_buf);
        ctx->image_buf = (uint16_t*)malloc((size_t)fb);
        if (!ctx->image_buf) { ctx->image_buf_bytes = 0; return; }
        ctx->image_buf_bytes = (size_t)fb;
    }

    if (ctx->sensor_type == 1) {
        uint8_t* dst = (uint8_t*)ctx->image_buf;
        for (int i = 0; i < pixels; i++) {
            uint8_t v = (uint8_t)(image_buffer[i] >> 8);
            dst[i * 4 + 0] = v;
            dst[i * 4 + 1] = v;
            dst[i * 4 + 2] = v;
            dst[i * 4 + 3] = 255;
        }
    } else {
        memcpy(ctx->image_buf, image_buffer, (size_t)fb);
    }
    ctx->frame_bytes = fb;
    ctx->frame_ready++;     /* signal to waiter — must be last write */
}

/* ── Helpers ──────────────────────────────────────────────────────────────── */

static int refresh_image_dims(TsiCtx* ctx) {
    if (!ctx || !ctx->handle) return -1;
    if (tl_camera_get_image_width(ctx->handle, &ctx->img_width)) return -1;
    if (tl_camera_get_image_height(ctx->handle, &ctx->img_height)) return -1;
    return 0;
}

static int wait_for_frame(TsiCtx* ctx, int timeout_ms) {
    int elapsed = 0;
    while (ctx->frame_ready <= 0) {
        if (elapsed >= timeout_ms) return -1;
        shim_sleep_ms(1);
        elapsed++;
    }
    ctx->frame_ready--;
    return 0;
}

static int sdk_operation_mode_from_code(int mode) {
    switch (mode) {
        case 1: return TL_CAMERA_OPERATION_MODE_SOFTWARE_TRIGGERED;
        case 2: return TL_CAMERA_OPERATION_MODE_HARDWARE_TRIGGERED;
        case 3: return TL_CAMERA_OPERATION_MODE_BULB;
        default: return -1;
    }
}

static int operation_mode_code_from_sdk(int mode) {
    switch (mode) {
        case TL_CAMERA_OPERATION_MODE_SOFTWARE_TRIGGERED: return 1;
        case TL_CAMERA_OPERATION_MODE_HARDWARE_TRIGGERED: return 2;
        case TL_CAMERA_OPERATION_MODE_BULB: return 3;
        default: return -1;
    }
}

/* ── SDK lifecycle ────────────────────────────────────────────────────────── */

int tsi_sdk_open(void) {
    if (sdk_open_count > 0) {
        sdk_open_count++;
        return 0;
    }

    if (tl_camera_sdk_dll_initialize() != 0) return -1;
    if (tl_camera_open_sdk() != 0) {
        tl_camera_sdk_dll_uninitialize();
        return -1;
    }
    sdk_open_count = 1;
    return 0;
}

void tsi_sdk_close(void) {
    if (sdk_open_count <= 0) {
        return;
    }
    sdk_open_count--;
    if (sdk_open_count > 0) {
        return;
    }

    tl_camera_close_sdk();
    tl_camera_sdk_dll_uninitialize();
}

/* ── Camera discovery ─────────────────────────────────────────────────────── */

/**
 * Fill `buf` with a space-separated list of discovered camera ID strings.
 * Returns number of cameras found, or -1 on error.
 */
int tsi_discover_cameras(char* buf, int len) {
    if (!buf || len <= 0) return -1;
    buf[0] = '\0';
    if (tl_camera_discover_available_cameras(buf, len) != 0) return -1;

    /* Count space-separated tokens. */
    if (buf[0] == '\0') return 0;
    int count = 1;
    for (int i = 0; buf[i]; i++) {
        if (buf[i] == ' ' && buf[i + 1] != '\0') count++;
    }
    return count;
}

/* ── Camera open / close ──────────────────────────────────────────────────── */

TsiCtx* tsi_open_camera(const char* camera_id) {
    if (!camera_id) return NULL;

    void* handle = NULL;
    if (tl_camera_open_camera(camera_id, &handle) != 0 || !handle)
        return NULL;

    TsiCtx* ctx = (TsiCtx*)calloc(1, sizeof(TsiCtx));
    if (!ctx) { tl_camera_close_camera(handle); return NULL; }
    ctx->handle = handle;

    /* Software-triggered mode; frames_per_trigger = 1 for snap (overridden per call). */
    tl_camera_set_operation_mode(handle, TL_CAMERA_OPERATION_MODE_SOFTWARE_TRIGGERED);
    ctx->operation_mode = 1;

    /* Read sensor properties. */
    tl_camera_get_camera_sensor_type(handle, &ctx->sensor_type);
    tl_camera_get_bit_depth(handle, &ctx->bit_depth);
    ctx->bytes_per_pixel = 2;   /* SDK3 always delivers uint16 */
    if (ctx->sensor_type == 1) {
        ctx->bit_depth = 8;
        ctx->bytes_per_pixel = 4;
    }

    tl_camera_get_image_width(handle,  &ctx->sensor_width);
    tl_camera_get_image_height(handle, &ctx->sensor_height);
    ctx->img_width  = ctx->sensor_width;
    ctx->img_height = ctx->sensor_height;

    /* Register frame callback. */
    tl_camera_set_frame_available_callback(handle, frame_callback, ctx);

    return ctx;
}

void tsi_close_camera(TsiCtx* ctx) {
    if (!ctx) return;
    if (ctx->in_sequence) {
        tl_camera_disarm(ctx->handle);
        ctx->in_sequence = 0;
    }
    tl_camera_close_camera(ctx->handle);
    free(ctx->image_buf);
    free(ctx);
}

/* ── Property getters ─────────────────────────────────────────────────────── */

int tsi_get_image_width(TsiCtx* ctx)         { return ctx ? ctx->img_width  : 0; }
int tsi_get_image_height(TsiCtx* ctx)        { return ctx ? ctx->img_height : 0; }
int tsi_get_sensor_width(TsiCtx* ctx)        { return ctx ? ctx->sensor_width  : 0; }
int tsi_get_sensor_height(TsiCtx* ctx)       { return ctx ? ctx->sensor_height : 0; }
int tsi_get_bit_depth(TsiCtx* ctx)           { return ctx ? ctx->bit_depth : 0; }
int tsi_get_bytes_per_pixel(TsiCtx* ctx)     { return ctx ? ctx->bytes_per_pixel : 0; }
int tsi_get_sensor_type(TsiCtx* ctx)         { return ctx ? ctx->sensor_type : 0; }

int tsi_get_serial_number(TsiCtx* ctx, char* buf, int len) {
    if (!ctx || !buf) return -1;
    return tl_camera_get_serial_number(ctx->handle, buf, len) == 0 ? 0 : -1;
}

int tsi_get_firmware_version(TsiCtx* ctx, char* buf, int len) {
    if (!ctx || !buf) return -1;
    return tl_camera_get_firmware_version(ctx->handle, buf, len) == 0 ? 0 : -1;
}

/* ── Exposure (milliseconds in API, microseconds in SDK) ─────────────────── */

long long tsi_get_exposure_us(TsiCtx* ctx) {
    if (!ctx) return -1;
    long long v = 0;
    tl_camera_get_exposure_time(ctx->handle, &v);
    return v;
}

int tsi_set_exposure_us(TsiCtx* ctx, long long us) {
    if (!ctx) return -1;
    return tl_camera_set_exposure_time(ctx->handle, us) == 0 ? 0 : -1;
}

int tsi_get_exposure_range_us(TsiCtx* ctx, long long* min_out, long long* max_out) {
    if (!ctx) return -1;
    return tl_camera_get_exposure_time_range(ctx->handle, min_out, max_out) == 0 ? 0 : -1;
}

/* ── Trigger mode / polarity ─────────────────────────────────────────────── */

int tsi_is_operation_mode_supported(TsiCtx* ctx, int mode) {
    if (!ctx) return -1;
    int sdk_mode = sdk_operation_mode_from_code(mode);
    if (sdk_mode < 0) return -1;
    int supported = 0;
    if (tl_camera_get_is_operation_mode_supported(ctx->handle, sdk_mode, &supported) != 0) {
        return -1;
    }
    return supported ? 1 : 0;
}

int tsi_get_operation_mode(TsiCtx* ctx) {
    if (!ctx) return -1;
    int sdk_mode = 0;
    if (tl_camera_get_operation_mode(ctx->handle, &sdk_mode) != 0) return -1;
    ctx->operation_mode = operation_mode_code_from_sdk(sdk_mode);
    return ctx->operation_mode;
}

int tsi_set_operation_mode(TsiCtx* ctx, int mode) {
    if (!ctx) return -1;
    int sdk_mode = sdk_operation_mode_from_code(mode);
    if (sdk_mode < 0) return -1;
    if (tl_camera_set_operation_mode(ctx->handle, sdk_mode) != 0) return -1;
    ctx->operation_mode = mode;
    return 0;
}

int tsi_get_trigger_polarity(TsiCtx* ctx) {
    if (!ctx) return -1;
    int polarity = 0;
    if (tl_camera_get_trigger_polarity(ctx->handle, &polarity) != 0) return -1;
    return polarity == TL_CAMERA_TRIGGER_POLARITY_ACTIVE_HIGH ? 1 : 0;
}

int tsi_set_trigger_polarity(TsiCtx* ctx, int polarity) {
    if (!ctx) return -1;
    int sdk_polarity = polarity ? TL_CAMERA_TRIGGER_POLARITY_ACTIVE_HIGH
                                : TL_CAMERA_TRIGGER_POLARITY_ACTIVE_LOW;
    return tl_camera_set_trigger_polarity(ctx->handle, sdk_polarity) == 0 ? 0 : -1;
}

/* ── EEP / hot-pixel correction / gain ───────────────────────────────────── */

int tsi_is_eep_supported(TsiCtx* ctx) {
    if (!ctx) return -1;
    int supported = 0;
    if (tl_camera_get_is_eep_supported(ctx->handle, &supported) != 0) return -1;
    return supported ? 1 : 0;
}

int tsi_get_eep_enabled(TsiCtx* ctx) {
    if (!ctx) return -1;
    int status = 0;
    if (tl_camera_get_eep_status(ctx->handle, &status) != 0) return -1;
    return status == TL_CAMERA_EEP_STATUS_DISABLED ? 0 : 1;
}

int tsi_set_eep_enabled(TsiCtx* ctx, int enabled) {
    if (!ctx) return -1;
    return tl_camera_set_is_eep_enabled(ctx->handle, enabled ? 1 : 0) == 0 ? 0 : -1;
}

int tsi_get_hot_pixel_threshold_range(TsiCtx* ctx, int* min_out, int* max_out) {
    if (!ctx) return -1;
    return tl_camera_get_hot_pixel_correction_threshold_range(ctx->handle, min_out, max_out) == 0 ? 0 : -1;
}

int tsi_get_hot_pixel_enabled(TsiCtx* ctx) {
    if (!ctx) return -1;
    int enabled = 0;
    if (tl_camera_get_is_hot_pixel_correction_enabled(ctx->handle, &enabled) != 0) return -1;
    return enabled ? 1 : 0;
}

int tsi_set_hot_pixel_enabled(TsiCtx* ctx, int enabled) {
    if (!ctx) return -1;
    return tl_camera_set_is_hot_pixel_correction_enabled(ctx->handle, enabled ? 1 : 0) == 0 ? 0 : -1;
}

int tsi_get_hot_pixel_threshold(TsiCtx* ctx) {
    if (!ctx) return -1;
    int threshold = 0;
    if (tl_camera_get_hot_pixel_correction_threshold(ctx->handle, &threshold) != 0) return -1;
    return threshold;
}

int tsi_set_hot_pixel_threshold(TsiCtx* ctx, int threshold) {
    if (!ctx) return -1;
    return tl_camera_set_hot_pixel_correction_threshold(ctx->handle, threshold) == 0 ? 0 : -1;
}

int tsi_get_gain_range(TsiCtx* ctx, int* min_out, int* max_out) {
    if (!ctx) return -1;
    return tl_camera_get_gain_range(ctx->handle, min_out, max_out) == 0 ? 0 : -1;
}

int tsi_convert_gain_to_db(TsiCtx* ctx, int gain_index, double* gain_db) {
    if (!ctx || !gain_db) return -1;
    return tl_camera_convert_gain_to_decibels(ctx->handle, gain_index, gain_db) == 0 ? 0 : -1;
}

int tsi_get_gain_db(TsiCtx* ctx, double* gain_db) {
    if (!ctx || !gain_db) return -1;
    int gain = 0;
    if (tl_camera_get_gain(ctx->handle, &gain) != 0) return -1;
    return tl_camera_convert_gain_to_decibels(ctx->handle, gain, gain_db) == 0 ? 0 : -1;
}

int tsi_set_gain_db(TsiCtx* ctx, double gain_db) {
    if (!ctx) return -1;
    int gain = 0;
    if (tl_camera_convert_decibels_to_gain(ctx->handle, gain_db, &gain) != 0) return -1;
    return tl_camera_set_gain(ctx->handle, gain) == 0 ? 0 : -1;
}

/* ── ROI ──────────────────────────────────────────────────────────────────── */

/* Follow upstream TSI3Cam: SDK3 ROI is passed as (x_top_left, y_top_left,
   x_top_left + width, y_top_left + height), in unbinned pixels. */

int tsi_set_roi(TsiCtx* ctx, int x, int y, int w, int h) {
    if (!ctx) return -1;
    int x2 = x + w;
    int y2 = y + h;
    if (tl_camera_set_roi(ctx->handle, x, y, x2, y2) != 0) return -1;
    return refresh_image_dims(ctx);
}

int tsi_clear_roi(TsiCtx* ctx) {
    if (!ctx) return -1;
    if (tl_camera_set_roi(ctx->handle, 0, 0,
                          ctx->sensor_width,
                          ctx->sensor_height) != 0) {
        return -1;
    }
    return refresh_image_dims(ctx);
}

int tsi_get_roi(TsiCtx* ctx, int* x, int* y, int* w, int* h) {
    if (!ctx) return -1;
    int x1 = 0, y1 = 0, x2 = 0, y2 = 0;
    if (tl_camera_get_roi(ctx->handle, &x1, &y1, &x2, &y2) != 0) return -1;
    *x = x1;  *y = y1;
    *w = x2 - x1;  *h = y2 - y1;
    return 0;
}

/* ── Binning ──────────────────────────────────────────────────────────────── */

int tsi_get_binx(TsiCtx* ctx) {
    if (!ctx) return 1;
    int v = 1;
    tl_camera_get_binx(ctx->handle, &v);
    return v;
}

int tsi_get_biny(TsiCtx* ctx) {
    if (!ctx) return 1;
    int v = 1;
    tl_camera_get_biny(ctx->handle, &v);
    return v;
}

int tsi_set_binx(TsiCtx* ctx, int val) {
    if (!ctx) return -1;
    if (tl_camera_set_binx(ctx->handle, val) != 0) return -1;
    return refresh_image_dims(ctx);
}

int tsi_set_biny(TsiCtx* ctx, int val) {
    if (!ctx) return -1;
    if (tl_camera_set_biny(ctx->handle, val) != 0) return -1;
    return refresh_image_dims(ctx);
}

int tsi_get_binx_range(TsiCtx* ctx, int* min_out, int* max_out) {
    if (!ctx) return -1;
    return tl_camera_get_binx_range(ctx->handle, min_out, max_out) == 0 ? 0 : -1;
}

int tsi_get_biny_range(TsiCtx* ctx, int* min_out, int* max_out) {
    if (!ctx) return -1;
    return tl_camera_get_biny_range(ctx->handle, min_out, max_out) == 0 ? 0 : -1;
}

/* ── Snap (single frame, blocking) ───────────────────────────────────────── */

/**
 * Snap one frame in software-trigger mode.
 * `timeout_ms` is the maximum wait including exposure + readout time.
 * Returns 0 on success, -1 on error or timeout.
 */
int tsi_snap(TsiCtx* ctx, int timeout_ms) {
    if (!ctx || ctx->in_sequence) return -1;

    ctx->frame_ready = 0;
    tl_camera_set_frames_per_trigger_zero_for_unlimited(ctx->handle, 1);

    if (tl_camera_arm(ctx->handle, 2) != 0) return -1;
    if (ctx->operation_mode == 1) {
        if (tl_camera_issue_software_trigger(ctx->handle) != 0) {
            tl_camera_disarm(ctx->handle);
            return -1;
        }
    }

    int ret = wait_for_frame(ctx, timeout_ms);
    tl_camera_disarm(ctx->handle);
    return ret;
}

const uint16_t* tsi_get_frame_ptr(TsiCtx* ctx) {
    return ctx ? ctx->image_buf : NULL;
}

int tsi_get_frame_bytes(TsiCtx* ctx) {
    return ctx ? ctx->frame_bytes : 0;
}

/* ── Sequence acquisition ─────────────────────────────────────────────────── */

/**
 * Start sequence acquisition.  A frame_count of 0 means unlimited, matching
 * the TSI SDK and upstream Micro-Manager adapter.
 */
int tsi_start_cont(TsiCtx* ctx, int frame_count) {
    if (!ctx || ctx->in_sequence) return -1;
    if (frame_count < 0) frame_count = 0;

    ctx->frame_ready = 0;
    if (tl_camera_set_frames_per_trigger_zero_for_unlimited(ctx->handle, frame_count) != 0) {
        return -1;
    }

    if (tl_camera_arm(ctx->handle, 2) != 0) return -1;
    if (ctx->operation_mode == 1) {
        if (tl_camera_issue_software_trigger(ctx->handle) != 0) {
            tl_camera_disarm(ctx->handle);
            return -1;
        }
    }
    ctx->in_sequence = 1;
    return 0;
}

/**
 * Wait for the next frame from the continuous stream.
 * Returns 0 on success (frame copied to internal buffer), -1 on timeout.
 */
int tsi_get_next_frame(TsiCtx* ctx, int timeout_ms) {
    if (!ctx || !ctx->in_sequence) return -1;
    return wait_for_frame(ctx, timeout_ms);
}

int tsi_stop_cont(TsiCtx* ctx) {
    if (!ctx) return -1;
    if (!ctx->in_sequence) return 0;
    tl_camera_disarm(ctx->handle);
    ctx->in_sequence = 0;
    return 0;
}
