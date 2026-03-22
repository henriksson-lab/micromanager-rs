use std::ffi::{CStr, CString};

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Camera, Device};
use micromanager::types::{DeviceType, ImageRoi, PropertyValue};

use crate::ffi;

// SAFETY: JAICamera holds raw pointers into eBUS SDK objects.
// The eBUS SDK is thread-safe for separate camera handles; we guarantee that
// only one Rust thread accesses each JAICamera at a time by requiring `&mut
// self` on all mutating methods.
unsafe impl Send for JAICamera {}

// ── String helpers ────────────────────────────────────────────────────────────

const BUF: usize = 256;

/// Call an FFI function that fills a fixed-size char buffer.
/// Returns an owned String on success, or an error.
macro_rules! get_str {
    ($fn:expr) => {{
        let mut buf = [0i8; BUF];
        let rc = unsafe { $fn(buf.as_mut_ptr(), BUF as i32) };
        if rc != 0 {
            return Err(MmError::LocallyDefined("jai_get_str failed".into()));
        }
        let s = unsafe { CStr::from_ptr(buf.as_ptr()) };
        s.to_string_lossy().into_owned()
    }};
}

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

// ── Pixel format helpers ──────────────────────────────────────────────────────

fn bpp_to_bytes(bpp: u32) -> u32 {
    (bpp + 7) / 8
}

// ── Camera struct ─────────────────────────────────────────────────────────────

/// Sequence state: one stream + N pre-allocated PvBuffers for continuous grab.
struct SequenceState {
    stream:  *mut ffi::JaiStream,
    buffers: Vec<*mut ffi::JaiBuffer>,
}

pub struct JAICamera {
    props: PropertyMap,

    // Raw SDK handles (null when not initialized).
    system: *mut ffi::JaiSystem,
    device: *mut ffi::JaiDevice,
    seq:    Option<SequenceState>,

    // Cached image data from the last snap.
    image_buf:      Vec<u8>,
    width:          u32,
    height:         u32,
    bytes_per_pixel: u32,
    bit_depth:      u32,
    num_components: u32,

    // Pre-init settings.
    camera_index:  i32,
    serial_number: String,
    exposure_ms:   f64,
    gain:          f64,
    pixel_format:  String,
    binning:       i32,
}

