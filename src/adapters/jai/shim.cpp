/// C wrapper around the Pleora eBUS SDK C++ API.
///
/// Exposes a plain `extern "C"` API so that Rust can call into the C++ SDK
/// without needing a C++ FFI bridge on the Rust side.  All eBUS exceptions
/// are caught here; functions return -1 / nullptr on failure.

#include <PvSystem.h>
#include <PvDevice.h>
#include <PvDeviceGEV.h>
#include <PvDeviceU3V.h>
#include <PvStream.h>
#include <PvStreamGEV.h>
#include <PvStreamU3V.h>
#include <PvBuffer.h>
#include <PvImage.h>
#include <PvGenParameterArray.h>
#include <PvGenEnum.h>
#include <PvGenInteger.h>
#include <PvGenFloat.h>
#include <PvGenString.h>
#include <PvGenCommand.h>

#include <vector>
#include <string>
#include <cstring>
#include <cstdint>
#include <stdexcept>

// ── Internal wrappers ─────────────────────────────────────────────────────────

/// Wraps PvSystem and caches the flattened device list after Find().
struct JaiSystem {
    PvSystem pvSystem;
    // Parallel arrays: connectionId and serialNumber for each found device.
    std::vector<std::string> connectionIds;
    std::vector<std::string> serialNumbers;
};

/// Wraps a connected PvDevice.
struct JaiDevice {
    PvDevice*        pvDevice  = nullptr;
    std::string      connectionId;
};

/// Wraps an open PvStream.
struct JaiStream {
    PvStream* pvStream = nullptr;
};

/// Wraps a PvBuffer (either allocated internally or retrieved from stream).
struct JaiBuffer {
    PvBuffer* pvBuffer = nullptr;
    bool      owned    = true;   // false when retrieved from a stream queue
};

// ── Helpers ───────────────────────────────────────────────────────────────────

static void safe_copy(const char* src, char* dst, int len) {
    if (!src || len <= 0) { if (dst && len > 0) dst[0] = '\0'; return; }
    std::strncpy(dst, src, (size_t)(len - 1));
    dst[len - 1] = '\0';
}

// ── C API ─────────────────────────────────────────────────────────────────────

