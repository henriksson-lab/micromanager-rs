/**
 * shim.c — Thin C wrapper around the Andor SDK3 atcore API.
 *
 * The SDK3 API uses wide-character (wchar_t / AT_WC) feature names and a
 * buffer-queue model for image delivery.  This shim exposes a narrow-string,
 * opaque-context API suitable for Rust FFI.
 *
 * Snap flow:
 *   1. AT_QueueBuffer   — provide an aligned acquisition buffer
 *   2. AT_Command       — "AcquisitionStart"
 *   3. AT_WaitBuffer    — block until frame arrives (returns pointer into
 *                         the same buffer we queued)
 *   4. Unpack pixels    — strip per-row stride padding into a compact buffer
 *   5. AT_Command       — "AcquisitionStop"
 *   6. AT_Flush         — discard any remaining queued buffers
 *
 * Continuous mode follows the same pattern but with multiple buffers queued
 * and AcquisitionStart called once; each frame is retrieved with WaitBuffer
 * and re-queued after copying.
 *
 * AOIStride (bytes per row in the acquisition buffer, including padding) is
 * read after each ROI/binning change and used during unpacking.
 */

#include "atcore.h"
#include "atutility.h"

#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <wchar.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Portable aligned allocation ──────────────────────────────────────────── */

#ifdef _WIN32
#  include <malloc.h>
   static void* shim_alloc_aligned(size_t size) { return _aligned_malloc(size, 8); }
   static void  shim_free_aligned(void* p)       { _aligned_free(p); }
#else
#  include <stdlib.h>
   static void* shim_alloc_aligned(size_t size) {
       void* p = NULL;
       if (posix_memalign(&p, 8, size) != 0) return NULL;
       return p;
   }
   static void shim_free_aligned(void* p) { free(p); }
#endif

/* ── Wide-string helpers ──────────────────────────────────────────────────── */

/* Convert a narrow C string to a stack-allocated wchar_t buffer (max 128 chars). */
#define WIDEN(narrow, wide) \
    wchar_t wide[128];      \
    mbstowcs(wide, narrow, 127); \
    wide[127] = L'\0'

/* Convert a wchar_t string to a narrow char buffer (max `len` bytes). */
static void narrow(const wchar_t* src, char* dst, int len) {
    int i;
    for (i = 0; i < len - 1 && src[i]; i++)
        dst[i] = (char)(src[i] & 0xFF);
    dst[i] = '\0';
}

/* ── Andor3Ctx ─────────────────────────────────────────────────────────────── */

#define MAX_CONT_BUFS 8
#define LENGTH_FIELD_SIZE 4
#define CID_FIELD_SIZE 4
#define CID_FPGA_TICKS 1
#define ANDOR3_ERR_HARDWARE_OVERFLOW AT_ERR_HARDWARE_OVERFLOW

typedef struct Andor3Ctx {
    AT_H handle;

    /* Current image geometry (pixels, after ROI + binning). */
    int  img_width;
    int  img_height;
    int  stride;          /* bytes per row in acquisition buffer */
    int  bytes_per_pixel; /* always 2 for Mono16 */
    int  bit_depth;

    /* Compact output buffer (no row padding). */
    uint8_t* image_buf;
    size_t   image_buf_bytes;
    int      frame_bytes;

    /* Acquisition buffer (stride-aligned, SDK-owned side). */
    uint8_t* acq_buf;
    size_t   acq_buf_bytes;

    /* Continuous mode. */
    int      in_sequence;
    uint8_t* cont_bufs[MAX_CONT_BUFS];
    size_t   cont_buf_bytes;

    AT_64    last_timestamp;
    int      last_timestamp_valid;
    int      last_wait_error;
    int      buffer_overflow_event_registered;
    int      buffer_overflow_event_fired;
    int      exposure_end_event_registered;
    int      exposure_end_event_fired;
} Andor3Ctx;

/* ── Helpers ──────────────────────────────────────────────────────────────── */

