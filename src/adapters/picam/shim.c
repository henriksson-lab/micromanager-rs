/**
 * shim.c — Thin C wrapper around the PVCAM C API.
 *
 * Exposes a simplified opaque-context API so that Rust can interface with
 * Princeton Instruments cameras without needing to know PVCAM parameter IDs
 * or internal types.
 *
 * PVCAM is a pure C library; no C++ is used here.
 */

/* ── Platform includes ────────────────────────────────────────────────────── */

#ifdef __APPLE__
#  include <PICAM/master.h>
#  include <PICAM/pvcam.h>
#elif defined(__linux__)
#  if __has_include(<pvcam/master.h>)
#    include <pvcam/master.h>
#    include <pvcam/pvcam.h>
#  else
#    include <master.h>
#    include <pvcam.h>
#  endif
#else
/* Windows — PICAM / PVCAM headers */
#  include "picam.h"
#  include "picam_advanced.h"
#endif

#include <stdlib.h>
#include <string.h>
#include <stdint.h>

#ifdef __unix__
#  include <unistd.h>   /* usleep */
#endif
#ifdef _WIN32
#  include <windows.h>  /* Sleep */
static void usleep(unsigned long us) { Sleep((DWORD)(us / 1000 + 1)); }
#endif

/* ── Context struct ───────────────────────────────────────────────────────── */

typedef struct PvcamCtx {
    int16     hcam;

    /* Full sensor dimensions (read once on open). */
    uns16     sensor_width;
    uns16     sensor_height;

    /* Current ROI / binning (set before each snap or start_cont). */
    uns16     s1, s2, sbin;
    uns16     p1, p2, pbin;

    /* Derived image dimensions for the current ROI. */
    uns16     img_width;
    uns16     img_height;

    /* Internal buffers. */
    uint8_t*  snap_buf;
    uint32_t  snap_buf_size;
    uint8_t*  cont_buf;
    uint32_t  cont_buf_size;
    uint32_t  frame_size;    /* bytes per frame for current setup */

    /* State flags. */
    int       cont_running;
} PvcamCtx;

/* ── Internal helpers ─────────────────────────────────────────────────────── */

static void update_img_dims(PvcamCtx* ctx) {
    ctx->img_width  = (uns16)((ctx->s2 - ctx->s1 + 1) / ctx->sbin);
    ctx->img_height = (uns16)((ctx->p2 - ctx->p1 + 1) / ctx->pbin);
}

static int get_int16(int16 hcam, uns32 param_id, int16* out) {
    return pl_get_param(hcam, param_id, ATTR_CURRENT, (void*)out) ? 0 : -1;
}

static int get_uns16(int16 hcam, uns32 param_id, uns16* out) {
    return pl_get_param(hcam, param_id, ATTR_CURRENT, (void*)out) ? 0 : -1;
}

static int get_uns32(int16 hcam, uns32 param_id, uns32* out) {
    return pl_get_param(hcam, param_id, ATTR_CURRENT, (void*)out) ? 0 : -1;
}

/* ── Library lifecycle ────────────────────────────────────────────────────── */

int pvcam_init(void) {
    return pl_pvcam_init() ? 0 : -1;
}

void pvcam_uninit(void) {
    pl_pvcam_uninit();
}

/* ── Camera enumeration ───────────────────────────────────────────────────── */

int pvcam_get_camera_count(void) {
    int16 count = 0;
    if (!pl_cam_get_total(&count)) return -1;
    return (int)count;
}

int pvcam_get_camera_name(int idx, char* buf, int len) {
    char name[CAM_NAME_LEN + 1];
    if (!pl_cam_get_name((int16)idx, name)) return -1;
    strncpy(buf, name, (size_t)(len - 1));
    buf[len - 1] = '\0';
    return 0;
}

/* ── Camera open/close ────────────────────────────────────────────────────── */

