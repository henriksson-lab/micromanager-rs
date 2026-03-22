use crate::ffi::*;
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Camera, Device};
use micromanager::types::{DeviceType, ImageRoi, PropertyValue};
use std::ptr;

// Safety: dc1394 camera handles are not shared across threads in this adapter.
unsafe impl Send for IIDCCamera {}

// ─── Video mode table ────────────────────────────────────────────────────────

struct VideoModeEntry {
    id: dc1394video_mode_t,
    name: &'static str,
    width: u32,
    height: u32,
    color_coding: dc1394color_coding_t,
    bytes_per_pixel: u32,
    bit_depth: u32,
    num_components: u32,
}

static VIDEO_MODES: &[VideoModeEntry] = &[
    VideoModeEntry { id: DC1394_VIDEO_MODE_160x120_YUV444,    name: "160x120_YUV444",    width: 160,  height: 120,  color_coding: DC1394_COLOR_CODING_YUV444, bytes_per_pixel: 3, bit_depth: 8,  num_components: 3 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_320x240_YUV422,    name: "320x240_YUV422",    width: 320,  height: 240,  color_coding: DC1394_COLOR_CODING_YUV422, bytes_per_pixel: 2, bit_depth: 8,  num_components: 3 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_640x480_YUV411,    name: "640x480_YUV411",    width: 640,  height: 480,  color_coding: DC1394_COLOR_CODING_YUV411, bytes_per_pixel: 2, bit_depth: 8,  num_components: 3 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_640x480_YUV422,    name: "640x480_YUV422",    width: 640,  height: 480,  color_coding: DC1394_COLOR_CODING_YUV422, bytes_per_pixel: 2, bit_depth: 8,  num_components: 3 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_640x480_RGB8,      name: "640x480_RGB8",      width: 640,  height: 480,  color_coding: DC1394_COLOR_CODING_RGB8,   bytes_per_pixel: 3, bit_depth: 8,  num_components: 3 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_640x480_MONO8,     name: "640x480_MONO8",     width: 640,  height: 480,  color_coding: DC1394_COLOR_CODING_MONO8,  bytes_per_pixel: 1, bit_depth: 8,  num_components: 1 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_640x480_MONO16,    name: "640x480_MONO16",    width: 640,  height: 480,  color_coding: DC1394_COLOR_CODING_MONO16, bytes_per_pixel: 2, bit_depth: 16, num_components: 1 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_800x600_YUV422,    name: "800x600_YUV422",    width: 800,  height: 600,  color_coding: DC1394_COLOR_CODING_YUV422, bytes_per_pixel: 2, bit_depth: 8,  num_components: 3 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_800x600_RGB8,      name: "800x600_RGB8",      width: 800,  height: 600,  color_coding: DC1394_COLOR_CODING_RGB8,   bytes_per_pixel: 3, bit_depth: 8,  num_components: 3 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_800x600_MONO8,     name: "800x600_MONO8",     width: 800,  height: 600,  color_coding: DC1394_COLOR_CODING_MONO8,  bytes_per_pixel: 1, bit_depth: 8,  num_components: 1 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_800x600_MONO16,    name: "800x600_MONO16",    width: 800,  height: 600,  color_coding: DC1394_COLOR_CODING_MONO16, bytes_per_pixel: 2, bit_depth: 16, num_components: 1 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_1024x768_YUV422,   name: "1024x768_YUV422",   width: 1024, height: 768,  color_coding: DC1394_COLOR_CODING_YUV422, bytes_per_pixel: 2, bit_depth: 8,  num_components: 3 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_1024x768_RGB8,     name: "1024x768_RGB8",     width: 1024, height: 768,  color_coding: DC1394_COLOR_CODING_RGB8,   bytes_per_pixel: 3, bit_depth: 8,  num_components: 3 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_1024x768_MONO8,    name: "1024x768_MONO8",    width: 1024, height: 768,  color_coding: DC1394_COLOR_CODING_MONO8,  bytes_per_pixel: 1, bit_depth: 8,  num_components: 1 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_1024x768_MONO16,   name: "1024x768_MONO16",   width: 1024, height: 768,  color_coding: DC1394_COLOR_CODING_MONO16, bytes_per_pixel: 2, bit_depth: 16, num_components: 1 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_1280x960_YUV422,   name: "1280x960_YUV422",   width: 1280, height: 960,  color_coding: DC1394_COLOR_CODING_YUV422, bytes_per_pixel: 2, bit_depth: 8,  num_components: 3 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_1280x960_RGB8,     name: "1280x960_RGB8",     width: 1280, height: 960,  color_coding: DC1394_COLOR_CODING_RGB8,   bytes_per_pixel: 3, bit_depth: 8,  num_components: 3 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_1280x960_MONO8,    name: "1280x960_MONO8",    width: 1280, height: 960,  color_coding: DC1394_COLOR_CODING_MONO8,  bytes_per_pixel: 1, bit_depth: 8,  num_components: 1 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_1280x960_MONO16,   name: "1280x960_MONO16",   width: 1280, height: 960,  color_coding: DC1394_COLOR_CODING_MONO16, bytes_per_pixel: 2, bit_depth: 16, num_components: 1 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_1600x1200_YUV422,  name: "1600x1200_YUV422",  width: 1600, height: 1200, color_coding: DC1394_COLOR_CODING_YUV422, bytes_per_pixel: 2, bit_depth: 8,  num_components: 3 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_1600x1200_RGB8,    name: "1600x1200_RGB8",    width: 1600, height: 1200, color_coding: DC1394_COLOR_CODING_RGB8,   bytes_per_pixel: 3, bit_depth: 8,  num_components: 3 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_1600x1200_MONO8,   name: "1600x1200_MONO8",   width: 1600, height: 1200, color_coding: DC1394_COLOR_CODING_MONO8,  bytes_per_pixel: 1, bit_depth: 8,  num_components: 1 },
    VideoModeEntry { id: DC1394_VIDEO_MODE_1600x1200_MONO16,  name: "1600x1200_MONO16",  width: 1600, height: 1200, color_coding: DC1394_COLOR_CODING_MONO16, bytes_per_pixel: 2, bit_depth: 16, num_components: 1 },
];

