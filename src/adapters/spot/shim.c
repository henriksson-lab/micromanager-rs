/**
 * shim.c — Thin C wrapper around the Diagnostic Instruments SpotCam SDK.
 *
 * The SpotCam API is global (no camera handle): SpotSetValue selects the
 * active camera by device number.  This shim serialises all calls behind a
 * simple integer index and exposes a plain C API suitable for Rust FFI.
 *
 * Exposure unit: milliseconds in the public API.
 * SpotCam stores exposure as a struct with a duration in units of
 * SPOT_EXPOSUREINCREMENT nanoseconds (typically 1 ns).
 *
 * Platform notes:
 *   macOS: SpotCam.framework; SpotGetSequentialImages / SpotRetrieveSequentialImage
 *          take an extra `rowBytes` parameter.
 *   Windows: SpotCam.dll; same functions without the extra parameter.
 */

/* ── Platform detection ────────────────────────────────────────────────────── */

#ifdef _WIN32
#  define WIN32_LEAN_AND_MEAN
#  include <windows.h>
   static void shim_sleep_ms(int ms) { Sleep((DWORD)ms); }
#else
#  include <unistd.h>
#  ifdef __APPLE__
   static void shim_sleep_ms(int ms) { usleep((useconds_t)(ms) * 1000u); }
#  endif
#endif

/* ── SDK headers ───────────────────────────────────────────────────────────── */

#ifdef __APPLE__
#  include <SpotCam/SpotCam.h>
#endif

#ifdef _WIN32
   /* SpotCam.h normally lives alongside SpotCam.dll.  If the SDK root is set
      in the build script the include path is already handled; otherwise we
      attempt a default path. */
#  ifndef SPOT_H_INCLUDED
#    include <SpotCam.h>
#  endif
#endif

#include <stdlib.h>
#include <string.h>
#include <stdint.h>

/* ── SpotCtx struct ────────────────────────────────────────────────────────── */

typedef struct SpotCtx {
    int   device_index;     /* 0-based SpotCam device number */
    int   img_width;
    int   img_height;
    short bit_depth;        /* e.g. 8, 12, 16 */
    short binning;          /* SpotCam SPOT_BINSIZE (1 = full res) */
    int   gain_index;       /* SPOT_GAIN 1-based */

    /* Snap buffer (owned). */
    uint8_t* image_buf;
    size_t   image_buf_bytes;
    int      frame_bytes;   /* bytes in the last snapped frame */
} SpotCtx;

#if !defined(__APPLE__) && !defined(_WIN32)

/* Linux has no SpotCam SDK.  Keep the Rust feature buildable and let
   initialize() report zero devices instead of failing at C compile time. */
int spot_find_devices(void) { return 0; }

int spot_get_device_name(int idx, char* buf, int len) {
    (void)idx;
    if (buf && len > 0) buf[0] = '\0';
    return -1;
}

int spot_get_serial_number(int idx, char* buf, int len) {
    (void)idx;
    if (buf && len > 0) buf[0] = '\0';
    return -1;
}

SpotCtx* spot_open(int device_index) {
    (void)device_index;
    return NULL;
}

void spot_close(SpotCtx* ctx) {
    (void)ctx;
}

int spot_get_image_width(SpotCtx* ctx) {
    (void)ctx;
    return 0;
}

int spot_get_image_height(SpotCtx* ctx) {
    (void)ctx;
    return 0;
}

int spot_get_bit_depth(SpotCtx* ctx) {
    (void)ctx;
    return 0;
}

double spot_get_exposure_ms(SpotCtx* ctx) {
    (void)ctx;
    return 0.0;
}

int spot_set_exposure_ms(SpotCtx* ctx, double ms) {
    (void)ctx;
    (void)ms;
    return -1;
}

int spot_get_gain(SpotCtx* ctx) {
    (void)ctx;
    return 1;
}

int spot_set_gain(SpotCtx* ctx, int gain) {
    (void)ctx;
    (void)gain;
    return -1;
}

