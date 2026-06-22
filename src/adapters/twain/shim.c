/**
 * shim.c — Thin C wrapper around the TWAIN 2.x DSM API.
 *
 * Exposes a simplified synchronous "snap" interface suitable for Rust FFI.
 *
 * Platform support:
 *   Windows — TWAINDSM.dll loaded at runtime via LoadLibrary; hidden HWND
 *             used for TWAIN message routing; image delivered as Win32 DIB
 *             (HGLOBAL) which is unpacked to a compact pixel buffer.
 *   Linux   — libtwaindsm.so loaded at runtime via dlopen; HWND is NULL
 *             (many Linux TWAIN sources support message-less operation).
 *
 * TWAIN state machine (abbreviated):
 *   1 PreSession → 2 DSM Loaded → 3 DSM Open → 4 Source Open →
 *   5 Source Enabled → 6 Transfer Ready → 7 Transferring → (back to 5)
 *
 * Thread safety: none — all calls must be made from the same thread that
 * called twain_init() (required by the TWAIN spec for Win32 message routing).
 */

/* ── Platform ──────────────────────────────────────────────────────────────── */

#ifdef _WIN32
#  define WIN32_LEAN_AND_MEAN
#  include <windows.h>
   typedef HMODULE lib_handle_t;
   static void shim_sleep_ms(int ms) { Sleep((DWORD)ms); }
#else
#  include <dlfcn.h>
#  include <unistd.h>
#  include <wchar.h>
   typedef void* lib_handle_t;
   static void shim_sleep_ms(int ms) { usleep((useconds_t)(ms) * 1000u); }
   /* Stub HWND for Linux where TWAIN message routing is optional. */
   typedef void* HWND;
   typedef unsigned int UINT;
#  ifndef FALSE
#    define FALSE 0
#  endif
#endif

/* ── TWAIN header ──────────────────────────────────────────────────────────── */

/* twain.h from TWAIN Working Group (BSD-licensed).  Included from the
   reference source tree via the build.rs include path. */
#ifdef _WIN32
#  include "TWAIN.H"
#else
#  include "twain.h"
#endif

#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdio.h>

/* ── Type alias ────────────────────────────────────────────────────────────── */

typedef TW_UINT16 (FAR PASCAL *DSMENTRYPROC)(
    pTW_IDENTITY, pTW_IDENTITY,
    TW_UINT32, TW_UINT16, TW_UINT16, TW_MEMREF);

/* ── Global DSM state ──────────────────────────────────────────────────────── */

static lib_handle_t  g_dsm_lib   = NULL;
static DSMENTRYPROC  g_dsm_entry = NULL;
static HWND          g_hwnd      = NULL;
static TW_IDENTITY   g_app_id;
static int           g_dsm_open  = 0;

/* ── Helpers ──────────────────────────────────────────────────────────────── */

static TW_UINT16 dsm_call(
        pTW_IDENTITY pDest,
        TW_UINT32 dg, TW_UINT16 dat, TW_UINT16 msg,
        TW_MEMREF pData)
{
    return g_dsm_entry(&g_app_id, pDest, dg, dat, msg, pData);
}

/* ── TwainCtx ─────────────────────────────────────────────────────────────── */

typedef struct TwainCtx {
    TW_IDENTITY src_id;
    int         src_open;

    /* Image buffer (owned). */
    uint8_t*    image_buf;
    size_t      image_buf_bytes;
    int         frame_bytes;

    /* Current image dimensions & format. */
    int         img_width;
    int         img_height;
    int         bytes_per_pixel;
    int         bit_depth;
} TwainCtx;

/* ── Hidden window (Windows only) ─────────────────────────────────────────── */

#ifdef _WIN32
static LRESULT CALLBACK TwainWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    (void)hwnd; (void)wp; (void)lp;
    /* TWAIN messages are dispatched to this window by DSM; we do nothing
       special — all processing happens in the PeekMessage loop in twain_snap. */
    return DefWindowProcA(hwnd, msg, wp, lp);
}

static HWND create_hidden_window(void) {
    WNDCLASSA wc;
    memset(&wc, 0, sizeof(wc));
    wc.lpfnWndProc   = TwainWndProc;
    wc.hInstance     = GetModuleHandleA(NULL);
    wc.lpszClassName = "TwainShimWindow";
    RegisterClassA(&wc);   /* ignore error — may already be registered */

    return CreateWindowA(
        "TwainShimWindow", "",
        WS_POPUP,
        0, 0, 1, 1,
        NULL, NULL,
        GetModuleHandleA(NULL),
        NULL);
}
#endif /* _WIN32 */