PvcamCtx* pvcam_open(const char* name) {
    int16 hcam = -1;
    /* PVCAM wants a non-const char* for the name — cast is safe. */
    if (!pl_cam_open((char*)name, &hcam, OPEN_EXCLUSIVE)) return NULL;

    PvcamCtx* ctx = (PvcamCtx*)calloc(1, sizeof(PvcamCtx));
    if (!ctx) { pl_cam_close(hcam); return NULL; }
    ctx->hcam = hcam;

    /* Read full sensor dimensions. */
    uns16 w = 0, h = 0;
    get_uns16(hcam, PARAM_SER_SIZE, &w);
    get_uns16(hcam, PARAM_PAR_SIZE, &h);
    ctx->sensor_width  = w;
    ctx->sensor_height = h;

    /* Default ROI: full frame, binning 1. */
    ctx->s1   = 0;
    ctx->s2   = (uns16)(w > 0 ? w - 1 : 0);
    ctx->sbin = 1;
    ctx->p1   = 0;
    ctx->p2   = (uns16)(h > 0 ? h - 1 : 0);
    ctx->pbin = 1;
    update_img_dims(ctx);

    return ctx;
}

void pvcam_close(PvcamCtx* ctx) {
    if (!ctx) return;
    if (ctx->cont_running) {
        pl_exp_abort(ctx->hcam, CCS_HALT);
        ctx->cont_running = 0;
    }
    pl_cam_close(ctx->hcam);
    free(ctx->snap_buf);
    free(ctx->cont_buf);
    free(ctx);
}

/* ── Read-only camera info ────────────────────────────────────────────────── */

uint16_t pvcam_get_sensor_width(PvcamCtx* ctx) {
    return ctx ? (uint16_t)ctx->sensor_width : 0;
}

uint16_t pvcam_get_sensor_height(PvcamCtx* ctx) {
    return ctx ? (uint16_t)ctx->sensor_height : 0;
}

uint16_t pvcam_get_image_width(PvcamCtx* ctx) {
    return ctx ? (uint16_t)ctx->img_width : 0;
}

uint16_t pvcam_get_image_height(PvcamCtx* ctx) {
    return ctx ? (uint16_t)ctx->img_height : 0;
}

int pvcam_get_bit_depth(PvcamCtx* ctx) {
    if (!ctx) return -1;
    int16 bd = 0;
    if (get_int16(ctx->hcam, PARAM_BIT_DEPTH, &bd) != 0) return -1;
    return (int)bd;
}

int pvcam_get_serial_number(PvcamCtx* ctx, char* buf, int len) {
    if (!ctx || !buf) return -1;
    char tmp[MAX_ALPHA_SER_NUM_LEN + 1];
    memset(tmp, 0, sizeof(tmp));
    if (!pl_get_param(ctx->hcam, PARAM_HEAD_SER_NUM_ALPHA, ATTR_CURRENT, (void*)tmp))
        return -1;
    strncpy(buf, tmp, (size_t)(len - 1));
    buf[len - 1] = '\0';
    return 0;
}

int pvcam_get_chip_name(PvcamCtx* ctx, char* buf, int len) {
    if (!ctx || !buf) return -1;
    char tmp[CCD_NAME_LEN + 1];
    memset(tmp, 0, sizeof(tmp));
    if (!pl_get_param(ctx->hcam, PARAM_CHIP_NAME, ATTR_CURRENT, (void*)tmp))
        return -1;
    strncpy(buf, tmp, (size_t)(len - 1));
    buf[len - 1] = '\0';
    return 0;
}

/* ── Gain ─────────────────────────────────────────────────────────────────── */

int pvcam_get_gain_index(PvcamCtx* ctx) {
    if (!ctx) return -1;
    int16 g = 1;
    if (get_int16(ctx->hcam, PARAM_GAIN_INDEX, &g) != 0) return -1;
    return (int)g;
}

int pvcam_get_gain_max(PvcamCtx* ctx) {
    if (!ctx) return -1;
    int16 max = 1;
    if (!pl_get_param(ctx->hcam, PARAM_GAIN_INDEX, ATTR_MAX, (void*)&max)) return -1;
    return (int)max;
}