int spot_get_binning(SpotCtx* ctx) {
    (void)ctx;
    return 1;
}

int spot_set_binning(SpotCtx* ctx, int bin) {
    (void)ctx;
    (void)bin;
    return -1;
}

float spot_get_temperature_c(SpotCtx* ctx) {
    (void)ctx;
    return 0.0f;
}

int spot_get_gain_max(SpotCtx* ctx) {
    (void)ctx;
    return 1;
}

int spot_set_roi(SpotCtx* ctx, int x, int y, int w, int h) {
    (void)ctx;
    (void)x;
    (void)y;
    (void)w;
    (void)h;
    return -1;
}

int spot_clear_roi(SpotCtx* ctx) {
    (void)ctx;
    return -1;
}

int spot_snap(SpotCtx* ctx, int timeout_ms) {
    (void)ctx;
    (void)timeout_ms;
    return -1;
}

const uint8_t* spot_get_frame_ptr(SpotCtx* ctx) {
    (void)ctx;
    return NULL;
}

int spot_get_frame_bytes(SpotCtx* ctx) {
    (void)ctx;
    return 0;
}

#else

/* ── Helpers ──────────────────────────────────────────────────────────────── */

static void select_device(SpotCtx* ctx) {
    int idx = ctx->device_index;
    SpotSetValue(SPOT_DRIVERDEVICENUMBER, &idx);
}

static int ensure_buf(SpotCtx* ctx, size_t bytes) {
    if (bytes > ctx->image_buf_bytes) {
        free(ctx->image_buf);
        ctx->image_buf = (uint8_t*)malloc(bytes);
        if (!ctx->image_buf) { ctx->image_buf_bytes = 0; return -1; }
        ctx->image_buf_bytes = bytes;
    }
    return 0;
}

/* ── Device enumeration ────────────────────────────────────────────────────── */

/** Returns the number of SpotCam devices found, or -1 on error. */
int spot_find_devices(void) {
    DWORD count = 0;
    if (SpotFindDevices(&count) != 0) return -1;
    return (int)count;
}

/** Fill `buf` with the model description of device `idx`. */
int spot_get_device_name(int idx, char* buf, int len) {
    if (!buf || len <= 0) return -1;
    buf[0] = '\0';
    int save = idx;
    SpotSetValue(SPOT_DRIVERDEVICENUMBER, &save);
    SPOT_DEVICE_INFO info;
    memset(&info, 0, sizeof(info));
    if (SpotGetValue(SPOT_DEVICEINFO, &info) != 0) return -1;
    strncpy(buf, info.szDescription, (size_t)(len - 1));
    buf[len - 1] = '\0';
    return 0;
}

/** Fill `buf` with the serial number of device `idx`. */
int spot_get_serial_number(int idx, char* buf, int len) {
    if (!buf || len <= 0) return -1;
    buf[0] = '\0';
    int save = idx;
    SpotSetValue(SPOT_DRIVERDEVICENUMBER, &save);
    SPOT_DEVICE_INFO info;
    memset(&info, 0, sizeof(info));
    if (SpotGetValue(SPOT_DEVICEINFO, &info) != 0) return -1;
    strncpy(buf, info.szSerialNum, (size_t)(len - 1));
    buf[len - 1] = '\0';
    return 0;
}

/* ── Open / close ─────────────────────────────────────────────────────────── */

SpotCtx* spot_open(int device_index) {
    SpotCtx* ctx = (SpotCtx*)calloc(1, sizeof(SpotCtx));
    if (!ctx) return NULL;
    ctx->device_index = device_index;
    ctx->binning      = 1;
    ctx->gain_index   = 1;
    ctx->bit_depth    = 16;

    select_device(ctx);

    /* Read current image dimensions. */
    SPOT_IMAGERECT roi;
    memset(&roi, 0, sizeof(roi));
    if (SpotGetValue(SPOT_IMAGERECT, &roi) == 0) {
        ctx->img_width  = (int)(roi.right  - roi.left);
        ctx->img_height = (int)(roi.bottom - roi.top);
    }

    /* Read bit depth. */
    short bd = 16;
    if (SpotGetValue(SPOT_BITDEPTH, &bd) == 0) ctx->bit_depth = bd;

    /* Read binning. */
    short bn = 1;
    if (SpotGetValue(SPOT_BINSIZE, &bn) == 0) ctx->binning = bn;

    return ctx;
}