extern "C" {

// ── System ───────────────────────────────────────────────────────────────────

JaiSystem* jai_system_new() {
    try { return new JaiSystem(); }
    catch (...) { return nullptr; }
}

void jai_system_free(JaiSystem* s) { delete s; }

/// Enumerate all eBUS-visible devices.  Returns the number of devices found,
/// or -1 on error.  Results are cached inside `s` and accessible via
/// jai_system_get_device_id / jai_system_get_device_serial.
int jai_system_find(JaiSystem* s) {
    if (!s) return -1;
    try {
        s->connectionIds.clear();
        s->serialNumbers.clear();

        PvResult r = s->pvSystem.Find();
        if (!r.IsOK()) return -1;

        for (uint32_t i = 0; i < s->pvSystem.GetInterfaceCount(); i++) {
            const PvInterface* iface = s->pvSystem.GetInterface(i);
            if (!iface) continue;
            for (uint32_t j = 0; j < iface->GetDeviceCount(); j++) {
                const PvDeviceInfo* di = iface->GetDeviceInfo(j);
                if (!di) continue;
                s->connectionIds.push_back(di->GetConnectionID().GetAscii());
                s->serialNumbers.push_back(di->GetSerialNumber().GetAscii());
            }
        }
        return (int)s->connectionIds.size();
    } catch (...) { return -1; }
}

int jai_system_get_device_id(JaiSystem* s, int idx, char* buf, int len) {
    if (!s || idx < 0 || (size_t)idx >= s->connectionIds.size()) return -1;
    safe_copy(s->connectionIds[(size_t)idx].c_str(), buf, len);
    return 0;
}

int jai_system_get_device_serial(JaiSystem* s, int idx, char* buf, int len) {
    if (!s || idx < 0 || (size_t)idx >= s->serialNumbers.size()) return -1;
    safe_copy(s->serialNumbers[(size_t)idx].c_str(), buf, len);
    return 0;
}

// ── Device ───────────────────────────────────────────────────────────────────

JaiDevice* jai_device_connect(const char* connection_id) {
    if (!connection_id) return nullptr;
    try {
        PvResult r;
        PvDevice* dev = PvDevice::CreateAndConnect(PvString(connection_id), &r);
        if (!r.IsOK() || !dev) return nullptr;
        JaiDevice* d = new JaiDevice();
        d->pvDevice     = dev;
        d->connectionId = connection_id;
        return d;
    } catch (...) { return nullptr; }
}

void jai_device_free(JaiDevice* d) {
    if (!d) return;
    try {
        if (d->pvDevice) {
            d->pvDevice->Disconnect();
            PvDevice::Free(d->pvDevice);
            d->pvDevice = nullptr;
        }
    } catch (...) {}
    delete d;
}

int jai_device_get_int(JaiDevice* d, const char* name, int64_t* out) {
    if (!d || !d->pvDevice || !name || !out) return -1;
    try {
        PvGenParameterArray* p = d->pvDevice->GetParameters();
        int64_t v = 0;
        PvResult r = p->GetIntegerValue(PvString(name), v);
        if (!r.IsOK()) return -1;
        *out = v;
        return 0;
    } catch (...) { return -1; }
}

int jai_device_get_int_increment(JaiDevice* d, const char* name, int64_t* out) {
    if (!d || !d->pvDevice || !name || !out) return -1;
    try {
        PvGenParameterArray* p = d->pvDevice->GetParameters();
        PvGenInteger* node = p->GetInteger(PvString(name));
        if (!node) return -1;
        int64_t v = 1;
        PvResult r = node->GetIncrement(v);
        if (!r.IsOK()) return -1;
        *out = v > 0 ? v : 1;
        return 0;
    } catch (...) { return -1; }
}

int jai_device_set_int(JaiDevice* d, const char* name, int64_t value) {
    if (!d || !d->pvDevice || !name) return -1;
    try {
        PvGenParameterArray* p = d->pvDevice->GetParameters();
        return p->SetIntegerValue(PvString(name), value).IsOK() ? 0 : -1;
    } catch (...) { return -1; }
}

int jai_device_get_float(JaiDevice* d, const char* name, double* out) {
    if (!d || !d->pvDevice || !name || !out) return -1;
    try {
        PvGenParameterArray* p = d->pvDevice->GetParameters();
        double v = 0.0;
        PvResult r = p->GetFloatValue(PvString(name), v);
        if (!r.IsOK()) return -1;
        *out = v;
        return 0;
    } catch (...) { return -1; }
}

int jai_device_set_float(JaiDevice* d, const char* name, double value) {
    if (!d || !d->pvDevice || !name) return -1;
    try {
        PvGenParameterArray* p = d->pvDevice->GetParameters();
        return p->SetFloatValue(PvString(name), value).IsOK() ? 0 : -1;
    } catch (...) { return -1; }
}

int jai_device_get_string(JaiDevice* d, const char* name, char* buf, int len) {
    if (!d || !d->pvDevice || !name || !buf) return -1;
    try {
        PvGenParameterArray* p = d->pvDevice->GetParameters();
        PvString v;
        PvResult r = p->GetStringValue(PvString(name), v);
        if (!r.IsOK()) return -1;
        safe_copy(v.GetAscii(), buf, len);
        return 0;
    } catch (...) { return -1; }
}

int jai_device_get_enum(JaiDevice* d, const char* name, char* buf, int len) {
    if (!d || !d->pvDevice || !name || !buf) return -1;
    try {
        PvGenParameterArray* p = d->pvDevice->GetParameters();
        PvString v;
        PvResult r = p->GetEnumValue(PvString(name), v);
        if (!r.IsOK()) return -1;
        safe_copy(v.GetAscii(), buf, len);
        return 0;
    } catch (...) { return -1; }
}

int jai_device_get_enum_entries(JaiDevice* d, const char* name, char* buf, int len) {
    if (!d || !d->pvDevice || !name || !buf) return -1;
    try {
        PvGenParameterArray* p = d->pvDevice->GetParameters();
        PvGenEnum* e = p->GetEnum(PvString(name));
        if (!e) return -1;
        int64_t count = 0;
        if (!e->GetEntriesCount(count).IsOK()) return -1;
        std::string joined;
        for (int64_t i = 0; i < count; ++i) {
            const PvGenEnumEntry* entry = nullptr;
            if (!e->GetEntryByIndex(static_cast<uint32_t>(i), &entry).IsOK() || !entry)
                continue;
            if (!entry->IsAvailable())
                continue;
            PvString entry_name;
            if (!entry->GetName(entry_name).IsOK())
                continue;
            if (!joined.empty())
                joined.push_back(';');
            joined.append(entry_name.GetAscii());
        }
        if (joined.empty()) return -1;
        safe_copy(joined.c_str(), buf, len);
        return 0;
    } catch (...) { return -1; }
}

int jai_device_set_enum(JaiDevice* d, const char* name, const char* value) {
    if (!d || !d->pvDevice || !name || !value) return -1;
    try {
        PvGenParameterArray* p = d->pvDevice->GetParameters();
        return p->SetEnumValue(PvString(name), PvString(value)).IsOK() ? 0 : -1;
    } catch (...) { return -1; }
}

int jai_device_execute(JaiDevice* d, const char* name) {
    if (!d || !d->pvDevice || !name) return -1;
    try {
        PvGenParameterArray* p = d->pvDevice->GetParameters();
        return p->ExecuteCommand(PvString(name)).IsOK() ? 0 : -1;
    } catch (...) { return -1; }
}

uint64_t jai_device_payload_size(JaiDevice* d) {
    if (!d || !d->pvDevice) return 0;
    try { return (uint64_t)d->pvDevice->GetPayloadSize(); }
    catch (...) { return 0; }
}

int jai_device_stream_enable(JaiDevice* d) {
    if (!d || !d->pvDevice) return -1;
    try { return d->pvDevice->StreamEnable().IsOK() ? 0 : -1; }
    catch (...) { return -1; }
}

int jai_device_stream_disable(JaiDevice* d) {
    if (!d || !d->pvDevice) return -1;
    try { return d->pvDevice->StreamDisable().IsOK() ? 0 : -1; }
    catch (...) { return -1; }
}

/// Returns the connection ID string (used to open the matching stream).
int jai_device_get_connection_id(JaiDevice* d, char* buf, int len) {
    if (!d || !buf) return -1;
    safe_copy(d->connectionId.c_str(), buf, len);
    return 0;
}

// ── Stream ───────────────────────────────────────────────────────────────────

JaiStream* jai_stream_open(const char* connection_id) {
    if (!connection_id) return nullptr;
    try {
        PvResult r;
        PvStream* s = PvStream::CreateAndOpen(PvString(connection_id), &r);
        if (!r.IsOK() || !s) return nullptr;
        JaiStream* js = new JaiStream();
        js->pvStream = s;
        return js;
    } catch (...) { return nullptr; }
}

void jai_stream_free(JaiStream* s) {
    if (!s) return;
    try {
        if (s->pvStream) {
            s->pvStream->Close();
            PvStream::Free(s->pvStream);
            s->pvStream = nullptr;
        }
    } catch (...) {}
    delete s;
}

int jai_stream_queue(JaiStream* s, JaiBuffer* buf) {
    if (!s || !s->pvStream || !buf || !buf->pvBuffer) return -1;
    try { return s->pvStream->QueueBuffer(buf->pvBuffer).IsOK() ? 0 : -1; }
    catch (...) { return -1; }
}

/// Retrieves the next completed buffer from the stream.  Returns a JaiBuffer
/// wrapping the PvBuffer (not owned by the caller — must be re-queued or the
/// stream closed before freeing).  Returns nullptr on timeout / error.
JaiBuffer* jai_stream_retrieve(JaiStream* s, uint32_t timeout_ms) {
    if (!s || !s->pvStream) return nullptr;
    try {
        PvBuffer* pvBuf = nullptr;
        PvResult  opResult;
        PvResult  r = s->pvStream->RetrieveBuffer(&pvBuf, &opResult, timeout_ms);
        if (!r.IsOK() || !opResult.IsOK() || !pvBuf) return nullptr;
        // Wrap without ownership; caller must call jai_buffer_requeue or
        // jai_stream_abort before freeing the JaiBuffer wrapper.
        JaiBuffer* jb = new JaiBuffer();
        jb->pvBuffer = pvBuf;
        jb->owned    = false;
        return jb;
    } catch (...) { return nullptr; }
}

/// Re-queues a retrieved buffer back to the stream for reuse.
int jai_stream_requeue(JaiStream* s, JaiBuffer* buf) {
    if (!s || !s->pvStream || !buf || !buf->pvBuffer) return -1;
    try { return s->pvStream->QueueBuffer(buf->pvBuffer).IsOK() ? 0 : -1; }
    catch (...) { return -1; }
}

int jai_stream_abort(JaiStream* s) {
    if (!s || !s->pvStream) return -1;
    try { return s->pvStream->AbortQueuedBuffers().IsOK() ? 0 : -1; }
    catch (...) { return -1; }
}

// ── Buffer ───────────────────────────────────────────────────────────────────

JaiBuffer* jai_buffer_alloc(uint64_t size) {
    try {
        JaiBuffer* jb = new JaiBuffer();
        jb->pvBuffer = new PvBuffer();
        jb->pvBuffer->Alloc((uint32_t)size);
        jb->owned = true;
        return jb;
    } catch (...) { return nullptr; }
}

void jai_buffer_free(JaiBuffer* buf) {
    if (!buf) return;
    if (buf->owned && buf->pvBuffer) {
        delete buf->pvBuffer;
        buf->pvBuffer = nullptr;
    } else {
        // Not owned: only delete the wrapper, not the underlying PvBuffer
        // (which is still in the stream queue or has been aborted).
    }
    delete buf;
}

uint32_t jai_buffer_width(JaiBuffer* buf) {
    if (!buf || !buf->pvBuffer) return 0;
    try {
        PvImage* img = buf->pvBuffer->GetImage();
        return img ? img->GetWidth() : 0;
    } catch (...) { return 0; }
}

uint32_t jai_buffer_height(JaiBuffer* buf) {
    if (!buf || !buf->pvBuffer) return 0;
    try {
        PvImage* img = buf->pvBuffer->GetImage();
        return img ? img->GetHeight() : 0;
    } catch (...) { return 0; }
}

uint32_t jai_buffer_bits_per_pixel(JaiBuffer* buf) {
    if (!buf || !buf->pvBuffer) return 8;
    try {
        PvImage* img = buf->pvBuffer->GetImage();
        return img ? img->GetBitsPerPixel() : 8;
    } catch (...) { return 8; }
}

uint32_t jai_buffer_bits_per_component(JaiBuffer* buf) {
    if (!buf || !buf->pvBuffer) return 8;
    try {
        PvImage* img = buf->pvBuffer->GetImage();
        if (!img) return 8;
        return PvImage::GetBitsPerComponent(img->GetPixelType());
    } catch (...) { return 8; }
}

int jai_buffer_is_color(JaiBuffer* buf) {
    if (!buf || !buf->pvBuffer) return 0;
    try {
        PvImage* img = buf->pvBuffer->GetImage();
        if (!img) return 0;
        return PvImage::IsPixelColor(img->GetPixelType()) ? 1 : 0;
    } catch (...) { return 0; }
}

uint32_t jai_buffer_padding_x(JaiBuffer* buf) {
    if (!buf || !buf->pvBuffer) return 0;
    try {
        PvImage* img = buf->pvBuffer->GetImage();
        return img ? img->GetPaddingX() : 0;
    } catch (...) { return 0; }
}

/// Pointer to raw pixel data (valid until the buffer is freed or re-queued).
const uint8_t* jai_buffer_data(JaiBuffer* buf) {
    if (!buf || !buf->pvBuffer) return nullptr;
    try {
        PvImage* img = buf->pvBuffer->GetImage();
        if (!img) return nullptr;
        return reinterpret_cast<const uint8_t*>(img->GetDataPointer());
    } catch (...) { return nullptr; }
}

/// Total size of the pixel data in bytes (width * bytes_per_pixel + paddingX) * height.
uint64_t jai_buffer_data_size(JaiBuffer* buf) {
    if (!buf || !buf->pvBuffer) return 0;
    try {
        PvImage* img = buf->pvBuffer->GetImage();
        if (!img) return 0;
        uint32_t w       = img->GetWidth();
        uint32_t h       = img->GetHeight();
        uint32_t bpp     = img->GetBitsPerPixel();
        uint32_t padding = img->GetPaddingX();
        uint32_t stride  = (w * bpp + 7) / 8 + padding;
        return (uint64_t)stride * h;
    } catch (...) { return 0; }
}

} // extern "C"