static void refresh_geometry(Andor3Ctx* ctx) {
    AT_64 v = 0;
    AT_GetInt(ctx->handle, L"AOIWidth",  &v); ctx->img_width  = (int)v;
    AT_GetInt(ctx->handle, L"AOIHeight", &v); ctx->img_height = (int)v;
    AT_GetInt(ctx->handle, L"AOIStride", &v); ctx->stride     = (int)v;
    AT_GetInt(ctx->handle, L"BitDepth",  &v); ctx->bit_depth  = (int)v;
    ctx->bytes_per_pixel = (ctx->bit_depth > 16) ? 4 : 2;
}

static int ensure_acq_buf(Andor3Ctx* ctx) {
    /* SDK requires buffer >= ImageSizeBytes, aligned to 8 bytes. */
    AT_64 img_bytes = 0;
    if (AT_GetInt(ctx->handle, L"ImageSizeBytes", &img_bytes) != AT_SUCCESS)
        return -1;
    size_t needed = (size_t)img_bytes;
    if (needed > ctx->acq_buf_bytes) {
        shim_free_aligned(ctx->acq_buf);
        ctx->acq_buf = (uint8_t*)shim_alloc_aligned(needed);
        if (!ctx->acq_buf) { ctx->acq_buf_bytes = 0; return -1; }
        ctx->acq_buf_bytes = needed;
    }
    return 0;
}

static int ensure_image_buf(Andor3Ctx* ctx) {
    size_t needed = (size_t)(ctx->img_width * ctx->img_height * ctx->bytes_per_pixel);
    if (needed > ctx->image_buf_bytes) {
        free(ctx->image_buf);
        ctx->image_buf = (uint8_t*)malloc(needed);
        if (!ctx->image_buf) { ctx->image_buf_bytes = 0; return -1; }
        ctx->image_buf_bytes = needed;
    }
    return 0;
}

/* Copy one acquired frame (stride-padded) into the compact output buffer. */
static void unpack_frame(Andor3Ctx* ctx, const uint8_t* src) {
    int row_bytes = ctx->img_width * ctx->bytes_per_pixel;
    uint8_t* dst  = ctx->image_buf;
    for (int y = 0; y < ctx->img_height; y++) {
        memcpy(dst, src, (size_t)row_bytes);
        dst += row_bytes;
        src += (size_t)ctx->stride;
    }
    ctx->frame_bytes = ctx->img_width * ctx->img_height * ctx->bytes_per_pixel;
}

static int is_software_trigger_mode(Andor3Ctx* ctx) {
    wchar_t trigger_mode[128] = {0};
    int trigger_idx = 0;
    return AT_GetEnumIndex(ctx->handle, L"TriggerMode", &trigger_idx) == AT_SUCCESS &&
           AT_GetEnumStringByIndex(ctx->handle, L"TriggerMode", trigger_idx,
                                   trigger_mode, 128) == AT_SUCCESS &&
           wcscmp(trigger_mode, L"Software") == 0;
}

static void send_software_trigger_if_needed(Andor3Ctx* ctx) {
    if (is_software_trigger_mode(ctx)) {
        AT_Command(ctx->handle, L"SoftwareTrigger");
    }
}

static int AT_EXP_CONV andor3_event_callback(AT_H handle, const AT_WC* feature, void* context) {
    (void)handle;
    Andor3Ctx* ctx = (Andor3Ctx*)context;
    if (!ctx || !feature) return AT_CALLBACK_SUCCESS;
    if (wcscmp(feature, L"BufferOverflowEvent") == 0) {
        ctx->buffer_overflow_event_fired = 1;
    } else if (wcscmp(feature, L"ExposureEndEvent") == 0) {
        ctx->exposure_end_event_fired = 1;
        if (ctx->in_sequence && is_software_trigger_mode(ctx)) {
            AT_Command(ctx->handle, L"SoftwareTrigger");
        }
    }
    return AT_CALLBACK_SUCCESS;
}

