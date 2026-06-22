use std::ffi::{CStr, CString};
use std::ptr::NonNull;

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Camera, Device};
use crate::types::{DeviceType, ImageRoi, PropertyValue};

use super::ffi;

// SAFETY: TWAIN's Win32 message routing requires all calls to happen on the
// same thread; `&mut self` enforces single-thread access.
unsafe impl Send for TwainCamera {}

const BUF: usize = 4096;

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

// ── Camera struct ──────────────────────────────────────────────────────────────

pub struct TwainCamera {
    props: PropertyMap,
    ctx: Option<NonNull<ffi::TwainCtx>>,

    // Pre-init
    source_name: String, // empty = default source
    exposure_ms: f64,    // stored but not pushed to TWAIN (many sources ignore it)
    pixel_type: String,
    binning: i32,

    // Post-init (updated after every snap)
    img_width: u32,
    img_height: u32,
    bytes_per_pixel: u32,
    bit_depth: u32,
    image_buf: Vec<u8>,

    capturing: bool,
    sequence_count: i64,
    sequence_interval_ms: f64,
}

impl TwainCamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property(
                "CameraName",
                PropertyValue::String("Twain Camera".into()),
                true,
            )
            .unwrap();
        props
            .define_property("CameraID", PropertyValue::String("V2.0".into()), true)
            .unwrap();
        props
            .define_property("TwainCamera", PropertyValue::String("".into()), false)
            .unwrap();
        props
            .define_property("Binning", PropertyValue::Integer(1), false)
            .unwrap();
        props.set_allowed_values("Binning", &["1"]).unwrap();
        props
            .define_property("PixelType", PropertyValue::String("32bitRGB".into()), false)
            .unwrap();
        props
            .set_allowed_values("PixelType", &["32bitRGB"])
            .unwrap();
        props
            .define_property("Exposure", PropertyValue::Float(10.0), false)
            .unwrap();
        props
            .define_property("ScanMode", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .define_property(
                "vendor settings",
                PropertyValue::String("Hide".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("vendor settings", &["Show", "Hide"])
            .unwrap();
        props
            .define_property("Width", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("Height", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property("BitDepth", PropertyValue::Integer(32), true)
            .unwrap();
        props
            .define_property("BytesPerPixel", PropertyValue::Integer(4), true)
            .unwrap();

        Self {
            props,
            ctx: None,
            source_name: String::new(),
            exposure_ms: 10.0,
            pixel_type: "32bitRGB".into(),
            binning: 1,
            img_width: 0,
            img_height: 0,
            bytes_per_pixel: 4,
            bit_depth: 32,
            image_buf: Vec::new(),
            capturing: false,
            sequence_count: 0,
            sequence_interval_ms: 0.0,
        }
    }

    fn check_open(&self) -> MmResult<()> {
        if self.ctx.is_none() {
            Err(MmError::NotConnected)
        } else {
            Ok(())
        }
    }

    fn ctx_ptr(&self) -> MmResult<*mut ffi::TwainCtx> {
        self.ctx.map(NonNull::as_ptr).ok_or(MmError::NotConnected)
    }

    fn sync_dims(&mut self) {
        let Ok(ctx) = self.ctx_ptr() else { return };
        self.img_width = unsafe { ffi::twain_get_image_width(ctx) } as u32;
        self.img_height = unsafe { ffi::twain_get_image_height(ctx) } as u32;
        self.bytes_per_pixel = unsafe { ffi::twain_get_bytes_per_pixel(ctx) } as u32;
        self.bit_depth = unsafe { ffi::twain_get_bit_depth(ctx) } as u32;

        self.props
            .entry_mut("Width")
            .map(|e| e.value = PropertyValue::Integer(self.img_width as i64));
        self.props
            .entry_mut("Height")
            .map(|e| e.value = PropertyValue::Integer(self.img_height as i64));
        self.props
            .entry_mut("BitDepth")
            .map(|e| e.value = PropertyValue::Integer(self.bit_depth as i64));
        self.props
            .entry_mut("BytesPerPixel")
            .map(|e| e.value = PropertyValue::Integer(self.bytes_per_pixel as i64));
    }

    /// Snap timeout: generous overhead above exposure, minimum 30 s (TWAIN
    /// sources with native UIs or slow hardware can take a long time).
    fn snap_timeout_ms(&self) -> i32 {
        (self.exposure_ms as i32 + 30_000).max(30_000)
    }

    fn update_pixel_metadata_from_type(&mut self) -> MmResult<()> {
        let byte_depth = match self.pixel_type.as_str() {
            "8bit" => 1,
            "16bit" => 2,
            "32bitRGB" => 4,
            _ => return Err(MmError::InvalidPropertyValue),
        };
        self.bytes_per_pixel = byte_depth;
        self.bit_depth = byte_depth * 8;
        self.props
            .entry_mut("BitDepth")
            .map(|e| e.value = PropertyValue::Integer(self.bit_depth as i64));
        self.props
            .entry_mut("BytesPerPixel")
            .map(|e| e.value = PropertyValue::Integer(self.bytes_per_pixel as i64));
        Ok(())
    }
}

impl Default for TwainCamera {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TwainCamera {
    fn drop(&mut self) {
        if let Some(ctx) = self.ctx.take() {
            unsafe { ffi::twain_close(ctx.as_ptr()) };
        }
        unsafe { ffi::twain_close_dsm() };
    }
}

// ── Device trait ───────────────────────────────────────────────────────────────

impl Device for TwainCamera {
    fn name(&self) -> &str {
        "TwainCam"
    }
    fn description(&self) -> &str {
        "Twain Camera Device "
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.ctx.is_some() {
            return Ok(());
        }

        if unsafe { ffi::twain_init() } != 0 {
            return Err(MmError::LocallyDefined("TWAIN: failed to open DSM".into()));
        }

        // Enumerate sources so the caller can see what is available.
        let mut disc_buf = vec![0i8; BUF];
        let count = unsafe { ffi::twain_find_sources(disc_buf.as_mut_ptr(), BUF as i32) };
        if count < 0 {
            return Err(MmError::LocallyDefined(
                "TWAIN: source enumeration failed".into(),
            ));
        }
        if count == 0 {
            return Err(MmError::LocallyDefined(
                "TWAIN: no TWAIN sources found".into(),
            ));
        }

        // Build allowed values list for TwainCamera property.
        let sources_str = unsafe { CStr::from_ptr(disc_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let source_names: Vec<&str> = sources_str.split('\n').collect();
        let refs: Vec<&str> = source_names.iter().map(|s| s.trim()).collect();
        self.props.set_allowed_values("TwainCamera", &refs).ok();

        // Open selected source (or default if name is empty).
        let name_cstr = cstr(&self.source_name);
        let ptr = if self.source_name.is_empty() {
            unsafe { ffi::twain_open(std::ptr::null()) }
        } else {
            unsafe { ffi::twain_open(name_cstr.as_ptr()) }
        };

        let ctx = NonNull::new(ptr).ok_or_else(|| {
            MmError::LocallyDefined(format!(
                "TWAIN: failed to open source '{}'",
                if self.source_name.is_empty() {
                    "<default>"
                } else {
                    &self.source_name
                }
            ))
        })?;
        self.ctx = Some(ctx);

        // Record which source was actually opened.
        let opened_name = unsafe {
            CStr::from_ptr(ffi::twain_get_source_name(ctx.as_ptr()))
                .to_string_lossy()
                .into_owned()
        };
        self.source_name = opened_name.clone();
        self.props
            .entry_mut("TwainCamera")
            .map(|e| e.value = PropertyValue::String(opened_name));

        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if let Some(ctx) = self.ctx.take() {
            unsafe { ffi::twain_close(ctx.as_ptr()) };
        }
        unsafe { ffi::twain_close_dsm() };
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "TwainCamera" => Ok(PropertyValue::String(self.source_name.clone())),
            "Exposure" => Ok(PropertyValue::Float(self.exposure_ms)),
            "Binning" => Ok(PropertyValue::Integer(self.binning as i64)),
            "PixelType" => Ok(PropertyValue::String(self.pixel_type.clone())),
            "vendor settings" => Ok(PropertyValue::String("Hide".into())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "TwainCamera" => {
                if self.ctx.is_some() {
                    return Err(MmError::LocallyDefined(
                        "TwainCamera cannot be changed after initialize()".into(),
                    ));
                }
                self.source_name = val.as_str().to_string();
                self.props.set(name, val)
            }
            "Exposure" => {
                self.exposure_ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(self.exposure_ms))
            }
            "Binning" => {
                if self.capturing {
                    return Err(MmError::CameraBusyAcquiring);
                }
                let binning = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as i32;
                self.props
                    .set(name, PropertyValue::Integer(binning as i64))?;
                self.binning = binning;
                Ok(())
            }
            "PixelType" => {
                if self.capturing {
                    return Err(MmError::CameraBusyAcquiring);
                }
                let pixel_type = val.as_str().to_string();
                self.props
                    .set(name, PropertyValue::String(pixel_type.clone()))?;
                self.pixel_type = pixel_type;
                self.update_pixel_metadata_from_type()
            }
            "vendor settings" => {
                let requested = val.as_str();
                if requested != "Show" && requested != "Hide" {
                    return Err(MmError::InvalidPropertyValue);
                }
                // Upstream resets this action property to Hide after a Show request.
                self.props.set(name, PropertyValue::String("Hide".into()))
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> {
        self.props.property_names().to_vec()
    }
    fn has_property(&self, name: &str) -> bool {
        self.props.has_property(name)
    }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Camera
    }
    fn busy(&self) -> bool {
        false
    }
}

// ── Camera trait ───────────────────────────────────────────────────────────────

impl Camera for TwainCamera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_open()?;
        let timeout = self.snap_timeout_ms();
        let ctx = self.ctx_ptr()?;
        let rc = unsafe { ffi::twain_snap(ctx, timeout) };
        if rc != 0 {
            return Err(MmError::SnapImageFailed);
        }
        self.sync_dims();
        let ptr = unsafe { ffi::twain_get_frame_ptr(ctx) };
        if ptr.is_null() {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        let bytes = unsafe { ffi::twain_get_frame_bytes(ctx) } as usize;
        if bytes == 0 {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        self.image_buf.clear();
        self.image_buf
            .extend_from_slice(unsafe { std::slice::from_raw_parts(ptr, bytes) });
        Ok(())
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if self.ctx.is_none() {
            return Err(MmError::NotConnected);
        }
        if self.image_buf.is_empty() {
            return Err(MmError::LocallyDefined("No image captured yet".into()));
        }
        Ok(&self.image_buf)
    }

    fn get_image_width(&self) -> u32 {
        self.img_width
    }
    fn get_image_height(&self) -> u32 {
        self.img_height
    }
    fn get_image_bytes_per_pixel(&self) -> u32 {
        self.bytes_per_pixel.max(1)
    }
    fn get_bit_depth(&self) -> u32 {
        self.bit_depth
    }
    fn get_number_of_components(&self) -> u32 {
        match self.pixel_type.as_str() {
            "32bitRGB" => 4,
            "8bit" | "16bit" => 1,
            _ => 0,
        }
    }
    fn get_number_of_channels(&self) -> u32 {
        1
    }
    fn get_exposure(&self) -> f64 {
        self.exposure_ms
    }

    fn set_exposure(&mut self, exp_ms: f64) -> MmResult<()> {
        self.exposure_ms = exp_ms;
        self.props.set("Exposure", PropertyValue::Float(exp_ms))?;
        Ok(())
    }

    fn get_binning(&self) -> i32 {
        self.binning
    }
    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        self.set_property("Binning", PropertyValue::Integer(bin as i64))
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        Ok(ImageRoi::new(0, 0, self.img_width, self.img_height))
    }

    fn set_roi(&mut self, _roi: ImageRoi) -> MmResult<()> {
        // ROI via TWAIN capability (ICAP_FRAMES) is source-dependent; not
        // universally supported — return Ok to allow graceful degradation.
        Ok(())
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, count: i64, interval_ms: f64) -> MmResult<()> {
        self.check_open()?;
        if self.capturing {
            return Err(MmError::CameraBusyAcquiring);
        }
        // TWAIN acquisition is driven by the adapter thread upstream.  This
        // shim does not own a Core callback, so it records the bounded sequence
        // state and lets callers drive frames through snap_image().
        self.sequence_count = count;
        self.sequence_interval_ms = self.exposure_ms.max(interval_ms);
        self.capturing = true;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        self.capturing = false;
        self.sequence_count = 0;
        self.sequence_interval_ms = 0.0;
        Ok(())
    }

    fn is_capturing(&self) -> bool {
        self.capturing
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_properties() {
        let d = TwainCamera::new();
        assert_eq!(d.device_type(), DeviceType::Camera);
        assert_eq!(d.name(), "TwainCam");
        assert_eq!(d.get_exposure(), 10.0);
        assert_eq!(d.get_binning(), 1);
        assert_eq!(
            d.get_property("CameraName").unwrap(),
            PropertyValue::String("Twain Camera".into())
        );
        assert_eq!(
            d.get_property("CameraID").unwrap(),
            PropertyValue::String("V2.0".into())
        );
        assert_eq!(
            d.get_property("PixelType").unwrap(),
            PropertyValue::String("32bitRGB".into())
        );
        assert_eq!(d.get_image_bytes_per_pixel(), 4);
        assert_eq!(d.get_bit_depth(), 32);
        assert!(!d.is_capturing());
        assert_eq!(d.get_number_of_channels(), 1);
    }

    #[test]
    fn set_source_name_pre_init() {
        let mut d = TwainCamera::new();
        d.set_property("TwainCamera", PropertyValue::String("MyScanner".into()))
            .unwrap();
        assert_eq!(d.source_name, "MyScanner");
    }

    #[test]
    fn set_exposure_pre_init() {
        let mut d = TwainCamera::new();
        d.set_property("Exposure", PropertyValue::Float(250.0))
            .unwrap();
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
    fn image_buffer_is_owned_rust_storage_after_snap_copy() {
        let mut d = TwainCamera::new();
        d.ctx = Some(NonNull::<ffi::TwainCtx>::dangling());
        d.image_buf = vec![1, 2, 3, 4];

        assert_eq!(d.get_image_buffer().unwrap(), &[1, 2, 3, 4]);
        d.ctx = None;
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
        assert!(d.is_property_read_only("CameraName"));
        assert!(d.is_property_read_only("CameraID"));
        assert!(!d.is_property_read_only("TwainCamera"));
        assert!(!d.is_property_read_only("Exposure"));
    }

    #[test]
    fn twain_action_and_mode_properties_match_upstream_defaults() {
        let mut d = TwainCamera::new();
        assert!(d.has_property("TwainCamera"));
        assert!(d.has_property("ScanMode"));
        assert!(d.has_property("vendor settings"));
        assert!(d
            .set_property("Binning", PropertyValue::Integer(2))
            .is_err());
        assert!(d
            .set_property("PixelType", PropertyValue::String("8bit".into()))
            .is_err());
        d.set_property("vendor settings", PropertyValue::String("Show".into()))
            .unwrap();
        assert_eq!(
            d.get_property("vendor settings").unwrap(),
            PropertyValue::String("Hide".into())
        );
        assert!(d
            .set_property("vendor settings", PropertyValue::String("Maybe".into()))
            .is_err());
    }

    #[test]
    fn components_by_bit_depth() {
        let mut d = TwainCamera::new();
        d.pixel_type = "8bit".into();
        assert_eq!(d.get_number_of_components(), 1); // 8-bit gray
        d.pixel_type = "32bitRGB".into();
        assert_eq!(d.get_number_of_components(), 4);
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

    #[test]
    fn sequence_state_rejects_duplicate_start_and_records_actual_interval() {
        let mut d = TwainCamera::new();
        d.ctx = Some(NonNull::<ffi::TwainCtx>::dangling());
        d.set_exposure(25.0);

        d.start_sequence_acquisition(3, 10.0).unwrap();
        assert!(d.is_capturing());
        assert_eq!(d.sequence_count, 3);
        assert_eq!(d.sequence_interval_ms, 25.0);
        assert_eq!(
            d.start_sequence_acquisition(1, 1.0).unwrap_err(),
            MmError::CameraBusyAcquiring
        );

        d.stop_sequence_acquisition().unwrap();
        assert!(!d.is_capturing());
        assert_eq!(d.sequence_count, 0);
        d.ctx = None;
    }

    #[test]
    fn pixel_type_and_binning_reject_changes_while_capturing() {
        let mut d = TwainCamera::new();
        d.ctx = Some(NonNull::<ffi::TwainCtx>::dangling());
        d.start_sequence_acquisition(2, 5.0).unwrap();

        assert_eq!(
            d.set_property("PixelType", PropertyValue::String("32bitRGB".into()))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(
            d.set_property("Binning", PropertyValue::Integer(1))
                .unwrap_err(),
            MmError::CameraBusyAcquiring
        );
        assert_eq!(d.pixel_type, "32bitRGB");
        assert_eq!(d.binning, 1);

        d.stop_sequence_acquisition().unwrap();
        d.ctx = None;
    }
}