impl JAICamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("CameraIndex",   PropertyValue::Integer(0),                false).unwrap();
        props.define_property("SerialNumber",  PropertyValue::String("".into()),         false).unwrap();
        props.define_property("Exposure",      PropertyValue::Float(10.0),              false).unwrap();
        props.define_property("Gain",          PropertyValue::Float(0.0),               false).unwrap();
        props.define_property("PixelFormat",   PropertyValue::String("Mono8".into()),   false).unwrap();
        props.define_property("Binning",       PropertyValue::Integer(1),               false).unwrap();
        props.define_property("Width",         PropertyValue::Integer(0),               true).unwrap();
        props.define_property("Height",        PropertyValue::Integer(0),               true).unwrap();
        props.define_property("Temperature",   PropertyValue::Float(0.0),              true).unwrap();
        props.define_property("Model",         PropertyValue::String("".into()),        true).unwrap();

        Self {
            props,
            system: std::ptr::null_mut(),
            device: std::ptr::null_mut(),
            seq:    None,
            image_buf:      Vec::new(),
            width:          0,
            height:         0,
            bytes_per_pixel: 1,
            bit_depth:      8,
            num_components: 1,
            camera_index:   0,
            serial_number:  String::new(),
            exposure_ms:    10.0,
            gain:           0.0,
            pixel_format:   "Mono8".into(),
            binning:        1,
        }
    }

    fn check_open(&self) -> MmResult<()> {
        if self.device.is_null() { Err(MmError::NotConnected) } else { Ok(()) }
    }

    // ── Device parameter helpers ───────────────────────────────────────────────

    fn dev_set_float(&self, name: &str, v: f64) {
        if self.device.is_null() { return; }
        let n = cstr(name);
        unsafe { ffi::jai_device_set_float(self.device, n.as_ptr(), v); }
    }

    fn dev_set_int(&self, name: &str, v: i64) {
        if self.device.is_null() { return; }
        let n = cstr(name);
        unsafe { ffi::jai_device_set_int(self.device, n.as_ptr(), v); }
    }

    fn dev_set_enum(&self, name: &str, v: &str) {
        if self.device.is_null() { return; }
        let n = cstr(name);
        let val = cstr(v);
        unsafe { ffi::jai_device_set_enum(self.device, n.as_ptr(), val.as_ptr()); }
    }

    fn dev_execute(&self, name: &str) {
        if self.device.is_null() { return; }
        let n = cstr(name);
        unsafe { ffi::jai_device_execute(self.device, n.as_ptr()); }
    }

    fn dev_get_float(&self, name: &str) -> Option<f64> {
        if self.device.is_null() { return None; }
        let n = cstr(name);
        let mut v: f64 = 0.0;
        let rc = unsafe { ffi::jai_device_get_float(self.device, n.as_ptr(), &mut v) };
        if rc == 0 { Some(v) } else { None }
    }

    fn dev_get_int(&self, name: &str) -> Option<i64> {
        if self.device.is_null() { return None; }
        let n = cstr(name);
        let mut v: i64 = 0;
        let rc = unsafe { ffi::jai_device_get_int(self.device, n.as_ptr(), &mut v) };
        if rc == 0 { Some(v) } else { None }
    }

    fn dev_get_string(&self, name: &str) -> Option<String> {
        if self.device.is_null() { return None; }
        let n = cstr(name);
        let mut buf = [0i8; BUF];
        let rc = unsafe {
            ffi::jai_device_get_string(self.device, n.as_ptr(), buf.as_mut_ptr(), BUF as i32)
        };
        if rc != 0 { return None; }
        let s = unsafe { CStr::from_ptr(buf.as_ptr()) };
        Some(s.to_string_lossy().into_owned())
    }

    fn dev_get_enum(&self, name: &str) -> Option<String> {
        if self.device.is_null() { return None; }
        let n = cstr(name);
        let mut buf = [0i8; BUF];
        let rc = unsafe {
            ffi::jai_device_get_enum(self.device, n.as_ptr(), buf.as_mut_ptr(), BUF as i32)
        };
        if rc != 0 { return None; }
        let s = unsafe { CStr::from_ptr(buf.as_ptr()) };
        Some(s.to_string_lossy().into_owned())
    }

    // ── Sync dimensions from camera ───────────────────────────────────────────

    fn sync_dimensions(&mut self) {
        if let Some(w) = self.dev_get_int("Width")  { self.width  = w as u32; }
        if let Some(h) = self.dev_get_int("Height") { self.height = h as u32; }
        if let Some(fmt) = self.dev_get_enum("PixelFormat") {
            self.pixel_format = fmt;
        }
        self.bytes_per_pixel = bpp_to_bytes(
            unsafe { ffi::jai_buffer_bits_per_pixel(std::ptr::null_mut()) }.max(8),
        );
        // Fallback: use bits-per-pixel from current format string
        self.bytes_per_pixel = if self.pixel_format.contains("16") { 2 }
            else if self.pixel_format.contains("10") || self.pixel_format.contains("12") { 2 }
            else if self.pixel_format.contains("8") && self.pixel_format.contains("RGB")  { 3 }
            else { 1 };
        self.bit_depth = if self.pixel_format.contains("12") { 12 }
            else if self.pixel_format.contains("10") { 10 }
            else if self.pixel_format.contains("16") { 16 }
            else { 8 };
        self.num_components = if self.pixel_format.contains("RGB") || self.pixel_format.contains("BGR") { 3 } else { 1 };
        self.props.entry_mut("Width") .map(|e| e.value = PropertyValue::Integer(self.width  as i64));
        self.props.entry_mut("Height").map(|e| e.value = PropertyValue::Integer(self.height as i64));
    }

    // ── Apply pre-init settings to open device ────────────────────────────────

    fn apply_settings(&mut self) {
        let ms  = self.exposure_ms;
        let g   = self.gain;
        let bin = self.binning;
        let fmt = self.pixel_format.clone();

        // Exposure: ExposureTimeAbs in µs (most JAI cameras use this node).
        self.dev_set_float("ExposureTimeAbs", ms * 1_000.0);
        // Try the GenICam standard node name as well.
        self.dev_set_float("ExposureTime", ms * 1_000.0);

        // Gain
        if self.dev_set_float_check("Gain", g).is_err() {
            self.dev_set_int("GainRaw", g as i64);
        }

        // Binning (symmetric)
        self.dev_set_int("BinningHorizontal", bin as i64);
        self.dev_set_int("BinningVertical",   bin as i64);

        // Pixel format
        self.dev_set_enum("PixelFormat", &fmt);

        // Reset to factory defaults then forced-timed exposure mode.
        self.dev_set_int("UserSetSelector", 0);
        self.dev_execute("UserSetLoad");
        self.dev_set_enum("ExposureMode", "Timed");
        self.dev_set_enum("MultiRoiMode", "Off");
    }

    fn dev_set_float_check(&self, name: &str, v: f64) -> MmResult<()> {
        if self.device.is_null() { return Err(MmError::NotConnected); }
        let n = cstr(name);
        let rc = unsafe { ffi::jai_device_set_float(self.device, n.as_ptr(), v) };
        if rc == 0 { Ok(()) } else { Err(MmError::Err) }
    }

    // ── Single-frame grab ─────────────────────────────────────────────────────

    fn snap_one_frame(&mut self) -> MmResult<()> {
        // 1. Set single-frame mode.
        self.dev_set_enum("AcquisitionMode", "SingleFrame");

        // 2. Get connection ID to open the matching stream.
        let mut conn_buf = [0i8; BUF];
        let rc = unsafe {
            ffi::jai_device_get_connection_id(self.device, conn_buf.as_mut_ptr(), BUF as i32)
        };
        if rc != 0 { return Err(MmError::NotConnected); }
        let conn = unsafe { CStr::from_ptr(conn_buf.as_ptr()) };
        let conn_cstr = CString::new(conn.to_bytes()).map_err(|_| MmError::Err)?;

        // 3. Open stream.
        let stream = unsafe { ffi::jai_stream_open(conn_cstr.as_ptr()) };
        if stream.is_null() {
            return Err(MmError::LocallyDefined("JAI: failed to open stream".into()));
        }

        let result = (|| {
            // 4. Allocate one buffer.
            let payload = unsafe { ffi::jai_device_payload_size(self.device) };
            let buf = unsafe { ffi::jai_buffer_alloc(payload) };
            if buf.is_null() {
                return Err(MmError::LocallyDefined("JAI: buffer alloc failed".into()));
            }

            // 5. Queue buffer + start acquisition.
            unsafe { ffi::jai_stream_queue(stream, buf) };
            unsafe { ffi::jai_device_stream_enable(self.device) };
            self.dev_execute("AcquisitionStart");

            // 6. Wait for frame (4 second timeout).
            let grabbed = unsafe { ffi::jai_stream_retrieve(stream, 4000) };

            // 7. Stop acquisition.
            self.dev_execute("AcquisitionStop");
            unsafe { ffi::jai_device_stream_disable(self.device) };

            if grabbed.is_null() {
                unsafe { ffi::jai_buffer_free(buf) };
                return Err(MmError::SnapImageFailed);
            }

            // 8. Copy pixel data.
            self.copy_from_buffer(grabbed);

            // 9. Release (grabbed is non-owned; free the wrapper but not the PvBuffer
            //    since stream is about to close).
            unsafe { ffi::jai_buffer_free(grabbed) };
            unsafe { ffi::jai_buffer_free(buf) };
            Ok(())
        })();

        unsafe { ffi::jai_stream_free(stream) };
        result
    }

    /// Copy pixel data from a retrieved buffer into `self.image_buf`.
    fn copy_from_buffer(&mut self, buf: *mut ffi::JaiBuffer) {
        let size = unsafe { ffi::jai_buffer_data_size(buf) } as usize;
        let data = unsafe { ffi::jai_buffer_data(buf) };
        if data.is_null() || size == 0 { return; }

        self.width          = unsafe { ffi::jai_buffer_width(buf) };
        self.height         = unsafe { ffi::jai_buffer_height(buf) };
        let bpp             = unsafe { ffi::jai_buffer_bits_per_pixel(buf) };
        let bpc             = unsafe { ffi::jai_buffer_bits_per_component(buf) };
        let is_color        = unsafe { ffi::jai_buffer_is_color(buf) } != 0;
        self.bytes_per_pixel = bpp_to_bytes(bpp);
        self.bit_depth       = bpc;
        self.num_components  = if is_color { 3 } else { 1 };

        self.image_buf.resize(size, 0);
        unsafe { std::ptr::copy_nonoverlapping(data, self.image_buf.as_mut_ptr(), size) };

        self.props.entry_mut("Width") .map(|e| e.value = PropertyValue::Integer(self.width  as i64));
        self.props.entry_mut("Height").map(|e| e.value = PropertyValue::Integer(self.height as i64));
    }

    // ── Sequence: dequeue one frame from the continuous stream ────────────────

    fn snap_from_sequence(&mut self) -> MmResult<()> {
        let seq = self.seq.as_ref().ok_or(MmError::NotConnected)?;
        let grabbed = unsafe { ffi::jai_stream_retrieve(seq.stream, 4000) };
        if grabbed.is_null() { return Err(MmError::SnapImageFailed); }
        self.copy_from_buffer(grabbed);
        // Re-queue the underlying buffer for reuse.
        let seq = self.seq.as_ref().unwrap();
        unsafe { ffi::jai_stream_requeue(seq.stream, grabbed) };
        // Free the non-owning wrapper.
        unsafe { ffi::jai_buffer_free(grabbed) };
        Ok(())
    }
}