static int register_event(Andor3Ctx* ctx, const wchar_t* event_name) {
    AT_BOOL implemented = AT_FALSE;
    if (AT_IsImplemented(ctx->handle, event_name, &implemented) != AT_SUCCESS ||
        implemented != AT_TRUE) {
        return 0;
    }
    if (AT_RegisterFeatureCallback(ctx->handle, event_name, andor3_event_callback, ctx) != AT_SUCCESS) {
        return 0;
    }

    AT_BOOL event_selector_implemented = AT_FALSE;
    AT_BOOL event_enable_implemented = AT_FALSE;
    if (AT_IsImplemented(ctx->handle, L"EventSelector", &event_selector_implemented) == AT_SUCCESS &&
        AT_IsImplemented(ctx->handle, L"EventEnable", &event_enable_implemented) == AT_SUCCESS &&
        event_selector_implemented == AT_TRUE &&
        event_enable_implemented == AT_TRUE) {
        AT_SetEnumString(ctx->handle, L"EventSelector", event_name);
        AT_SetBool(ctx->handle, L"EventEnable", AT_TRUE);
    }
    return 1;
}

static void reset_event_flag(Andor3Ctx* ctx, const wchar_t* event_name) {
    if (wcscmp(event_name, L"BufferOverflowEvent") == 0) {
        ctx->buffer_overflow_event_fired = 0;
    } else if (wcscmp(event_name, L"ExposureEndEvent") == 0) {
        ctx->exposure_end_event_fired = 0;
    }
}

static int normalize_wait_error(Andor3Ctx* ctx, int rc) {
    if (rc == AT_SUCCESS) return AT_SUCCESS;
    if (rc == AT_ERR_HARDWARE_OVERFLOW ||
        (ctx->buffer_overflow_event_registered && ctx->buffer_overflow_event_fired)) {
        reset_event_flag(ctx, L"BufferOverflowEvent");
        return ANDOR3_ERR_HARDWARE_OVERFLOW;
    }
    return rc;
}

static uint32_t read_le_u32(const uint8_t* p) {
    return ((uint32_t)p[0]) |
           ((uint32_t)p[1] << 8) |
           ((uint32_t)p[2] << 16) |
           ((uint32_t)p[3] << 24);
}

static AT_64 read_le_i64(const uint8_t* p) {
    uint64_t v = ((uint64_t)p[0]) |
                 ((uint64_t)p[1] << 8) |
                 ((uint64_t)p[2] << 16) |
                 ((uint64_t)p[3] << 24) |
                 ((uint64_t)p[4] << 32) |
                 ((uint64_t)p[5] << 40) |
                 ((uint64_t)p[6] << 48) |
                 ((uint64_t)p[7] << 56);
    return (AT_64)v;
}

static void parse_frame_metadata(Andor3Ctx* ctx, const uint8_t* frame, int frame_size) {
    if (!ctx) return;
    ctx->last_timestamp = 0;
    ctx->last_timestamp_valid = 0;
    if (!frame || frame_size <= 0) return;

    const uint8_t* begin = frame;
    const uint8_t* p = frame + frame_size;
    while (p > begin + LENGTH_FIELD_SIZE + CID_FIELD_SIZE) {
        p -= LENGTH_FIELD_SIZE;
        uint32_t feature_size = read_le_u32(p);
        if (feature_size < CID_FIELD_SIZE || (size_t)(p - begin) < CID_FIELD_SIZE) return;

        p -= CID_FIELD_SIZE;
        uint32_t cid = read_le_u32(p);
        uint32_t payload_size = feature_size - CID_FIELD_SIZE;
        if ((size_t)(p - begin) < payload_size) return;
        p -= payload_size;

        if (cid == CID_FPGA_TICKS && payload_size >= 8) {
            ctx->last_timestamp = read_le_i64(p);
            ctx->last_timestamp_valid = 1;
            return;
        }
    }
}

static AT_64 current_binning_factor(Andor3Ctx* ctx) {
    int idx = 0;
    wchar_t value[128] = {0};
    if (AT_GetEnumIndex(ctx->handle, L"AOIBinning", &idx) != AT_SUCCESS ||
        AT_GetEnumStringByIndex(ctx->handle, L"AOIBinning", idx, value, 128) != AT_SUCCESS) {
        return 1;
    }
    if (wcscmp(value, L"2x2") == 0) return 2;
    if (wcscmp(value, L"4x4") == 0) return 4;
    return 1;
}

/* ── SDK lifecycle ─────────────────────────────────────────────────────────── */

