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
 * Pixel encoding: the shim forces "Mono16" on open so every pixel is exactly
 * 2 bytes.  AOIStride (bytes per row in the acquisition buffer, including
 * padding) is read after each ROI/binning change and used during unpacking.
 */

#include "atcore.h"

#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <wchar.h>

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
} Andor3Ctx;

/* ── Helpers ──────────────────────────────────────────────────────────────── */

static void refresh_geometry(Andor3Ctx* ctx) {
    AT_64 v = 0;
    AT_GetInt(ctx->handle, L"AOIWidth",  &v); ctx->img_width  = (int)v;
    AT_GetInt(ctx->handle, L"AOIHeight", &v); ctx->img_height = (int)v;
    AT_GetInt(ctx->handle, L"AOIStride", &v); ctx->stride     = (int)v;
    AT_GetInt(ctx->handle, L"BitDepth",  &v); ctx->bit_depth  = (int)v;
    ctx->bytes_per_pixel = 2; /* Mono16 */
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

/* ── SDK lifecycle ─────────────────────────────────────────────────────────── */

int andor3_sdk_open(void) {
    return AT_InitialiseLibrary() == AT_SUCCESS ? 0 : -1;
}

void andor3_sdk_close(void) {
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

    /* Force Mono16 encoding for a uniform 2-bytes-per-pixel output. */
    AT_SetEnumString(handle, L"PixelEncoding", L"Mono16");
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

/* String feature (narrow output). */
int andor3_get_string(Andor3Ctx* ctx, const char* feature, char* buf, int len) {
    if (!ctx || !buf) return -1;
    WIDEN(feature, wfeature);
    wchar_t wbuf[256] = {0};
    if (AT_GetString(ctx->handle, wfeature, wbuf, 256) != AT_SUCCESS) return -1;
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
    return AT_SetEnumString(ctx->handle, wfeature, wvalue) == AT_SUCCESS ? 0 : -1;
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

/* AOI / Binning. */
int andor3_set_aoi(Andor3Ctx* ctx, int left, int top, int width, int height) {
    if (!ctx) return -1;
    /* SDK3 AOI is 1-based. */
    AT_SetInt(ctx->handle, L"AOILeft",   (AT_64)(left   + 1));
    AT_SetInt(ctx->handle, L"AOITop",    (AT_64)(top    + 1));
    AT_SetInt(ctx->handle, L"AOIWidth",  (AT_64)width);
    AT_SetInt(ctx->handle, L"AOIHeight", (AT_64)height);
    refresh_geometry(ctx);
    return 0;
}

int andor3_clear_aoi(Andor3Ctx* ctx) {
    if (!ctx) return -1;
    AT_64 sw = 0, sh = 0;
    AT_GetInt(ctx->handle, L"SensorWidth",  &sw);
    AT_GetInt(ctx->handle, L"SensorHeight", &sh);
    AT_SetInt(ctx->handle, L"AOILeft",   1);
    AT_SetInt(ctx->handle, L"AOITop",    1);
    AT_SetInt(ctx->handle, L"AOIWidth",  sw);
    AT_SetInt(ctx->handle, L"AOIHeight", sh);
    refresh_geometry(ctx);
    return 0;
}

int andor3_get_aoi(Andor3Ctx* ctx, int* left, int* top, int* w, int* h) {
    if (!ctx) return -1;
    AT_64 l = 0, t = 0;
    AT_GetInt(ctx->handle, L"AOILeft",   &l);
    AT_GetInt(ctx->handle, L"AOITop",    &t);
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

    AT_U8* returned_buf = NULL;
    int    returned_size = 0;
    int rc = AT_WaitBuffer(ctx->handle, &returned_buf, &returned_size,
                           (unsigned int)timeout_ms);

    AT_Command(ctx->handle, L"AcquisitionStop");
    AT_Flush(ctx->handle);

    if (rc != AT_SUCCESS || !returned_buf) return -1;

    unpack_frame(ctx, returned_buf);
    return 0;
}

const uint8_t* andor3_get_frame_ptr(Andor3Ctx* ctx) {
    return ctx ? ctx->image_buf : NULL;
}

int andor3_get_frame_bytes(Andor3Ctx* ctx) {
    return ctx ? ctx->frame_bytes : 0;
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
    ctx->in_sequence = 1;
    return 0;
}

int andor3_get_next_frame(Andor3Ctx* ctx, int timeout_ms) {
    if (!ctx || !ctx->in_sequence) return -1;

    AT_U8* returned_buf = NULL;
    int    returned_size = 0;
    int rc = AT_WaitBuffer(ctx->handle, &returned_buf, &returned_size,
                           (unsigned int)timeout_ms);
    if (rc != AT_SUCCESS || !returned_buf) return -1;

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