int pvcam_set_gain_index(PvcamCtx* ctx, int idx) {
    if (!ctx) return -1;
    int16 g = (int16)idx;
    return pl_set_param(ctx->hcam, PARAM_GAIN_INDEX, (void*)&g) ? 0 : -1;
}

/* ── Temperature ──────────────────────────────────────────────────────────── */

/* PVCAM stores temperature in hundredths of a degree Celsius (int16). */
double pvcam_get_temperature(PvcamCtx* ctx) {
    if (!ctx) return -273.15;
    int16 raw = 0;
    if (!pl_get_param(ctx->hcam, PARAM_TEMP, ATTR_CURRENT, (void*)&raw))
        return -273.15;
    return (double)raw / 100.0;
}

double pvcam_get_temp_setpoint(PvcamCtx* ctx) {
    if (!ctx) return -273.15;
    int16 raw = 0;
    if (!pl_get_param(ctx->hcam, PARAM_TEMP_SETPOINT, ATTR_CURRENT, (void*)&raw))
        return -273.15;
    return (double)raw / 100.0;
}

int pvcam_set_temp_setpoint(PvcamCtx* ctx, double celsius) {
    if (!ctx) return -1;
    int16 raw = (int16)(celsius * 100.0);
    return pl_set_param(ctx->hcam, PARAM_TEMP_SETPOINT, (void*)&raw) ? 0 : -1;
}

/* ── ROI / binning ────────────────────────────────────────────────────────── */

void pvcam_set_roi(PvcamCtx* ctx,
                   uint16_t x, uint16_t y, uint16_t w, uint16_t h,
                   uint16_t xbin, uint16_t ybin) {
    if (!ctx) return;
    ctx->s1   = (uns16)x;
    ctx->s2   = (uns16)(x + w - 1);
    ctx->sbin = (uns16)(xbin < 1 ? 1 : xbin);
    ctx->p1   = (uns16)y;
    ctx->p2   = (uns16)(y + h - 1);
    ctx->pbin = (uns16)(ybin < 1 ? 1 : ybin);
    /* Clamp to sensor boundaries. */
    if (ctx->s2 >= ctx->sensor_width  && ctx->sensor_width  > 0)
        ctx->s2 = (uns16)(ctx->sensor_width  - 1);
    if (ctx->p2 >= ctx->sensor_height && ctx->sensor_height > 0)
        ctx->p2 = (uns16)(ctx->sensor_height - 1);
    update_img_dims(ctx);
}

void pvcam_clear_roi(PvcamCtx* ctx) {
    if (!ctx) return;
    ctx->s1   = 0;
    ctx->s2   = (uns16)(ctx->sensor_width  > 0 ? ctx->sensor_width  - 1 : 0);
    ctx->sbin = 1;
    ctx->p1   = 0;
    ctx->p2   = (uns16)(ctx->sensor_height > 0 ? ctx->sensor_height - 1 : 0);
    ctx->pbin = 1;
    update_img_dims(ctx);
}

/* ── Snap (single frame, blocking) ───────────────────────────────────────── */

int pvcam_snap(PvcamCtx* ctx, uint32_t exp_ms, uint32_t timeout_ms) {
    if (!ctx) return -1;
    if (ctx->cont_running) return -1;   /* must stop sequence first */

    rgn_type roi;
    roi.s1   = ctx->s1;
    roi.s2   = ctx->s2;
    roi.sbin = ctx->sbin;
    roi.p1   = ctx->p1;
    roi.p2   = ctx->p2;
    roi.pbin = ctx->pbin;

    uns32 bytes = 0;
    if (!pl_exp_setup_seq(ctx->hcam, 1, 1, &roi, TIMED_MODE,
                          (uns32)exp_ms, &bytes))
        return -1;

    /* (Re-)allocate snap buffer if needed. */
    if (bytes > ctx->snap_buf_size) {
        free(ctx->snap_buf);
        ctx->snap_buf = (uint8_t*)malloc((size_t)bytes);
        if (!ctx->snap_buf) { ctx->snap_buf_size = 0; return -1; }
        ctx->snap_buf_size = bytes;
    }
    ctx->frame_size = bytes;

    if (!pl_exp_start_seq(ctx->hcam, (void*)ctx->snap_buf)) return -1;

    /* Poll until done or timeout. */
    int16  status   = READOUT_NOT_ACTIVE;
    uns32  byte_cnt = 0;
    uns32  elapsed  = 0;
    while (elapsed < timeout_ms) {
        if (!pl_exp_check_status(ctx->hcam, &status, &byte_cnt)) break;
        if (status == READOUT_COMPLETE) break;
        if (status == READOUT_FAILED)   { pl_exp_finish_seq(ctx->hcam, ctx->snap_buf, 0); return -1; }
        usleep(1000);   /* 1 ms */
        elapsed += 1;
    }

    pl_exp_finish_seq(ctx->hcam, (void*)ctx->snap_buf, 0);

    if (status != READOUT_COMPLETE) return -1;

    /* Update derived image dimensions from the ROI. */
    update_img_dims(ctx);
    return 0;
}