int andor3_sdk_open(void) {
    if (AT_InitialiseLibrary() != AT_SUCCESS) return -1;
    if (AT_InitialiseUtilityLibrary() != AT_SUCCESS) {
        AT_FinaliseLibrary();
        return -1;
    }
    return 0;
}

void andor3_sdk_close(void) {
    AT_FinaliseUtilityLibrary();
    AT_FinaliseLibrary();
}

int andor3_get_device_count(void) {
    AT_64 count = 0;
    if (AT_GetInt(AT_HANDLE_SYSTEM, L"DeviceCount", &count) != AT_SUCCESS)
        return 0;
    return (int)count;
}

/* ── Open / close ──────────────────────────────────────────────────────────── */

Andor3Ctx* andor3_open(int camera_index) {
    AT_H handle = AT_HANDLE_UNINITIALISED;
    if (AT_Open(camera_index, &handle) != AT_SUCCESS) return NULL;

    Andor3Ctx* ctx = (Andor3Ctx*)calloc(1, sizeof(Andor3Ctx));
    if (!ctx) { AT_Close(handle); return NULL; }
    ctx->handle         = handle;
    ctx->bytes_per_pixel = 2;
    ctx->last_wait_error = AT_SUCCESS;
    ctx->buffer_overflow_event_registered = register_event(ctx, L"BufferOverflowEvent");
    ctx->exposure_end_event_registered = register_event(ctx, L"ExposureEndEvent");

    /* Internal trigger, single-frame acquisition mode for snap. */
    AT_SetEnumString(handle, L"TriggerMode",   L"Internal");

    refresh_geometry(ctx);
    return ctx;
}

void andor3_close(Andor3Ctx* ctx) {
    if (!ctx) return;
    if (ctx->in_sequence) {
        AT_Command(ctx->handle, L"AcquisitionStop");
        AT_Flush(ctx->handle);
        for (int i = 0; i < MAX_CONT_BUFS; i++) {
            shim_free_aligned(ctx->cont_bufs[i]);
            ctx->cont_bufs[i] = NULL;
        }
        ctx->in_sequence = 0;
    }
    if (ctx->buffer_overflow_event_registered) {
        AT_UnregisterFeatureCallback(ctx->handle, L"BufferOverflowEvent", andor3_event_callback, ctx);
    }
    if (ctx->exposure_end_event_registered) {
        AT_UnregisterFeatureCallback(ctx->handle, L"ExposureEndEvent", andor3_event_callback, ctx);
    }
    AT_Close(ctx->handle);
    shim_free_aligned(ctx->acq_buf);
    free(ctx->image_buf);
    free(ctx);
}

/* ── Property getters / setters ─────────────────────────────────────────────── */

int andor3_get_image_width(Andor3Ctx* ctx)       { return ctx ? ctx->img_width       : 0; }
int andor3_get_image_height(Andor3Ctx* ctx)      { return ctx ? ctx->img_height      : 0; }
int andor3_get_bytes_per_pixel(Andor3Ctx* ctx)   { return ctx ? ctx->bytes_per_pixel : 2; }
int andor3_get_bit_depth(Andor3Ctx* ctx)         { return ctx ? ctx->bit_depth       : 16; }

int andor3_get_sensor_width(Andor3Ctx* ctx) {
    if (!ctx) return 0;
    AT_64 v = 0;
    AT_GetInt(ctx->handle, L"SensorWidth", &v);
    return (int)v;
}
int andor3_get_sensor_height(Andor3Ctx* ctx) {
    if (!ctx) return 0;
    AT_64 v = 0;
    AT_GetInt(ctx->handle, L"SensorHeight", &v);
    return (int)v;
}

double andor3_get_exposure_s(Andor3Ctx* ctx) {
    if (!ctx) return 0.0;
    double v = 0.0;
    AT_GetFloat(ctx->handle, L"ExposureTime", &v);
    return v;
}

int andor3_set_exposure_s(Andor3Ctx* ctx, double seconds) {
    if (!ctx) return -1;
    return AT_SetFloat(ctx->handle, L"ExposureTime", seconds) == AT_SUCCESS ? 0 : -1;
}