impl Default for JAICamera {
    fn default() -> Self { Self::new() }
}

impl Drop for JAICamera {
    fn drop(&mut self) {
        // Stop any active sequence.
        let _ = self.stop_sequence_acquisition();
        // Disconnect device.
        if !self.device.is_null() {
            unsafe { ffi::jai_device_free(self.device) };
            self.device = std::ptr::null_mut();
        }
        // Free system.
        if !self.system.is_null() {
            unsafe { ffi::jai_system_free(self.system) };
            self.system = std::ptr::null_mut();
        }
    }
}

// ── Device trait ──────────────────────────────────────────────────────────────

impl Device for JAICamera {
    fn name(&self) -> &str { "JAICamera" }
    fn description(&self) -> &str { "JAI camera (Pleora eBUS SDK)" }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.device.is_null() { return Ok(()); }

        // Create system + find cameras.
        let sys = unsafe { ffi::jai_system_new() };
        if sys.is_null() {
            return Err(MmError::LocallyDefined("JAI: failed to create PvSystem".into()));
        }
        self.system = sys;

        let count = unsafe { ffi::jai_system_find(sys) };
        if count < 0 {
            return Err(MmError::LocallyDefined("JAI: device enumeration failed".into()));
        }
        if count == 0 {
            return Err(MmError::LocallyDefined("JAI: no cameras found".into()));
        }