fn mode_by_name(name: &str) -> Option<&'static VideoModeEntry> {
    VIDEO_MODES.iter().find(|m| m.name == name)
}

fn mode_by_id(id: dc1394video_mode_t) -> Option<&'static VideoModeEntry> {
    VIDEO_MODES.iter().find(|m| m.id == id)
}

/// Returns true for Format_7 scalable modes (not handled here).
fn is_format7(mode: dc1394video_mode_t) -> bool {
    mode >= DC1394_VIDEO_MODE_FORMAT7_0
}

// ─── Camera struct ───────────────────────────────────────────────────────────

pub struct IIDCCamera {
    props: PropertyMap,
    ctx: *mut dc1394_t,
    camera: *mut dc1394camera_t,
    image_buf: Vec<u8>,
    width: u32,
    height: u32,
    bytes_per_pixel: u32,
    bit_depth: u32,
    num_components: u32,
    capturing: bool,
    camera_index: usize,
    video_mode_id: dc1394video_mode_t,
}

impl IIDCCamera {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("CameraIndex", PropertyValue::Integer(0), false).unwrap();
        props.define_property("VideoMode", PropertyValue::String("640x480_MONO8".into()), false).unwrap();
        props.define_property("Exposure", PropertyValue::Float(10.0), false).unwrap();
        props.define_property("Gain", PropertyValue::Integer(0), false).unwrap();
        props.define_property("Width", PropertyValue::Integer(640), true).unwrap();
        props.define_property("Height", PropertyValue::Integer(480), true).unwrap();
        props.define_property("PixelFormat", PropertyValue::String("MONO8".into()), true).unwrap();