/* ── DSM initialisation / cleanup ─────────────────────────────────────────── */

/**
 * Load the TWAIN DSM and open it.  Must be called once before any other
 * twain_* function.  Returns 0 on success, -1 on error.
 */
int twain_init(void) {
    if (g_dsm_open) return 0;

#ifdef _WIN32
    g_dsm_lib = LoadLibraryA("TWAINDSM.dll");
    if (!g_dsm_lib) {
        /* Fall back to legacy twain_32.dll on some older systems. */
        g_dsm_lib = LoadLibraryA("twain_32.dll");
    }
    if (!g_dsm_lib) return -1;
    g_dsm_entry = (DSMENTRYPROC)GetProcAddress(g_dsm_lib, "DSM_Entry");
#else
    g_dsm_lib = dlopen("libtwaindsm.so", RTLD_LAZY);
    if (!g_dsm_lib) return -1;
    g_dsm_entry = (DSMENTRYPROC)dlsym(g_dsm_lib, "DSM_Entry");
#endif

    if (!g_dsm_entry) {
#ifdef _WIN32
        FreeLibrary(g_dsm_lib);
#else
        dlclose(g_dsm_lib);
#endif
        g_dsm_lib = NULL;
        return -1;
    }

    /* Build application identity. */
    memset(&g_app_id, 0, sizeof(g_app_id));
    g_app_id.Id                    = 0;   /* DSM assigns real id */
    g_app_id.Version.MajorNum      = 1;
    g_app_id.Version.MinorNum      = 0;
    g_app_id.Version.Language      = TWLG_USA;
    g_app_id.Version.Country       = TWCY_USA;
    strncpy(g_app_id.Version.Info,  "1.0", sizeof(g_app_id.Version.Info) - 1);
    g_app_id.ProtocolMajor         = TWON_PROTOCOLMAJOR;
    g_app_id.ProtocolMinor         = TWON_PROTOCOLMINOR;
    g_app_id.SupportedGroups       = DF_APP2 | DG_IMAGE | DG_CONTROL;
    strncpy(g_app_id.Manufacturer,  "MicroManager", sizeof(g_app_id.Manufacturer) - 1);
    strncpy(g_app_id.ProductFamily, "Generic",      sizeof(g_app_id.ProductFamily) - 1);
    strncpy(g_app_id.ProductName,   "mm-twain",     sizeof(g_app_id.ProductName)   - 1);

    /* On Windows, TWAIN requires a parent HWND for MSG_OPENDSM. */
#ifdef _WIN32
    g_hwnd = create_hidden_window();
    if (!g_hwnd) {
        FreeLibrary(g_dsm_lib);
        g_dsm_lib = NULL;
        return -1;
    }
    TW_UINT16 rc = dsm_call(NULL, DG_CONTROL, DAT_PARENT, MSG_OPENDSM, (TW_MEMREF)&g_hwnd);
#else
    TW_UINT16 rc = dsm_call(NULL, DG_CONTROL, DAT_PARENT, MSG_OPENDSM, (TW_MEMREF)&g_hwnd);
#endif

    if (rc != TWRC_SUCCESS) {
#ifdef _WIN32
        DestroyWindow(g_hwnd); g_hwnd = NULL;
        FreeLibrary(g_dsm_lib);
#else
        dlclose(g_dsm_lib);
#endif
        g_dsm_lib = NULL;
        return -1;
    }

    g_dsm_open = 1;
    return 0;
}

void twain_close_dsm(void) {
    if (!g_dsm_open) return;
    dsm_call(NULL, DG_CONTROL, DAT_PARENT, MSG_CLOSEDSM, (TW_MEMREF)&g_hwnd);
    g_dsm_open = 0;

#ifdef _WIN32
    if (g_hwnd) { DestroyWindow(g_hwnd); g_hwnd = NULL; }
    if (g_dsm_lib) { FreeLibrary(g_dsm_lib); g_dsm_lib = NULL; }
#else
    if (g_dsm_lib) { dlclose(g_dsm_lib); g_dsm_lib = NULL; }
#endif
}

/* ── Source enumeration ───────────────────────────────────────────────────── */

/**
 * Fill `buf` with newline-separated TWAIN source ProductName strings.
 * Returns the count of sources found, or -1 on error.
 */