double andor3_get_temperature(Andor3Ctx* ctx) {
    if (!ctx) return 0.0;
    double v = 0.0;
    AT_GetFloat(ctx->handle, L"SensorTemperature", &v);
    return v;
}

int andor3_is_implemented(Andor3Ctx* ctx, const char* feature) {
    if (!ctx) return 0;
    WIDEN(feature, wfeature);
    AT_BOOL v = AT_FALSE;
    if (AT_IsImplemented(ctx->handle, wfeature, &v) != AT_SUCCESS) return 0;
    return v == AT_TRUE ? 1 : 0;
}

int andor3_is_read_only(Andor3Ctx* ctx, const char* feature) {
    if (!ctx) return 0;
    WIDEN(feature, wfeature);
    AT_BOOL v = AT_FALSE;
    if (AT_IsReadOnly(ctx->handle, wfeature, &v) != AT_SUCCESS) return 0;
    return v == AT_TRUE ? 1 : 0;
}

/* String feature (narrow output). */
int andor3_get_string(Andor3Ctx* ctx, const char* feature, char* buf, int len) {
    if (!ctx || !buf) return -1;
    WIDEN(feature, wfeature);
    wchar_t wbuf[256] = {0};
    AT_H handle = strcmp(feature, "SoftwareVersion") == 0 ? AT_HANDLE_SYSTEM : ctx->handle;
    if (AT_GetString(handle, wfeature, wbuf, 256) != AT_SUCCESS) return -1;
    narrow(wbuf, buf, len);
    return 0;
}

/* Enum feature — get current value as narrow string. */
int andor3_get_enum(Andor3Ctx* ctx, const char* feature, char* buf, int len) {
    if (!ctx || !buf) return -1;
    WIDEN(feature, wfeature);
    int idx = 0;
    if (AT_GetEnumIndex(ctx->handle, wfeature, &idx) != AT_SUCCESS) return -1;
    wchar_t wbuf[128] = {0};
    if (AT_GetEnumStringByIndex(ctx->handle, wfeature, idx, wbuf, 128) != AT_SUCCESS) return -1;
    narrow(wbuf, buf, len);
    return 0;
}

/* Enum feature — set by narrow string value. */
int andor3_set_enum(Andor3Ctx* ctx, const char* feature, const char* value) {
    if (!ctx) return -1;
    WIDEN(feature, wfeature);
    WIDEN(value,   wvalue);
    if (AT_SetEnumString(ctx->handle, wfeature, wvalue) != AT_SUCCESS) return -1;
    if (strcmp(feature, "PixelEncoding") == 0 || strcmp(feature, "AOIBinning") == 0) {
        refresh_geometry(ctx);
    }
    return 0;
}

int andor3_set_enum_index(Andor3Ctx* ctx, const char* feature, int index) {
    if (!ctx) return -1;
    WIDEN(feature, wfeature);
    if (AT_SetEnumIndex(ctx->handle, wfeature, index) != AT_SUCCESS) return -1;
    if (strcmp(feature, "PixelEncoding") == 0 || strcmp(feature, "AOIBinning") == 0) {
        refresh_geometry(ctx);
    }
    return 0;
}

/* Enumerate available enum values for a feature (newline-separated, narrow). */
int andor3_enum_values(Andor3Ctx* ctx, const char* feature, char* buf, int len) {
    if (!ctx || !buf || len <= 0) return -1;
    buf[0] = '\0';
    WIDEN(feature, wfeature);
    int count = 0;
    AT_GetEnumCount(ctx->handle, wfeature, &count);
    int written = 0;
    for (int i = 0; i < count; i++) {
        AT_BOOL avail = AT_FALSE;
        AT_IsEnumIndexAvailable(ctx->handle, wfeature, i, &avail);
        if (!avail) continue;
        wchar_t wval[128] = {0};
        if (AT_GetEnumStringByIndex(ctx->handle, wfeature, i, wval, 128) != AT_SUCCESS) continue;
        char val[128]; narrow(wval, val, 128);
        int vlen = (int)strlen(val);
        if (written + vlen + 2 >= len) break;
        if (written > 0) { buf[written++] = '\n'; }
        memcpy(buf + written, val, (size_t)vlen);
        written += vlen;
        buf[written] = '\0';
    }
    return written;
}