        Self {
            props,
            ctx: ptr::null_mut(),
            camera: ptr::null_mut(),
            image_buf: Vec::new(),
            width: 640,
            height: 480,
            bytes_per_pixel: 1,
            bit_depth: 8,
            num_components: 1,
            capturing: false,
            camera_index: 0,
            video_mode_id: DC1394_VIDEO_MODE_640x480_MONO8,
        }
    }

    fn check_initialized(&self) -> MmResult<()> {
        if self.camera.is_null() {
            Err(MmError::NotConnected)
        } else {
            Ok(())
        }
    }

    /// Apply the current video_mode_id to the hardware and update internal state.
    fn apply_video_mode(&mut self) -> MmResult<()> {
        let err = unsafe { dc1394_video_set_mode(self.camera, self.video_mode_id) };
        if err != DC1394_SUCCESS {
            return Err(MmError::LocallyDefined(format!("dc1394_video_set_mode error {}", err)));
        }

        if let Some(entry) = mode_by_id(self.video_mode_id) {
            self.width = entry.width;
            self.height = entry.height;
            self.bytes_per_pixel = entry.bytes_per_pixel;
            self.bit_depth = entry.bit_depth;
            self.num_components = entry.num_components;
            let fmt = color_coding_name(entry.color_coding);
            self.props.entry_mut("PixelFormat").map(|e| e.value = PropertyValue::String(fmt.into()));
        } else {
            // Unknown mode (e.g. Format7): query from hardware
            let mut w = 0u32;
            let mut h = 0u32;
            unsafe { dc1394_get_image_size_from_video_mode(self.camera, self.video_mode_id, &mut w, &mut h) };
            self.width = w;
            self.height = h;
        }

        self.props.entry_mut("Width").map(|e| e.value = PropertyValue::Integer(self.width as i64));
        self.props.entry_mut("Height").map(|e| e.value = PropertyValue::Integer(self.height as i64));
        Ok(())
    }

    /// Set shutter feature to manual mode and apply current exposure value.
    fn apply_exposure(&mut self) {
        let raw = self.props.get("Exposure")
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(10.0) as u32;
        unsafe {
            dc1394_feature_set_mode(self.camera, DC1394_FEATURE_SHUTTER, DC1394_FEATURE_MODE_MANUAL);
            dc1394_feature_set_value(self.camera, DC1394_FEATURE_SHUTTER, raw);
        }
    }

    /// Set gain feature to manual mode and apply current gain value.
    fn apply_gain(&mut self) {
        let raw = self.props.get("Gain")
            .ok()
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as u32;
        unsafe {
            dc1394_feature_set_mode(self.camera, DC1394_FEATURE_GAIN, DC1394_FEATURE_MODE_MANUAL);
            dc1394_feature_set_value(self.camera, DC1394_FEATURE_GAIN, raw);
        }
    }

    /// One-shot capture: set up DMA, capture one frame, tear down.
    fn capture_one_frame(&mut self) -> MmResult<()> {
        let camera = self.camera;

        let err = unsafe { dc1394_capture_setup(camera, 4, DC1394_CAPTURE_FLAGS_DEFAULT) };
        if err != DC1394_SUCCESS {
            return Err(MmError::LocallyDefined(format!("dc1394_capture_setup error {}", err)));
        }

        let err = unsafe { dc1394_video_set_transmission(camera, DC1394_ON) };
        if err != DC1394_SUCCESS {
            unsafe { dc1394_capture_stop(camera) };
            return Err(MmError::LocallyDefined(format!("dc1394_video_set_transmission error {}", err)));
        }

        let mut frame: *mut dc1394video_frame_t = ptr::null_mut();
        let err = unsafe { dc1394_capture_dequeue(camera, DC1394_CAPTURE_POLICY_WAIT, &mut frame) };
        if err != DC1394_SUCCESS || frame.is_null() {
            unsafe {
                dc1394_video_set_transmission(camera, DC1394_OFF);
                dc1394_capture_stop(camera);
            }
            return Err(MmError::SnapImageFailed);
        }

        self.copy_frame(frame);

        unsafe {
            dc1394_capture_enqueue(camera, frame);
            dc1394_video_set_transmission(camera, DC1394_OFF);
            dc1394_capture_stop(camera);
        }
        Ok(())
    }

    /// Copy image data from a dequeued frame into `self.image_buf`.
    fn copy_frame(&mut self, frame: *mut dc1394video_frame_t) {
        unsafe {
            let f = &*frame;
            // Use stride × height as the pixel data size; fall back to dimensions × bpp.
            let nbytes = if f.stride > 0 {
                (f.stride * f.size[1]) as usize
            } else {
                self.width as usize * self.height as usize * self.bytes_per_pixel as usize
            };
            self.image_buf.resize(nbytes, 0);
            if !f.image.is_null() && nbytes > 0 {
                ptr::copy_nonoverlapping(f.image, self.image_buf.as_mut_ptr(), nbytes);
            }
        }
    }
}