int twain_find_sources(char* buf, int len) {
    if (!g_dsm_open || !buf || len <= 0) return -1;
    buf[0] = '\0';

    TW_IDENTITY src;
    memset(&src, 0, sizeof(src));

    TW_UINT16 rc = dsm_call(NULL, DG_CONTROL, DAT_IDENTITY, MSG_GETFIRST,
                             (TW_MEMREF)&src);
    if (rc == TWRC_FAILURE) return 0;
    if (rc != TWRC_SUCCESS) return -1;

    int count = 0;
    do {
        int remaining = len - (int)strlen(buf) - 1;
        if (remaining <= 0) break;
        if (count > 0) strncat(buf, "\n", (size_t)remaining);
        strncat(buf, src.ProductName, (size_t)(remaining - 1));
        count++;

        memset(&src, 0, sizeof(src));
        rc = dsm_call(NULL, DG_CONTROL, DAT_IDENTITY, MSG_GETNEXT,
                      (TW_MEMREF)&src);
    } while (rc == TWRC_SUCCESS);

    return count;
}

/* ── Open / close source ──────────────────────────────────────────────────── */

/**
 * Open a TWAIN source by ProductName.  Pass NULL or "" to open the default
 * source.  Returns an opaque TwainCtx* or NULL on error.
 */
TwainCtx* twain_open(const char* source_name) {
    if (!g_dsm_open) return NULL;

    TW_IDENTITY src;
    memset(&src, 0, sizeof(src));

    if (!source_name || source_name[0] == '\0') {
        /* Open the default source. */
        if (dsm_call(NULL, DG_CONTROL, DAT_IDENTITY, MSG_GETDEFAULT,
                     (TW_MEMREF)&src) != TWRC_SUCCESS)
            return NULL;
    } else {
        /* Find source by name. */
        TW_UINT16 rc = dsm_call(NULL, DG_CONTROL, DAT_IDENTITY, MSG_GETFIRST,
                                 (TW_MEMREF)&src);
        int found = 0;
        while (rc == TWRC_SUCCESS) {
            if (strcmp(src.ProductName, source_name) == 0) { found = 1; break; }
            rc = dsm_call(NULL, DG_CONTROL, DAT_IDENTITY, MSG_GETNEXT,
                          (TW_MEMREF)&src);
        }
        if (!found) return NULL;
    }

    if (dsm_call(NULL, DG_CONTROL, DAT_IDENTITY, MSG_OPENDS,
                 (TW_MEMREF)&src) != TWRC_SUCCESS)
        return NULL;

    TwainCtx* ctx = (TwainCtx*)calloc(1, sizeof(TwainCtx));
    if (!ctx) {
        dsm_call(&src, DG_CONTROL, DAT_IDENTITY, MSG_CLOSEDS, (TW_MEMREF)&src);
        return NULL;
    }
    ctx->src_id   = src;
    ctx->src_open = 1;
    return ctx;
}

void twain_close(TwainCtx* ctx) {
    if (!ctx) return;
    if (ctx->src_open) {
        dsm_call(NULL, DG_CONTROL, DAT_IDENTITY, MSG_CLOSEDS,
                 (TW_MEMREF)&ctx->src_id);
    }
    free(ctx->image_buf);
    free(ctx);
}

/* ── Property getters ─────────────────────────────────────────────────────── */

int twain_get_image_width(TwainCtx* ctx)       { return ctx ? ctx->img_width       : 0; }
int twain_get_image_height(TwainCtx* ctx)      { return ctx ? ctx->img_height      : 0; }
int twain_get_bytes_per_pixel(TwainCtx* ctx)   { return ctx ? ctx->bytes_per_pixel : 1; }
int twain_get_bit_depth(TwainCtx* ctx)         { return ctx ? ctx->bit_depth       : 8; }

const char* twain_get_source_name(TwainCtx* ctx) {
    return ctx ? ctx->src_id.ProductName : "";
}

/* ── Snap ─────────────────────────────────────────────────────────────────── */

/**
 * Acquire one image from the TWAIN source.
 * Returns 0 on success, -1 on error/timeout.
 *
 * Implementation notes:
 *   1. Enable source in non-UI mode (ShowUI = FALSE).
 *   2. Run a local PeekMessage loop, routing each message through
 *      DAT_EVENT/MSG_PROCESSEVENT until MSG_XFERDONE or MSG_CLOSEDSREQ.
 *   3. Transfer image via DAT_IMAGENATIVEXFER (returns HGLOBAL DIB on
 *      Windows).  Unpack the DIB, stripping the 4-byte row alignment.
 *   4. End transfer with MSG_ENDXFER; disable source.
 */
