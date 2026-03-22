use std::ffi::{CStr, CString};

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Camera, Device};
use micromanager::types::{DeviceType, ImageRoi, PropertyValue};

use crate::ffi;

// SAFETY: TwainCamera holds a raw pointer to TwainCtx.  TWAIN's Win32 message
// routing requires all calls to happen on the same thread; `&mut self` enforces
// single-thread access.
unsafe impl Send for TwainCamera {}

const BUF: usize = 4096;

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

// ── Camera struct ──────────────────────────────────────────────────────────────

pub struct TwainCamera {
    props:   PropertyMap,
    ctx:     *mut ffi::TwainCtx,

    // Pre-init
    source_name: String,   // empty = default source
    exposure_ms: f64,      // stored but not pushed to TWAIN (many sources ignore it)

    // Post-init (updated after every snap)
    img_width:       u32,
    img_height:      u32,
    bytes_per_pixel: u32,
    bit_depth:       u32,

    capturing: bool,
}

impl TwainCamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("SourceName",  PropertyValue::String("".into()),  false).unwrap();
        props.define_property("Exposure",    PropertyValue::Float(100.0),       false).unwrap();
        props.define_property("Width",       PropertyValue::Integer(0),         true).unwrap();
        props.define_property("Height",      PropertyValue::Integer(0),         true).unwrap();
        props.define_property("BitDepth",    PropertyValue::Integer(8),         true).unwrap();
        props.define_property("BytesPerPixel", PropertyValue::Integer(1),       true).unwrap();

        Self {
            props,
            ctx: std::ptr::null_mut(),
            source_name:     String::new(),
            exposure_ms:     100.0,
            img_width:       0,
            img_height:      0,
            bytes_per_pixel: 1,
            bit_depth:       8,
            capturing:       false,
        }
    }

    fn check_open(&self) -> MmResult<()> {
        if self.ctx.is_null() { Err(MmError::NotConnected) } else { Ok(()) }
    }

    fn sync_dims(&mut self) {
        if self.ctx.is_null() { return; }
        self.img_width       = unsafe { ffi::twain_get_image_width(self.ctx)       } as u32;
        self.img_height      = unsafe { ffi::twain_get_image_height(self.ctx)      } as u32;
        self.bytes_per_pixel = unsafe { ffi::twain_get_bytes_per_pixel(self.ctx)   } as u32;
        self.bit_depth       = unsafe { ffi::twain_get_bit_depth(self.ctx)         } as u32;

        self.props.entry_mut("Width").map(|e| e.value = PropertyValue::Integer(self.img_width as i64));
        self.props.entry_mut("Height").map(|e| e.value = PropertyValue::Integer(self.img_height as i64));
        self.props.entry_mut("BitDepth").map(|e| e.value = PropertyValue::Integer(self.bit_depth as i64));
        self.props.entry_mut("BytesPerPixel").map(|e| e.value = PropertyValue::Integer(self.bytes_per_pixel as i64));
    }

    /// Snap timeout: generous overhead above exposure, minimum 30 s (TWAIN
    /// sources with native UIs or slow hardware can take a long time).
    fn snap_timeout_ms(&self) -> i32 {
        (self.exposure_ms as i32 + 30_000).max(30_000)
    }
}

impl Default for TwainCamera {
    fn default() -> Self { Self::new() }
}

impl Drop for TwainCamera {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            unsafe { ffi::twain_close(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
        unsafe { ffi::twain_close_dsm() };
    }
}

// ── Device trait ───────────────────────────────────────────────────────────────

impl Device for TwainCamera {
    fn name(&self) -> &str { "TwainCamera" }
    fn description(&self) -> &str { "Generic TWAIN camera adapter" }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.ctx.is_null() { return Ok(()); }

        if unsafe { ffi::twain_init() } != 0 {
            return Err(MmError::LocallyDefined("TWAIN: failed to open DSM".into()));
        }

        // Enumerate sources so the caller can see what is available.
        let mut disc_buf = vec![0i8; BUF];
        let count = unsafe { ffi::twain_find_sources(disc_buf.as_mut_ptr(), BUF as i32) };
        if count < 0 {
            return Err(MmError::LocallyDefined("TWAIN: source enumeration failed".into()));
        }
        if count == 0 {
            return Err(MmError::LocallyDefined("TWAIN: no TWAIN sources found".into()));
        }