int andor3_get_float(Andor3Ctx* ctx, const char* feature, double* value) {
    if (!ctx || !value) return -1;
    WIDEN(feature, wfeature);
    return AT_GetFloat(ctx->handle, wfeature, value) == AT_SUCCESS ? 0 : -1;
}

int andor3_set_float(Andor3Ctx* ctx, const char* feature, double value) {
    if (!ctx) return -1;
    WIDEN(feature, wfeature);
    return AT_SetFloat(ctx->handle, wfeature, value) == AT_SUCCESS ? 0 : -1;
}

int andor3_get_float_limits(Andor3Ctx* ctx, const char* feature, double* min, double* max) {
    if (!ctx || !min || !max) return -1;
    WIDEN(feature, wfeature);
    if (AT_GetFloatMin(ctx->handle, wfeature, min) != AT_SUCCESS) return -1;
    if (AT_GetFloatMax(ctx->handle, wfeature, max) != AT_SUCCESS) return -1;
    return 0;
}

int andor3_get_int(Andor3Ctx* ctx, const char* feature, AT_64* value) {
    if (!ctx || !value) return -1;
    WIDEN(feature, wfeature);
    return AT_GetInt(ctx->handle, wfeature, value) == AT_SUCCESS ? 0 : -1;
}

int andor3_set_int(Andor3Ctx* ctx, const char* feature, AT_64 value) {
    if (!ctx) return -1;
    WIDEN(feature, wfeature);
    return AT_SetInt(ctx->handle, wfeature, value) == AT_SUCCESS ? 0 : -1;
}

int andor3_get_int_limits(Andor3Ctx* ctx, const char* feature, AT_64* min, AT_64* max) {
    if (!ctx || !min || !max) return -1;
    WIDEN(feature, wfeature);
    if (AT_GetIntMin(ctx->handle, wfeature, min) != AT_SUCCESS) return -1;
    if (AT_GetIntMax(ctx->handle, wfeature, max) != AT_SUCCESS) return -1;
    return 0;
}

int andor3_get_bool(Andor3Ctx* ctx, const char* feature, int* value) {
    if (!ctx || !value) return -1;
    WIDEN(feature, wfeature);
    AT_BOOL v = AT_FALSE;
    if (AT_GetBool(ctx->handle, wfeature, &v) != AT_SUCCESS) return -1;
    *value = v == AT_TRUE ? 1 : 0;
    return 0;
}

int andor3_set_bool(Andor3Ctx* ctx, const char* feature, int value) {
    if (!ctx) return -1;
    WIDEN(feature, wfeature);
    return AT_SetBool(ctx->handle, wfeature, value ? AT_TRUE : AT_FALSE) == AT_SUCCESS ? 0 : -1;
}

int andor3_command(Andor3Ctx* ctx, const char* feature) {
    if (!ctx) return -1;
    WIDEN(feature, wfeature);
    return AT_Command(ctx->handle, wfeature) == AT_SUCCESS ? 0 : -1;
}