impl Default for IIDCCamera {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for IIDCCamera {
    fn drop(&mut self) {
        if !self.camera.is_null() {
            unsafe {
                if self.capturing {
                    dc1394_video_set_transmission(self.camera, DC1394_OFF);
                    dc1394_capture_stop(self.camera);
                }
                dc1394_camera_free(self.camera);
            }
            self.camera = ptr::null_mut();
        }
        if !self.ctx.is_null() {
            unsafe { dc1394_free(self.ctx) };
            self.ctx = ptr::null_mut();
        }
    }
}

// ─── Device trait ────────────────────────────────────────────────────────────

impl Device for IIDCCamera {
    fn name(&self) -> &str {
        "IIDCCamera"
    }

    fn description(&self) -> &str {
        "IIDC/IEEE-1394 FireWire camera (libdc1394)"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.camera.is_null() {
            return Ok(());
        }

        let ctx = unsafe { dc1394_new() };
        if ctx.is_null() {
            return Err(MmError::LocallyDefined("dc1394_new() failed — is libdc1394 installed?".into()));
        }
        self.ctx = ctx;

        // Enumerate cameras
        let mut list: *mut dc1394camera_list_t = ptr::null_mut();
        let err = unsafe { dc1394_camera_enumerate(ctx, &mut list) };
        if err != DC1394_SUCCESS || list.is_null() {
            unsafe { dc1394_free(ctx) };
            self.ctx = ptr::null_mut();
            return Err(MmError::LocallyDefined("Failed to enumerate IIDC cameras".into()));
        }

        let num = unsafe { (*list).num } as usize;
        if num == 0 {
            unsafe { dc1394_camera_free_list(list); dc1394_free(ctx) };
            self.ctx = ptr::null_mut();
            return Err(MmError::LocallyDefined("No IIDC cameras found on the bus".into()));
        }
        if self.camera_index >= num {
            unsafe { dc1394_camera_free_list(list); dc1394_free(ctx) };
            self.ctx = ptr::null_mut();
            return Err(MmError::LocallyDefined(format!(
                "Camera index {} out of range ({} camera(s) found)", self.camera_index, num
            )));
        }

        let id_ptr = unsafe { (*list).ids.add(self.camera_index) };
        let guid = unsafe { (*id_ptr).guid };
        let unit = unsafe { (*id_ptr).unit };
        unsafe { dc1394_camera_free_list(list) };

        let camera = unsafe { dc1394_camera_new_unit(ctx, guid, unit) };
        if camera.is_null() {
            unsafe { dc1394_free(ctx) };
            self.ctx = ptr::null_mut();
            return Err(MmError::LocallyDefined("Failed to open IIDC camera".into()));
        }
        self.camera = camera;

        // 400 Mbps (IEEE 1394a) is safe for all conventional cameras
        unsafe { dc1394_video_set_iso_speed(camera, DC1394_ISO_SPEED_400) };

        // Build allowed VideoMode values from camera's supported mode list
        let mut modes_list: dc1394video_modes_t = unsafe { std::mem::zeroed() };
        if unsafe { dc1394_video_get_supported_modes(camera, &mut modes_list) } == DC1394_SUCCESS {
            let supported: Vec<String> = (0..modes_list.num as usize)
                .filter_map(|i| {
                    let mid = modes_list.modes[i];
                    if is_format7(mid) { return None; }
                    mode_by_id(mid).map(|e| e.name.to_string())
                })
                .collect();
            if !supported.is_empty() {
                let refs: Vec<&str> = supported.iter().map(|s| s.as_str()).collect();
                self.props.set_allowed_values("VideoMode", &refs).ok();
            }
        }

        // Apply selected video mode
        self.apply_video_mode()?;

        // Apply exposure and gain (best-effort: camera may not support manual shutter)
        self.apply_exposure();
        self.apply_gain();

        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.capturing {
            self.stop_sequence_acquisition()?;
        }
        if !self.camera.is_null() {
            unsafe { dc1394_camera_free(self.camera) };
            self.camera = ptr::null_mut();
        }
        if !self.ctx.is_null() {
            unsafe { dc1394_free(self.ctx) };
            self.ctx = ptr::null_mut();
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "CameraIndex" => Ok(PropertyValue::Integer(self.camera_index as i64)),
            "Exposure" => {
                if !self.camera.is_null() {
                    let mut raw = 0u32;
                    unsafe { dc1394_feature_get_value(self.camera, DC1394_FEATURE_SHUTTER, &mut raw) };
                    Ok(PropertyValue::Float(raw as f64))
                } else {
                    self.props.get("Exposure").cloned()
                }
            }
            "Gain" => {
                if !self.camera.is_null() {
                    let mut raw = 0u32;
                    unsafe { dc1394_feature_get_value(self.camera, DC1394_FEATURE_GAIN, &mut raw) };
                    Ok(PropertyValue::Integer(raw as i64))
                } else {
                    self.props.get("Gain").cloned()
                }
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "CameraIndex" => {
                if !self.camera.is_null() {
                    return Err(MmError::LocallyDefined(
                        "CameraIndex cannot be changed after initialize()".into(),
                    ));
                }
                self.camera_index = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as usize;
                self.props.set(name, val)
            }
            "VideoMode" => {
                let mode_name = val.as_str().to_string();
                let entry = mode_by_name(&mode_name).ok_or(MmError::InvalidPropertyValue)?;
                self.video_mode_id = entry.id;
                // Update cached dimensions from the static table whether or not the
                // camera is open (apply_video_mode() does the hardware call too).
                self.width = entry.width;
                self.height = entry.height;
                self.bytes_per_pixel = entry.bytes_per_pixel;
                self.bit_depth = entry.bit_depth;
                self.num_components = entry.num_components;
                self.props.set(name, val)?;
                if !self.camera.is_null() {
                    self.apply_video_mode()?;
                }
                Ok(())
            }
            "Exposure" => {
                let exp = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Float(exp))?;
                if !self.camera.is_null() {
                    self.apply_exposure();
                }
                Ok(())
            }
            "Gain" => {
                let g = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props.set(name, PropertyValue::Integer(g))?;
                if !self.camera.is_null() {
                    self.apply_gain();
                }
                Ok(())
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

// ─── Camera trait ────────────────────────────────────────────────────────────

impl Camera for IIDCCamera {
    fn snap_image(&mut self) -> MmResult<()> {
        self.check_initialized()?;
        if self.capturing {
            // In sequence mode: dequeue the next available frame.
            let camera = self.camera;
            let mut frame: *mut dc1394video_frame_t = ptr::null_mut();
            let err = unsafe {
                dc1394_capture_dequeue(camera, DC1394_CAPTURE_POLICY_WAIT, &mut frame)
            };
            if err == DC1394_SUCCESS && !frame.is_null() {
                self.copy_frame(frame);
                unsafe { dc1394_capture_enqueue(camera, frame) };
            }
            Ok(())
        } else {
            self.capture_one_frame()
        }
    }

    fn get_image_buffer(&self) -> MmResult<&[u8]> {
        if self.image_buf.is_empty() {
            Err(MmError::LocallyDefined("No image captured yet — call snap_image() first".into()))
        } else {
            Ok(&self.image_buf)
        }
    }

    fn get_image_width(&self) -> u32 {
        self.width
    }

    fn get_image_height(&self) -> u32 {
        self.height
    }

    fn get_image_bytes_per_pixel(&self) -> u32 {
        self.bytes_per_pixel
    }

    fn get_bit_depth(&self) -> u32 {
        self.bit_depth
    }

    fn get_number_of_components(&self) -> u32 {
        self.num_components
    }

    fn get_number_of_channels(&self) -> u32 {
        1
    }

    fn get_exposure(&self) -> f64 {
        if !self.camera.is_null() {
            let mut raw = 0u32;
            unsafe { dc1394_feature_get_value(self.camera, DC1394_FEATURE_SHUTTER, &mut raw) };
            raw as f64
        } else {
            self.props.get("Exposure").ok().and_then(|v| v.as_f64()).unwrap_or(10.0)
        }
    }

    fn set_exposure(&mut self, exp_ms: f64) {
        self.props.set("Exposure", PropertyValue::Float(exp_ms)).ok();
        if !self.camera.is_null() {
            self.apply_exposure();
        }
    }

    fn get_binning(&self) -> i32 {
        1
    }

    fn set_binning(&mut self, bin: i32) -> MmResult<()> {
        if bin == 1 {
            Ok(())
        } else {
            Err(MmError::NotSupported)
        }
    }

    fn get_roi(&self) -> MmResult<ImageRoi> {
        Ok(ImageRoi::new(0, 0, self.width, self.height))
    }

    fn set_roi(&mut self, _roi: ImageRoi) -> MmResult<()> {
        // ROI requires Format_7 scalable modes; not supported for conventional modes.
        Err(MmError::NotSupported)
    }

    fn clear_roi(&mut self) -> MmResult<()> {
        Ok(())
    }

    fn start_sequence_acquisition(&mut self, _count: i64, _interval_ms: f64) -> MmResult<()> {
        self.check_initialized()?;
        if self.capturing {
            return Ok(());
        }
        let camera = self.camera;
        let err = unsafe { dc1394_capture_setup(camera, 16, DC1394_CAPTURE_FLAGS_DEFAULT) };
        if err != DC1394_SUCCESS {
            return Err(MmError::LocallyDefined(format!("dc1394_capture_setup error {}", err)));
        }
        let err = unsafe { dc1394_video_set_transmission(camera, DC1394_ON) };
        if err != DC1394_SUCCESS {
            unsafe { dc1394_capture_stop(camera) };
            return Err(MmError::LocallyDefined(format!(
                "dc1394_video_set_transmission error {}", err
            )));
        }
        self.capturing = true;
        Ok(())
    }

    fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        if !self.capturing {
            return Ok(());
        }
        unsafe {
            dc1394_video_set_transmission(self.camera, DC1394_OFF);
            dc1394_capture_stop(self.camera);
        }
        self.capturing = false;
        Ok(())
    }

    fn is_capturing(&self) -> bool {
        self.capturing
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn color_coding_name(coding: dc1394color_coding_t) -> &'static str {
    match coding {
        DC1394_COLOR_CODING_MONO8  => "MONO8",
        DC1394_COLOR_CODING_MONO16 => "MONO16",
        DC1394_COLOR_CODING_RGB8   => "RGB8",
        DC1394_COLOR_CODING_RGB16  => "RGB16",
        DC1394_COLOR_CODING_YUV411 => "YUV411",
        DC1394_COLOR_CODING_YUV422 => "YUV422",
        DC1394_COLOR_CODING_YUV444 => "YUV444",
        DC1394_COLOR_CODING_RAW8   => "RAW8",
        DC1394_COLOR_CODING_RAW16  => "RAW16",
        _                          => "UNKNOWN",
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_properties() {
        let d = IIDCCamera::new();
        assert_eq!(d.device_type(), DeviceType::Camera);
        assert_eq!(d.get_property("CameraIndex").unwrap(), PropertyValue::Integer(0));
        assert_eq!(d.get_image_width(), 640);
        assert_eq!(d.get_image_height(), 480);
        assert_eq!(d.get_bit_depth(), 8);
        assert_eq!(d.get_binning(), 1);
        assert!(!d.is_capturing());
    }

    #[test]
    fn set_camera_index_pre_init() {
        let mut d = IIDCCamera::new();
        d.set_property("CameraIndex", PropertyValue::Integer(2)).unwrap();
        assert_eq!(d.camera_index, 2);
    }

    #[test]
    fn set_video_mode_known() {
        let mut d = IIDCCamera::new();
        d.set_property("VideoMode", PropertyValue::String("1024x768_MONO8".into())).unwrap();
        assert_eq!(d.video_mode_id, DC1394_VIDEO_MODE_1024x768_MONO8);
        // Dimensions update pre-init only for known modes
        assert_eq!(d.width, 1024);
        assert_eq!(d.height, 768);
    }

    #[test]
    fn set_unknown_video_mode_rejected() {
        let mut d = IIDCCamera::new();
        assert!(d.set_property("VideoMode", PropertyValue::String("notamode".into())).is_err());
    }

    #[test]
    fn no_image_before_snap() {
        let d = IIDCCamera::new();
        assert!(d.get_image_buffer().is_err());
    }

    #[test]
    fn initialize_without_hardware_fails_gracefully() {
        let mut d = IIDCCamera::new();
        // On a machine without FireWire cameras this should return an error, not panic.
        let result = d.initialize();
        assert!(result.is_err());
    }

    #[test]
    fn binning_only_one_supported() {
        let mut d = IIDCCamera::new();
        assert!(d.set_binning(1).is_ok());
        assert!(d.set_binning(2).is_err());
    }

    #[test]
    fn snap_without_init_errors() {
        let mut d = IIDCCamera::new();
        assert!(d.snap_image().is_err());
    }
}