        // Build allowed values list for SourceName property.
        let sources_str = unsafe { CStr::from_ptr(disc_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let source_names: Vec<&str> = sources_str.split('\n').collect();
        let refs: Vec<&str> = source_names.iter().map(|s| s.trim()).collect();
        self.props.set_allowed_values("SourceName", &refs).ok();

        // Open selected source (or default if name is empty).
        let name_cstr = cstr(&self.source_name);
        let ptr = if self.source_name.is_empty() {
            unsafe { ffi::twain_open(std::ptr::null()) }
        } else {
            unsafe { ffi::twain_open(name_cstr.as_ptr()) }
        };

        if ptr.is_null() {
            return Err(MmError::LocallyDefined(format!(
                "TWAIN: failed to open source '{}'",
                if self.source_name.is_empty() { "<default>" } else { &self.source_name }
            )));
        }
        self.ctx = ptr;

        // Record which source was actually opened.
        let opened_name = unsafe {
            CStr::from_ptr(ffi::twain_get_source_name(self.ctx))
                .to_string_lossy()
                .into_owned()
        };
        self.source_name = opened_name.clone();
        self.props.entry_mut("SourceName")
            .map(|e| e.value = PropertyValue::String(opened_name));

        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if !self.ctx.is_null() {
            unsafe { ffi::twain_close(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
        unsafe { ffi::twain_close_dsm() };
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "SourceName" => Ok(PropertyValue::String(self.source_name.clone())),
            "Exposure"   => Ok(PropertyValue::Float(self.exposure_ms)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "SourceName" => {
                if !self.ctx.is_null() {
                    return Err(MmError::LocallyDefined(
                        "SourceName cannot be changed after initialize()".into(),
                    ));
                }
                self.source_name = val.as_str().to_string();
                self.props.set(name, val)
            }
            "Exposure" => {
                self.exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.exposure_ms))
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

// ── Camera trait ───────────────────────────────────────────────────────────────

impl Camera for TwainCamera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_open()?;
        let timeout = self.snap_timeout_ms();
        let rc = unsafe { ffi::twain_snap(self.ctx, timeout) };
        if rc != 0 { return Err(MmError::SnapImageFailed); }
        self.sync_dims();
        Ok(())
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if self.ctx.is_null() { return Err(MmError::NotConnected); }
        let ptr = unsafe { ffi::twain_get_frame_ptr(self.ctx) };
        if ptr.is_null() {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        let bytes = unsafe { ffi::twain_get_frame_bytes(self.ctx) } as usize;
        if bytes == 0 {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        // SAFETY: ptr points into the shim's internal buffer which lives for
        // the duration of ctx; borrowed with the same lifetime as &self.
        Ok(unsafe { std::slice::from_raw_parts(ptr, bytes) })
    }

    fn get_image_width(&self) -> u32  { self.img_width }
    fn get_image_height(&self) -> u32 { self.img_height }
    fn get_image_bytes_per_pixel(&self) -> u32 { self.bytes_per_pixel.max(1) }
    fn get_bit_depth(&self) -> u32 { self.bit_depth }
    fn get_number_of_components(&self) -> u32 {
        // TWAIN commonly delivers 8-bit gray (1 bpp) or 24-bit RGB (3 bpp).
        if self.bytes_per_pixel >= 3 { 3 } else { 1 }
    }
    fn get_number_of_channels(&self) -> u32 { 1 }
    fn get_exposure(&self) -> f64 { self.exposure_ms }

    fn set_exposure(&mut self, exp_ms: f64) {
        self.exposure_ms = exp_ms;
        self.props.set("Exposure", PropertyValue::Float(exp_ms)).ok();
    }

    fn get_binning(&self) -> i32 { 1 }
    fn set_binning(&mut self, _bin: i32) -> MmResult<()> { Ok(()) }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        Ok(ImageRoi::new(0, 0, self.img_width, self.img_height))
    }

    fn set_roi(&mut self, _roi: ImageRoi) -> MmResult<()> {
        // ROI via TWAIN capability (ICAP_FRAMES) is source-dependent; not
        // universally supported — return Ok to allow graceful degradation.
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> { Ok(()) }

    fn start_sequence_acquisition(&mut self, _count: i64, _interval_ms: f64) -> MmResult<()> {
        self.check_open()?;
        // TWAIN has no native continuous mode; flag capturing and let the
        // caller drive frame-by-frame via snap_image().
        self.capturing = true;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        self.capturing = false;
        Ok(())
    }

    fn is_capturing(&self) -> bool { self.capturing }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_properties() {
        let d = TwainCamera::new();
        assert_eq!(d.device_type(), DeviceType::Camera);
        assert_eq!(d.get_exposure(), 100.0);
        assert_eq!(d.get_binning(), 1);
        assert!(!d.is_capturing());
        assert_eq!(d.get_number_of_channels(), 1);
    }

    #[test]
    fn set_source_name_pre_init() {
        let mut d = TwainCamera::new();
        d.set_property("SourceName", PropertyValue::String("MyScanner".into())).unwrap();
        assert_eq!(d.source_name, "MyScanner");
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = TwainCamera::new();
        d.set_property("Exposure", PropertyValue::Float(250.0)).unwrap();
        assert_eq!(d.exposure_ms, 250.0);
        assert_eq!(d.get_exposure(), 250.0);
    }

    #[test]
    fn snap_without_init_errors() {
        let mut d = TwainCamera::new();
        assert!(d.snap_image().is_err());
    }

    #[test]
    fn no_image_before_snap() {
        let d = TwainCamera::new();
        assert!(d.get_image_buffer().is_err());
    }

    #[test]
    fn initialize_no_dsm_fails() {
        let mut d = TwainCamera::new();
        // No TWAIN DSM present on this system — expect an error.
        assert!(d.initialize().is_err());
    }

    #[test]
    fn readonly_properties() {
        let d = TwainCamera::new();
        assert!(d.is_property_read_only("Width"));
        assert!(d.is_property_read_only("Height"));
        assert!(d.is_property_read_only("BitDepth"));
        assert!(d.is_property_read_only("BytesPerPixel"));
        assert!(!d.is_property_read_only("SourceName"));
        assert!(!d.is_property_read_only("Exposure"));
    }

    #[test]
    fn components_by_bit_depth() {
        let mut d = TwainCamera::new();
        d.bytes_per_pixel = 1;
        assert_eq!(d.get_number_of_components(), 1);   // 8-bit gray
        d.bytes_per_pixel = 3;
        assert_eq!(d.get_number_of_components(), 3);   // 24-bit RGB
    }

    #[test]
    fn sequence_flag() {
        let mut d = TwainCamera::new();
        assert!(!d.is_capturing());
        // start_sequence_acquisition requires open ctx; just test stop
        d.stop_sequence_acquisition().unwrap();
        assert!(!d.is_capturing());
    }

    #[test]
    fn timeout_at_least_30s() {
        let d = TwainCamera::new();
        assert!(d.snap_timeout_ms() >= 30_000);
    }
}