/* AOI / Binning. */
int andor3_set_aoi(Andor3Ctx* ctx, int left, int top, int width, int height) {
    if (!ctx) return -1;
    AT_64 old_left = 0, old_top = 0, old_width = 0, old_height = 0;
    if (AT_GetInt(ctx->handle, L"AOILeft",   &old_left) != AT_SUCCESS) return -1;
    if (AT_GetInt(ctx->handle, L"AOITop",    &old_top) != AT_SUCCESS) return -1;
    if (AT_GetInt(ctx->handle, L"AOIWidth",  &old_width) != AT_SUCCESS) return -1;
    if (AT_GetInt(ctx->handle, L"AOIHeight", &old_height) != AT_SUCCESS) return -1;

    /* SDK3 AOI is 1-based. */
    if (AT_SetInt(ctx->handle, L"AOILeft",   1) != AT_SUCCESS) goto rollback;
    if (AT_SetInt(ctx->handle, L"AOITop",    1) != AT_SUCCESS) goto rollback;
    if (AT_SetInt(ctx->handle, L"AOIWidth",  (AT_64)width) != AT_SUCCESS) goto rollback;
    if (AT_SetInt(ctx->handle, L"AOIHeight", (AT_64)height) != AT_SUCCESS) goto rollback;
    if (AT_SetInt(ctx->handle, L"AOILeft",   (AT_64)(left   + 1)) != AT_SUCCESS) goto rollback;
    if (AT_SetInt(ctx->handle, L"AOITop",    (AT_64)(top    + 1)) != AT_SUCCESS) goto rollback;
    refresh_geometry(ctx);
    return 0;

rollback:
    AT_SetInt(ctx->handle, L"AOILeft",   1);
    AT_SetInt(ctx->handle, L"AOITop",    1);
    AT_SetInt(ctx->handle, L"AOIWidth",  old_width);
    AT_SetInt(ctx->handle, L"AOIHeight", old_height);
    AT_SetInt(ctx->handle, L"AOILeft",   old_left);
    AT_SetInt(ctx->handle, L"AOITop",    old_top);
    refresh_geometry(ctx);
    return -1;
}

int andor3_clear_aoi(Andor3Ctx* ctx) {
    if (!ctx) return -1;
    AT_64 sw = 0, sh = 0;
    AT_64 binning = current_binning_factor(ctx);
    if (AT_GetInt(ctx->handle, L"SensorWidth",  &sw) != AT_SUCCESS) return -1;
    if (AT_GetInt(ctx->handle, L"SensorHeight", &sh) != AT_SUCCESS) return -1;
    if (AT_SetInt(ctx->handle, L"AOILeft",   1) != AT_SUCCESS) return -1;
    if (AT_SetInt(ctx->handle, L"AOITop",    1) != AT_SUCCESS) return -1;
    if (AT_SetInt(ctx->handle, L"AOIWidth",  sw / binning) != AT_SUCCESS) return -1;
    if (AT_SetInt(ctx->handle, L"AOIHeight", sh / binning) != AT_SUCCESS) return -1;
    refresh_geometry(ctx);
    return 0;
}

int andor3_get_aoi(Andor3Ctx* ctx, int* left, int* top, int* w, int* h) {
    if (!ctx || !left || !top || !w || !h) return -1;
    AT_64 l = 0, t = 0;
    if (AT_GetInt(ctx->handle, L"AOILeft",   &l) != AT_SUCCESS) return -1;
    if (AT_GetInt(ctx->handle, L"AOITop",    &t) != AT_SUCCESS) return -1;
    *left = (int)(l - 1);  /* convert 1-based → 0-based */
    *top  = (int)(t - 1);
    *w    = ctx->img_width;
    *h    = ctx->img_height;
    return 0;
}

/* ── Snap (single-frame, blocking) ─────────────────────────────────────────── */

int andor3_snap(Andor3Ctx* ctx, int timeout_ms) {
    if (!ctx || ctx->in_sequence) return -1;

    refresh_geometry(ctx);
    if (ensure_acq_buf(ctx) != 0) return -1;
    if (ensure_image_buf(ctx) != 0) return -1;

    AT_64 img_bytes = 0;
    AT_GetInt(ctx->handle, L"ImageSizeBytes", &img_bytes);

    if (AT_QueueBuffer(ctx->handle, ctx->acq_buf, (int)img_bytes) != AT_SUCCESS) return -1;
    if (AT_Command(ctx->handle, L"AcquisitionStart") != AT_SUCCESS) {
        AT_Flush(ctx->handle);
        return -1;
    }

    send_software_trigger_if_needed(ctx);

    AT_U8* returned_buf = NULL;
    int    returned_size = 0;
    int rc = AT_WaitBuffer(ctx->handle, &returned_buf, &returned_size,
                           (unsigned int)timeout_ms);
    ctx->last_wait_error = normalize_wait_error(ctx, rc);

    AT_Command(ctx->handle, L"AcquisitionStop");
    AT_Flush(ctx->handle);

    if (ctx->last_wait_error != AT_SUCCESS || !returned_buf || returned_size < (int)img_bytes) return -1;

    parse_frame_metadata(ctx, returned_buf, returned_size);
    unpack_frame(ctx, returned_buf);
    return 0;
}