        // Find the connection ID: match by serial number, or fall back to index.
        let mut conn_buf = [0i8; BUF];
        let target_idx: i32 = if !self.serial_number.is_empty() {
            let mut found = -1i32;
            let sn_cmp = self.serial_number.clone();
            for i in 0..count {
                let mut sn_buf = [0i8; BUF];
                let rc = unsafe {
                    ffi::jai_system_get_device_serial(sys, i, sn_buf.as_mut_ptr(), BUF as i32)
                };
                if rc != 0 { continue; }
                let sn = unsafe { CStr::from_ptr(sn_buf.as_ptr()) };
                if sn.to_string_lossy() == sn_cmp.as_str() {
                    found = i;
                    break;
                }
            }
            if found < 0 {
                return Err(MmError::LocallyDefined(format!(
                    "JAI: camera with serial '{}' not found",
                    self.serial_number
                )));
            }
            found
        } else {
            self.camera_index.min(count - 1)
        };

        let rc = unsafe {
            ffi::jai_system_get_device_id(sys, target_idx, conn_buf.as_mut_ptr(), BUF as i32)
        };
        if rc != 0 {
            return Err(MmError::LocallyDefined("JAI: failed to get connection ID".into()));
        }
        let conn = unsafe { CStr::from_ptr(conn_buf.as_ptr()) };
        let conn_cstr = CString::new(conn.to_bytes()).map_err(|_| MmError::Err)?;