const void* pvcam_get_snap_frame(PvcamCtx* ctx) {
    return ctx ? (const void*)ctx->snap_buf : NULL;
}

uint32_t pvcam_get_frame_size(PvcamCtx* ctx) {
    return ctx ? ctx->frame_size : 0;
}

/* ── Continuous acquisition ───────────────────────────────────────────────── */

int pvcam_start_cont(PvcamCtx* ctx, uint32_t exp_ms, int num_frames) {
    if (!ctx || ctx->cont_running) return -1;

    rgn_type roi;
    roi.s1   = ctx->s1;
    roi.s2   = ctx->s2;
    roi.sbin = ctx->sbin;
    roi.p1   = ctx->p1;
    roi.p2   = ctx->p2;
    roi.pbin = ctx->pbin;

    uns32 frame_bytes = 0;
    if (!pl_exp_setup_cont(ctx->hcam, 1, &roi, TIMED_MODE,
                           (uns32)exp_ms, &frame_bytes, CIRC_OVERWRITE))
        return -1;

    uns32 buf_size = (uns32)(num_frames < 2 ? 2 : num_frames) * frame_bytes;
    if (buf_size > ctx->cont_buf_size) {
        free(ctx->cont_buf);
        ctx->cont_buf = (uint8_t*)malloc((size_t)buf_size);
        if (!ctx->cont_buf) { ctx->cont_buf_size = 0; return -1; }
        ctx->cont_buf_size = buf_size;
    }
    ctx->frame_size = frame_bytes;
    update_img_dims(ctx);

    if (!pl_exp_start_cont(ctx->hcam, (void*)ctx->cont_buf, buf_size))
        return -1;

    ctx->cont_running = 1;
    return 0;
}

/**
 * Get a pointer to the oldest unread frame in the circular buffer.
 * Returns 0 on success; the pointer is valid until pvcam_release_frame_cont().
 */
int pvcam_get_frame_cont(PvcamCtx* ctx, const void** frame_out) {
    if (!ctx || !ctx->cont_running || !frame_out) return -1;
    void* frame = NULL;
    if (!pl_exp_get_oldest_frame(ctx->hcam, &frame)) return -1;
    *frame_out = frame;
    return 0;
}

int pvcam_release_frame_cont(PvcamCtx* ctx) {
    if (!ctx) return -1;
    return pl_exp_unlock_oldest_frame(ctx->hcam) ? 0 : -1;
}

int pvcam_stop_cont(PvcamCtx* ctx) {
    if (!ctx) return -1;
    if (!ctx->cont_running) return 0;
    pl_exp_abort(ctx->hcam, CCS_HALT);
    ctx->cont_running = 0;
    return 0;
}

/* ── Error ────────────────────────────────────────────────────────────────── */

int pvcam_get_error_message(char* buf, int len) {
    if (!buf || len <= 0) return -1;
    int16 code = pl_error_code();
    char  msg[ERROR_MSG_LEN + 1];
    memset(msg, 0, sizeof(msg));
    pl_error_message(code, msg);
    strncpy(buf, msg, (size_t)(len - 1));
    buf[len - 1] = '\0';
    return 0;
}