void spot_close(SpotCtx* ctx) {
    if (!ctx) return;
    free(ctx->image_buf);
    free(ctx);
}

/* ── Property getters / setters ───────────────────────────────────────────── */

int spot_get_image_width(SpotCtx* ctx)  { return ctx ? ctx->img_width  : 0; }
int spot_get_image_height(SpotCtx* ctx) { return ctx ? ctx->img_height : 0; }
int spot_get_bit_depth(SpotCtx* ctx)    { return ctx ? (int)ctx->bit_depth : 0; }

/** Exposure in milliseconds (converts to SPOT_EXPOSURE_STRUCT2 internally). */
double spot_get_exposure_ms(SpotCtx* ctx) {
    if (!ctx) return 0.0;
    select_device(ctx);
    SPOT_EXPOSURE_STRUCT2 exp;
    memset(&exp, 0, sizeof(exp));
    if (SpotGetValue(SPOT_EXPOSURE2, &exp) != 0) return 0.0;
    /* Duration in increments of SPOT_EXPOSUREINCREMENT nanoseconds. */
    DWORD inc_ns = 1;
    SpotGetValue(SPOT_EXPOSUREINCREMENT, &inc_ns);
    if (inc_ns == 0) inc_ns = 1;
    double ns = (double)exp.dwExpDur * (double)inc_ns;
    return ns / 1e6;   /* ns → ms */
}

int spot_set_exposure_ms(SpotCtx* ctx, double ms) {
    if (!ctx) return -1;
    select_device(ctx);
    DWORD inc_ns = 1;
    SpotGetValue(SPOT_EXPOSUREINCREMENT, &inc_ns);
    if (inc_ns == 0) inc_ns = 1;
    DWORD ticks = (DWORD)((ms * 1e6) / (double)inc_ns + 0.5);
    SPOT_EXPOSURE_STRUCT2 exp;
    memset(&exp, 0, sizeof(exp));
    exp.dwExpDur      = ticks;
    exp.dwRedExpDur   = ticks;
    exp.dwGreenExpDur = ticks;
    exp.dwBlueExpDur  = ticks;
    exp.nGain         = (short)ctx->gain_index;
    return SpotSetValue(SPOT_EXPOSURE2, &exp) == 0 ? 0 : -1;
}

int spot_get_gain(SpotCtx* ctx) {
    if (!ctx) return 1;
    select_device(ctx);
    short g = 1;
    SpotGetValue(SPOT_GAIN, &g);
    return (int)g;
}

int spot_set_gain(SpotCtx* ctx, int gain) {
    if (!ctx) return -1;
    select_device(ctx);
    ctx->gain_index = gain;
    short g = (short)gain;
    return SpotSetValue(SPOT_GAIN, &g) == 0 ? 0 : -1;
}

int spot_get_binning(SpotCtx* ctx) {
    if (!ctx) return 1;
    return (int)ctx->binning;
}

int spot_set_binning(SpotCtx* ctx, int bin) {
    if (!ctx) return -1;
    select_device(ctx);
    short b = (short)bin;
    if (SpotSetValue(SPOT_BINSIZE, &b) != 0) return -1;
    ctx->binning = b;
    /* Refresh image dims. */
    SPOT_IMAGERECT roi;
    if (SpotGetValue(SPOT_IMAGERECT, &roi) == 0) {
        ctx->img_width  = (int)(roi.right  - roi.left);
        ctx->img_height = (int)(roi.bottom - roi.top);
    }
    return 0;
}

/** Temperature in degrees Celsius (SpotCam reports Fahrenheit). */
float spot_get_temperature_c(SpotCtx* ctx) {
    if (!ctx) return 0.0f;
    select_device(ctx);
    float tf = 0.0f;
    SpotGetSensorCurrentTemperature(&tf);
    return (tf - 32.0f) * 5.0f / 9.0f;
}