const uint8_t* andor3_get_frame_ptr(Andor3Ctx* ctx) {
    return ctx ? ctx->image_buf : NULL;
}

int andor3_get_frame_bytes(Andor3Ctx* ctx) {
    return ctx ? ctx->frame_bytes : 0;
}

long long andor3_get_last_timestamp(Andor3Ctx* ctx) {
    return (ctx && ctx->last_timestamp_valid) ? (long long)ctx->last_timestamp : 0;
}

int andor3_has_last_timestamp(Andor3Ctx* ctx) {
    return (ctx && ctx->last_timestamp_valid) ? 1 : 0;
}

int andor3_get_last_wait_error(Andor3Ctx* ctx) {
    return ctx ? ctx->last_wait_error : -1;
}

/* ── Continuous acquisition ─────────────────────────────────────────────────── */

int andor3_start_cont(Andor3Ctx* ctx) {
    if (!ctx || ctx->in_sequence) return -1;

    refresh_geometry(ctx);
    if (ensure_image_buf(ctx) != 0) return -1;

    AT_64 img_bytes = 0;
    AT_GetInt(ctx->handle, L"ImageSizeBytes", &img_bytes);
    size_t buf_sz = (size_t)img_bytes;

    /* Allocate and queue multiple buffers. */
    for (int i = 0; i < MAX_CONT_BUFS; i++) {
        if (!ctx->cont_bufs[i] || ctx->cont_buf_bytes < buf_sz) {
            shim_free_aligned(ctx->cont_bufs[i]);
            ctx->cont_bufs[i] = (uint8_t*)shim_alloc_aligned(buf_sz);
            if (!ctx->cont_bufs[i]) {
                /* Clean up already-allocated ones. */
                for (int j = 0; j < i; j++) {
                    shim_free_aligned(ctx->cont_bufs[j]);
                    ctx->cont_bufs[j] = NULL;
                }
                return -1;
            }
        }
        if (AT_QueueBuffer(ctx->handle, ctx->cont_bufs[i], (int)buf_sz) != AT_SUCCESS) {
            AT_Flush(ctx->handle);
            return -1;
        }
    }
    ctx->cont_buf_bytes = buf_sz;

    if (AT_Command(ctx->handle, L"AcquisitionStart") != AT_SUCCESS) {
        AT_Flush(ctx->handle);
        return -1;
    }
    send_software_trigger_if_needed(ctx);
    reset_event_flag(ctx, L"ExposureEndEvent");
    ctx->in_sequence = 1;
    return 0;
}

int andor3_get_next_frame(Andor3Ctx* ctx, int timeout_ms) {
    if (!ctx || !ctx->in_sequence) return -1;

    AT_U8* returned_buf = NULL;
    int    returned_size = 0;
    int rc = AT_WaitBuffer(ctx->handle, &returned_buf, &returned_size,
                           (unsigned int)timeout_ms);
    if (!ctx->exposure_end_event_registered) {
        send_software_trigger_if_needed(ctx);
    }
    ctx->last_wait_error = normalize_wait_error(ctx, rc);
    AT_64 img_bytes = 0;
    AT_GetInt(ctx->handle, L"ImageSizeBytes", &img_bytes);
    if (ctx->last_wait_error != AT_SUCCESS || !returned_buf || returned_size < (int)img_bytes) return -1;

    parse_frame_metadata(ctx, returned_buf, returned_size);
    unpack_frame(ctx, returned_buf);

    /* Re-queue the buffer so the camera can use it for the next frame. */
    AT_QueueBuffer(ctx->handle, returned_buf, (int)ctx->cont_buf_bytes);
    return 0;
}

int andor3_stop_cont(Andor3Ctx* ctx) {
    if (!ctx || !ctx->in_sequence) return 0;
    AT_Command(ctx->handle, L"AcquisitionStop");
    AT_Flush(ctx->handle);
    ctx->in_sequence = 0;
    return 0;
}

#ifdef __cplusplus
}
#endif