        // Connect.
        let dev = unsafe { ffi::jai_device_connect(conn_cstr.as_ptr()) };
        if dev.is_null() {
            return Err(MmError::LocallyDefined("JAI: failed to connect to camera".into()));
        }
        self.device = dev;

        // Apply pre-init settings.
        self.apply_settings();
        self.sync_dimensions();

        // Read back model name.
        if let Some(model) = self.dev_get_string("DeviceModelName") {
            self.props.entry_mut("Model").map(|e| e.value = PropertyValue::String(model));
        }
        // Read back serial number.
        if self.serial_number.is_empty() {
            if let Some(sn) = self.dev_get_string("DeviceSerialNumber") {
                self.serial_number = sn.clone();
                self.props.entry_mut("SerialNumber")
                    .map(|e| e.value = PropertyValue::String(sn));
            }
        }

        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        let _ = self.stop_sequence_acquisition();
        if !self.device.is_null() {
            unsafe { ffi::jai_device_free(self.device) };
            self.device = std::ptr::null_mut();
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Exposure"     => Ok(PropertyValue::Float(self.exposure_ms)),
            "Gain"         => Ok(PropertyValue::Float(self.gain)),
            "PixelFormat"  => Ok(PropertyValue::String(self.pixel_format.clone())),
            "Binning"      => Ok(PropertyValue::Integer(self.binning as i64)),
            "CameraIndex"  => Ok(PropertyValue::Integer(self.camera_index as i64)),
            "SerialNumber" => Ok(PropertyValue::String(self.serial_number.clone())),
            "Temperature"  => Ok(PropertyValue::Float(
                self.dev_get_float("DeviceTemperature").unwrap_or(0.0),
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "SerialNumber" => {
                if !self.device.is_null() {
                    return Err(MmError::LocallyDefined(
                        "SerialNumber cannot be changed after initialize()".into(),
                    ));
                }
                self.serial_number = val.as_str().to_string();
                self.props.set(name, val)
            }
            "CameraIndex" => {
                if !self.device.is_null() {
                    return Err(MmError::LocallyDefined(
                        "CameraIndex cannot be changed after initialize()".into(),
                    ));
                }
                self.camera_index = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props.set(name, PropertyValue::Integer(self.camera_index as i64))
            }
            "Exposure" => {
                self.exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.exposure_ms))?;
                let ms = self.exposure_ms;
                self.dev_set_float("ExposureTimeAbs", ms * 1_000.0);
                self.dev_set_float("ExposureTime",    ms * 1_000.0);
                Ok(())
            }
            "Gain" => {
                self.gain = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.gain))?;
                let g = self.gain;
                if self.dev_set_float_check("Gain", g).is_err() {
                    self.dev_set_int("GainRaw", g as i64);
                }
                Ok(())
            }
            "PixelFormat" => {
                self.pixel_format = val.as_str().to_string();
                self.props.set(name, val)?;
                let fmt = self.pixel_format.clone();
                self.dev_set_enum("PixelFormat", &fmt);
                self.sync_dimensions();
                Ok(())
            }
            "Binning" => {
                self.binning = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props.set(name, PropertyValue::Integer(self.binning as i64))?;
                let bin = self.binning;
                self.dev_set_int("BinningHorizontal", bin as i64);
                self.dev_set_int("BinningVertical",   bin as i64);
                self.sync_dimensions();
                Ok(())
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Camera }
    fn busy(&self) -> bool { false }
}