int spot_get_gain_max(SpotCtx* ctx) {
    if (!ctx) return 1;
    select_device(ctx);
    short maxg = 1;
    SpotGetValue(SPOT_GAINMAX, &maxg);
    return (int)maxg;
}

/* ── ROI ──────────────────────────────────────────────────────────────────── */

int spot_set_roi(SpotCtx* ctx, int x, int y, int w, int h) {
    if (!ctx) return -1;
    select_device(ctx);
    SPOT_IMAGERECT roi;
    roi.left   = (short)x;
    roi.top    = (short)y;
    roi.right  = (short)(x + w);
    roi.bottom = (short)(y + h);
    if (SpotSetValue(SPOT_IMAGERECT, &roi) != 0) return -1;
    ctx->img_width  = w;
    ctx->img_height = h;
    return 0;
}

int spot_clear_roi(SpotCtx* ctx) {
    if (!ctx) return -1;
    select_device(ctx);
    /* Setting SPOT_IMAGERECT to all zeros resets to full sensor. */
    SPOT_IMAGERECT roi;
    memset(&roi, 0, sizeof(roi));
    if (SpotSetValue(SPOT_IMAGERECT, &roi) != 0) return -1;
    /* Re-read actual dimensions. */
    if (SpotGetValue(SPOT_IMAGERECT, &roi) == 0) {
        ctx->img_width  = (int)(roi.right  - roi.left);
        ctx->img_height = (int)(roi.bottom - roi.top);
    }
    return 0;
}

/* ── Snap ─────────────────────────────────────────────────────────────────── */

/**
 * Snap one frame.  Returns 0 on success, -1 on error.
 * timeout_ms is ignored (SpotCam is always synchronous).
 */
int spot_snap(SpotCtx* ctx, int timeout_ms) {
    (void)timeout_ms;
    if (!ctx) return -1;
    select_device(ctx);

    int bpp = (ctx->bit_depth > 8) ? 2 : 1;
    size_t bytes = (size_t)ctx->img_width * (size_t)ctx->img_height * (size_t)bpp;
    if (bytes == 0) return -1;
    if (ensure_buf(ctx, bytes) != 0) return -1;

#ifdef __APPLE__
    DWORD row_bytes = (DWORD)(ctx->img_width * bpp);
    int ret = SpotGetSequentialImages(
        1,                          /* nImages */
        SPOT_INTERVALSHORTASPOSSIBLE,
        ctx->bit_depth,
        FALSE,                      /* bUseFlash */
        FALSE,                      /* bOpenShutter */
        row_bytes                   /* macOS extra param */
    );
#else
    int ret = SpotGetSequentialImages(
        1,
        SPOT_INTERVALSHORTASPOSSIBLE,
        ctx->bit_depth,
        FALSE,
        FALSE
    );
#endif
    if (ret != 0) return -1;

    /* Poll until image ready. */
    DWORD status = 0;
    int elapsed = 0;
    for (;;) {
        SpotQueryStatus(SPOT_STATUSSEQIMAGEREADY, &status);
        if (status) break;
        shim_sleep_ms(1);
        elapsed++;
        if (elapsed > 60000) return -1; /* 60 s hard timeout */
    }

#ifdef __APPLE__
    DWORD rb = (DWORD)(ctx->img_width * bpp);
    ret = SpotRetrieveSequentialImage(ctx->image_buf, rb);
#else
    ret = SpotRetrieveSequentialImage(ctx->image_buf);
#endif
    if (ret != 0) return -1;

    ctx->frame_bytes = (int)bytes;
    return 0;
}

const uint8_t* spot_get_frame_ptr(SpotCtx* ctx) {
    return ctx ? ctx->image_buf : NULL;
}

int spot_get_frame_bytes(SpotCtx* ctx) {
    return ctx ? ctx->frame_bytes : 0;
}

#endif