int twain_snap(TwainCtx* ctx, int timeout_ms) {
    if (!ctx || !ctx->src_open) return -1;

    /* Enable source without UI. */
    TW_USERINTERFACE ui;
    memset(&ui, 0, sizeof(ui));
    ui.ShowUI   = FALSE;
    ui.ModalUI  = FALSE;
    ui.hParent  = (TW_HANDLE)g_hwnd;

    if (dsm_call(&ctx->src_id, DG_CONTROL, DAT_USERINTERFACE, MSG_ENABLEDS,
                 (TW_MEMREF)&ui) != TWRC_SUCCESS)
        return -1;

    /* ── Message loop: wait for transfer-ready notification ── */
    int xfer_done = 0;
    int elapsed   = 0;

#ifdef _WIN32
    MSG msg;
    while (elapsed < timeout_ms) {
        if (PeekMessageA(&msg, NULL, 0, 0, PM_REMOVE)) {
            TW_EVENT ev;
            ev.pEvent   = (TW_MEMREF)&msg;
            ev.TWMessage = MSG_NULL;
            TW_UINT16 rc = dsm_call(&ctx->src_id, DG_CONTROL, DAT_EVENT,
                                     MSG_PROCESSEVENT, (TW_MEMREF)&ev);
            if (ev.TWMessage == MSG_XFERDONE) { xfer_done = 1; break; }
            if (ev.TWMessage == MSG_CLOSEDSREQ) { break; }
            if (rc != TWRC_DSEVENT) {
                TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }
        } else {
            shim_sleep_ms(1);
            elapsed++;
        }
    }
#else
    /* On Linux without a message queue, poll the DSM with a NULL event. */
    while (elapsed < timeout_ms) {
        /* Some Linux sources signal readiness via the callback registered
           through DAT_CALLBACK; without that mechanism we fall back to
           polling DAT_IMAGENATIVEXFER directly. */
        TW_IMAGEINFO info;
        memset(&info, 0, sizeof(info));
        TW_UINT16 rc = dsm_call(&ctx->src_id, DG_IMAGE, DAT_IMAGEINFO,
                                 MSG_GET, (TW_MEMREF)&info);
        if (rc == TWRC_SUCCESS) { xfer_done = 1; break; }
        shim_sleep_ms(10);
        elapsed += 10;
    }
#endif

    if (!xfer_done) {
        /* Disable source and bail. */
        memset(&ui, 0, sizeof(ui));
        dsm_call(&ctx->src_id, DG_CONTROL, DAT_USERINTERFACE, MSG_DISABLEDS,
                 (TW_MEMREF)&ui);
        return -1;
    }

    /* ── Image info ── */
    TW_IMAGEINFO info;
    memset(&info, 0, sizeof(info));
    dsm_call(&ctx->src_id, DG_IMAGE, DAT_IMAGEINFO, MSG_GET, (TW_MEMREF)&info);

    ctx->img_width      = (int)info.ImageWidth;
    ctx->img_height     = (int)info.ImageLength;
    ctx->bit_depth      = (int)info.BitsPerPixel;
    ctx->bytes_per_pixel = (ctx->bit_depth + 7) / 8;

    /* ── Native transfer — returns a Win32 HGLOBAL DIB on Windows ── */
#ifdef _WIN32
    TW_UINT32 hBitmap = 0;   /* HGLOBAL returned as TW_UINT32 */
    TW_UINT16 xrc = dsm_call(&ctx->src_id, DG_IMAGE, DAT_IMAGENATIVEXFER,
                              MSG_GET, (TW_MEMREF)&hBitmap);
    if (xrc != TWRC_XFERDONE || !hBitmap) goto fail_disable;

    {
        LPBITMAPINFOHEADER pHead = (LPBITMAPINFOHEADER)GlobalLock((HGLOBAL)(uintptr_t)hBitmap);
        if (!pHead) { GlobalFree((HGLOBAL)(uintptr_t)hBitmap); goto fail_disable; }

        int w         = (int)pHead->biWidth;
        int h         = (int)pHead->biHeight;
        int bpp       = (int)(pHead->biBitCount / 8);
        /* DIB rows are padded to 4-byte alignment. */
        int row_bytes = ((pHead->biBitCount * w + 31) / 32) * 4;

        /* Palette offset. */
        int palette_entries = 0;
        if (pHead->biBitCount == 1)       palette_entries = 2;
        else if (pHead->biBitCount == 4)  palette_entries = 16;
        else if (pHead->biBitCount == 8)  palette_entries = 256;
        unsigned char* pBits = (unsigned char*)pHead
                               + sizeof(BITMAPINFOHEADER)
                               + sizeof(RGBQUAD) * palette_entries;

        size_t compact = (size_t)(w * h * bpp);
        if (compact > ctx->image_buf_bytes) {
            free(ctx->image_buf);
            ctx->image_buf = (uint8_t*)malloc(compact);
            if (!ctx->image_buf) { ctx->image_buf_bytes = 0; GlobalUnlock((HGLOBAL)(uintptr_t)hBitmap); GlobalFree((HGLOBAL)(uintptr_t)hBitmap); goto fail_disable; }
            ctx->image_buf_bytes = compact;
        }

        /* DIBs are stored bottom-up; flip rows while stripping padding. */
        for (int y = 0; y < h; y++) {
            unsigned char* src_row  = pBits + (size_t)(h - 1 - y) * (size_t)row_bytes;
            unsigned char* dst_row  = ctx->image_buf + (size_t)y * (size_t)(w * bpp);
            memcpy(dst_row, src_row, (size_t)(w * bpp));
        }
        ctx->frame_bytes    = (int)compact;
        ctx->img_width      = w;
        ctx->img_height     = h;
        ctx->bytes_per_pixel = bpp;
        ctx->bit_depth      = (int)pHead->biBitCount;

        GlobalUnlock((HGLOBAL)(uintptr_t)hBitmap);
        GlobalFree((HGLOBAL)(uintptr_t)hBitmap);
    }
#else
    /* Linux: use DAT_IMAGEMEMXFER for memory-based transfer. */
    {
        size_t compact = (size_t)(ctx->img_width * ctx->img_height * ctx->bytes_per_pixel);
        if (compact == 0) goto fail_disable;
        if (compact > ctx->image_buf_bytes) {
            free(ctx->image_buf);
            ctx->image_buf = (uint8_t*)malloc(compact);
            if (!ctx->image_buf) { ctx->image_buf_bytes = 0; goto fail_disable; }
            ctx->image_buf_bytes = compact;
        }

        TW_IMAGEMEMXFER xfer;
        memset(&xfer, 0, sizeof(xfer));
        xfer.Compression  = TWON_DONTCARE16;
        xfer.BytesPerRow  = (TW_UINT32)(ctx->img_width * ctx->bytes_per_pixel);
        xfer.Columns      = (TW_UINT32)ctx->img_width;
        xfer.Rows         = (TW_UINT32)ctx->img_height;
        xfer.XOffset      = 0;
        xfer.YOffset      = 0;
        xfer.BytesWritten = 0;
        xfer.Memory.Flags  = TWMF_APPOWNS | TWMF_POINTER;
        xfer.Memory.Length = (TW_UINT32)compact;
        xfer.Memory.TheMem = (TW_MEMREF)ctx->image_buf;

        TW_UINT16 xrc = dsm_call(&ctx->src_id, DG_IMAGE, DAT_IMAGEMEMXFER,
                                  MSG_GET, (TW_MEMREF)&xfer);
        if (xrc != TWRC_SUCCESS && xrc != TWRC_XFERDONE) goto fail_disable;
        ctx->frame_bytes = (int)compact;
    }
#endif

    /* End the transfer. */
    {
        TW_PENDINGXFERS px;
        memset(&px, 0, sizeof(px));
        dsm_call(&ctx->src_id, DG_CONTROL, DAT_PENDINGXFERS, MSG_ENDXFER,
                 (TW_MEMREF)&px);
        if (px.Count != 0) {
            /* Drain any extras. */
            memset(&px, 0, sizeof(px));
            dsm_call(&ctx->src_id, DG_CONTROL, DAT_PENDINGXFERS, MSG_RESET,
                     (TW_MEMREF)&px);
        }
    }

    /* Disable source. */
    memset(&ui, 0, sizeof(ui));
    dsm_call(&ctx->src_id, DG_CONTROL, DAT_USERINTERFACE, MSG_DISABLEDS,
             (TW_MEMREF)&ui);
    return 0;

fail_disable:
    memset(&ui, 0, sizeof(ui));
    dsm_call(&ctx->src_id, DG_CONTROL, DAT_USERINTERFACE, MSG_DISABLEDS,
             (TW_MEMREF)&ui);
    return -1;
}

const uint8_t* twain_get_frame_ptr(TwainCtx* ctx) {
    return ctx ? ctx->image_buf : NULL;
}

int twain_get_frame_bytes(TwainCtx* ctx) {
    return ctx ? ctx->frame_bytes : 0;
}