// ── Camera trait ──────────────────────────────────────────────────────────────

impl Camera for JAICamera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_open()?;
        if self.seq.is_some() {
            return self.snap_from_sequence();
        }
        self.snap_one_frame()
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if self.image_buf.is_empty() {
            Err(MmError::LocallyDefined("No image captured yet".into()))
        } else {
            Ok(&self.image_buf)
        }
    }

    fn get_image_width(&self) -> u32  { self.width }
    fn get_image_height(&self) -> u32 { self.height }
    fn get_image_bytes_per_pixel(&self) -> u32 { self.bytes_per_pixel }
    fn get_bit_depth(&self) -> u32 { self.bit_depth }
    fn get_number_of_components(&self) -> u32 { self.num_components }
    fn get_number_of_channels(&self) -> u32 { 1 }
    fn get_exposure(&self) -> f64 { self.exposure_ms }

    fn set_exposure(&mut self, exp_ms: f64) {
        self.exposure_ms = exp_ms;
        self.props.set("Exposure", PropertyValue::Float(exp_ms)).ok();
        self.dev_set_float("ExposureTimeAbs", exp_ms * 1_000.0);
        self.dev_set_float("ExposureTime",    exp_ms * 1_000.0);
    }

    fn get_binning(&self) -> i32 { self.binning }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        self.binning = bin;
        self.props.set("Binning", PropertyValue::Integer(bin as i64))?;
        self.dev_set_int("BinningHorizontal", bin as i64);
        self.dev_set_int("BinningVertical",   bin as i64);
        self.sync_dimensions();
        Ok(())
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        Ok(ImageRoi::new(0, 0, self.width, self.height))
    }

    fn set_roi(&mut self, roi: ImageRoi) -> MmResult<()> {
        self.check_open()?;
        // Width/Height before OffsetX/Y (standard GenICam ordering).
        self.dev_set_int("Width",   roi.width  as i64);
        self.dev_set_int("Height",  roi.height as i64);
        self.dev_set_int("OffsetX", roi.x      as i64);
        self.dev_set_int("OffsetY", roi.y      as i64);
        self.sync_dimensions();
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        self.check_open()?;
        self.dev_set_int("OffsetX", 0);
        self.dev_set_int("OffsetY", 0);
        // Set width/height to their hardware maxima via the camera parameter.
        if let Some(max_w) = self.dev_get_int("WidthMax")  { self.dev_set_int("Width",  max_w); }
        if let Some(max_h) = self.dev_get_int("HeightMax") { self.dev_set_int("Height", max_h); }
        self.sync_dimensions();
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, _count: i64, _interval_ms: f64) -> MmResult<()> {
        self.check_open()?;
        if self.seq.is_some() { return Ok(()); }

        self.dev_set_enum("AcquisitionMode", "Continuous");

        // Get connection ID.
        let mut conn_buf = [0i8; BUF];
        let rc = unsafe {
            ffi::jai_device_get_connection_id(self.device, conn_buf.as_mut_ptr(), BUF as i32)
        };
        if rc != 0 { return Err(MmError::NotConnected); }
        let conn = unsafe { CStr::from_ptr(conn_buf.as_ptr()) };
        let conn_cstr = CString::new(conn.to_bytes()).map_err(|_| MmError::Err)?;

        // Open stream.
        let stream = unsafe { ffi::jai_stream_open(conn_cstr.as_ptr()) };
        if stream.is_null() {
            return Err(MmError::LocallyDefined("JAI: failed to open sequence stream".into()));
        }

        // Allocate and queue 8 buffers.
        let payload = unsafe { ffi::jai_device_payload_size(self.device) };
        let mut buffers: Vec<*mut ffi::JaiBuffer> = Vec::new();
        for _ in 0..8 {
            let b = unsafe { ffi::jai_buffer_alloc(payload) };
            if b.is_null() { break; }
            unsafe { ffi::jai_stream_queue(stream, b) };
            buffers.push(b);
        }
        if buffers.is_empty() {
            unsafe { ffi::jai_stream_free(stream) };
            return Err(MmError::LocallyDefined("JAI: buffer allocation failed".into()));
        }

        unsafe { ffi::jai_device_stream_enable(self.device) };
        self.dev_execute("AcquisitionStart");

        self.seq = Some(SequenceState { stream, buffers });
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        if self.seq.is_none() { return Ok(()); }

        self.dev_execute("AcquisitionStop");
        if !self.device.is_null() {
            unsafe { ffi::jai_device_stream_disable(self.device) };
        }

        if let Some(seq) = self.seq.take() {
            unsafe { ffi::jai_stream_abort(seq.stream) };
            unsafe { ffi::jai_stream_free(seq.stream) };
            for b in seq.buffers {
                unsafe { ffi::jai_buffer_free(b) };
            }
        }
        Ok(())
    }

    fn is_capturing(&self) -> bool { self.seq.is_some() }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_properties() {
        let d = JAICamera::new();
        assert_eq!(d.device_type(), DeviceType::Camera);
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(d.get_binning(), 1);
        assert!(!d.is_capturing());
        assert_eq!(d.get_number_of_channels(), 1);
    }

    #[test]
    fn set_camera_index_pre_init() {
        let mut d = JAICamera::new();
        d.set_property("CameraIndex", PropertyValue::Integer(2)).unwrap();
        assert_eq!(d.camera_index, 2);
    }

    #[test]
    fn set_serial_number_pre_init() {
        let mut d = JAICamera::new();
        d.set_property("SerialNumber", PropertyValue::String("ABCDEF".into())).unwrap();
        assert_eq!(d.serial_number, "ABCDEF");
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = JAICamera::new();
        d.set_property("Exposure", PropertyValue::Float(50.0)).unwrap();
        assert_eq!(d.exposure_ms, 50.0);
        assert_eq!(d.get_exposure(), 50.0);
    }

    #[test]
    fn set_gain_pre_init() {
        let mut d = JAICamera::new();
        d.set_property("Gain", PropertyValue::Float(3.0)).unwrap();
        assert_eq!(d.gain, 3.0);
    }

    #[test]
    fn no_image_before_snap() {
        let d = JAICamera::new();
        assert!(d.get_image_buffer().is_err());
    }

    #[test]
    fn snap_without_init_errors() {
        let mut d = JAICamera::new();
        assert!(d.snap_image().is_err());
    }

    #[test]
    fn initialize_no_camera_fails() {
        let mut d = JAICamera::new();
        // No eBUS cameras present — expect a meaningful error.
        assert!(d.initialize().is_err());
    }

    #[test]
    fn readonly_properties() {
        let d = JAICamera::new();
        assert!(d.is_property_read_only("Width"));
        assert!(d.is_property_read_only("Height"));
        assert!(d.is_property_read_only("Model"));
        assert!(!d.is_property_read_only("Exposure"));
    }
}
