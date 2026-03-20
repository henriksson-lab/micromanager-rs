use std::collections::HashMap;
use mm_device::error::{MmError, MmResult};
use mm_device::traits::AdapterModule;
use mm_device::types::{DeviceType, PropertyValue};

use crate::adapter_registry::AdapterRegistry;
use crate::circular_buffer::{CircularBuffer, ImageFrame};
use crate::config::{ConfigFile, ConfigGroup};
use crate::device_manager::DeviceManager;

/// The main MicroManager engine.  Mirrors the public API of `CMMCore`.
pub struct CMMCore {
    registry: AdapterRegistry,
    devices: DeviceManager,
    config_groups: HashMap<String, ConfigGroup>,
    circular_buffer: CircularBuffer,
    camera_label: Option<String>,
    shutter_label: Option<String>,
    focus_label: Option<String>,
    xy_stage_label: Option<String>,
}

impl CMMCore {
    pub fn new() -> Self {
        Self {
            registry: AdapterRegistry::new(),
            devices: DeviceManager::new(),
            config_groups: HashMap::new(),
            circular_buffer: CircularBuffer::new(256),
            camera_label: None,
            shutter_label: None,
            focus_label: None,
            xy_stage_label: None,
        }
    }

    // ─── Adapter registration ────────────────────────────────────────────────

    /// Register an adapter module so its devices can be loaded.
    pub fn register_adapter(&mut self, module: Box<dyn AdapterModule>) {
        self.registry.register(module);
    }

    // ─── Device load / unload ─────────────────────────────────────────────────

    /// Load a device from a registered adapter module and assign it a label.
    pub fn load_device(
        &mut self,
        label: &str,
        module_name: &str,
        device_name: &str,
    ) -> MmResult<()> {
        if self.devices.contains(label) {
            return Err(MmError::DuplicateLabel);
        }
        let device = self.registry.create_device(module_name, device_name)?;
        self.devices.add_device(label, module_name, device_name, device)?;
        Ok(())
    }

    /// Unload a device (calls shutdown first).
    pub fn unload_device(&mut self, label: &str) -> MmResult<()> {
        {
            let dev = self.devices.get_device_mut(label)?;
            dev.shutdown()?;
        }
        self.devices.remove_device(label)?;

        // Clear any role references to this label
        if self.camera_label.as_deref() == Some(label) {
            self.camera_label = None;
        }
        if self.shutter_label.as_deref() == Some(label) {
            self.shutter_label = None;
        }
        if self.focus_label.as_deref() == Some(label) {
            self.focus_label = None;
        }
        if self.xy_stage_label.as_deref() == Some(label) {
            self.xy_stage_label = None;
        }
        Ok(())
    }

    /// Initialize all loaded devices.
    pub fn initialize_all_devices(&mut self) -> MmResult<()> {
        let labels: Vec<String> = self.devices.labels().iter().map(|s| s.to_string()).collect();
        for label in labels {
            self.devices.get_device_mut(&label)?.initialize()?;
        }
        Ok(())
    }

    /// Initialize a single device by label.
    pub fn initialize_device(&mut self, label: &str) -> MmResult<()> {
        self.devices.get_device_mut(label)?.initialize()
    }

    // ─── Role assignment ──────────────────────────────────────────────────────

    pub fn set_camera_device(&mut self, label: &str) -> MmResult<()> {
        self.ensure_type(label, DeviceType::Camera)?;
        self.camera_label = Some(label.to_string());
        Ok(())
    }

    pub fn set_shutter_device(&mut self, label: &str) -> MmResult<()> {
        self.ensure_type(label, DeviceType::Shutter)?;
        self.shutter_label = Some(label.to_string());
        Ok(())
    }

    pub fn set_focus_device(&mut self, label: &str) -> MmResult<()> {
        self.ensure_type(label, DeviceType::Stage)?;
        self.focus_label = Some(label.to_string());
        Ok(())
    }

    pub fn set_xy_stage_device(&mut self, label: &str) -> MmResult<()> {
        self.ensure_type(label, DeviceType::XYStage)?;
        self.xy_stage_label = Some(label.to_string());
        Ok(())
    }

    fn ensure_type(&self, label: &str, expected: DeviceType) -> MmResult<()> {
        let dev = self.devices.get_device(label)?;
        if dev.device_type() != expected {
            return Err(MmError::WrongDeviceType);
        }
        Ok(())
    }

    // ─── Property access ─────────────────────────────────────────────────────

    pub fn get_property(&self, label: &str, prop: &str) -> MmResult<PropertyValue> {
        self.devices.get_device(label)?.get_property(prop)
    }

    pub fn set_property(&mut self, label: &str, prop: &str, value: PropertyValue) -> MmResult<()> {
        self.devices.get_device_mut(label)?.set_property(prop, value)
    }

    pub fn get_property_names(&self, label: &str) -> MmResult<Vec<String>> {
        Ok(self.devices.get_device(label)?.property_names())
    }

    // ─── Camera operations ────────────────────────────────────────────────────

    fn camera_label(&self) -> MmResult<String> {
        self.camera_label
            .clone()
            .ok_or(MmError::CoreFocusStageUndef)
    }

    /// Snap a single image using the current camera.
    pub fn snap_image(&mut self) -> MmResult<()> {
        let label = self.camera_label()?;
        self.devices
            .get_mut(&label)?
            .as_camera_mut()
            .ok_or(MmError::WrongDeviceType)?
            .snap_image()
    }

    /// Get the image from the last snap as an `ImageFrame`.
    pub fn get_image(&self) -> MmResult<ImageFrame> {
        let label = self.camera_label()?;
        let cam = self.devices
            .get(&label)?
            .as_camera()
            .ok_or(MmError::WrongDeviceType)?;
        let data = cam.get_image_buffer()?.to_vec();
        let w = cam.get_image_width();
        let h = cam.get_image_height();
        let bpp = cam.get_image_bytes_per_pixel();
        Ok(ImageFrame::new(data, w, h, bpp))
    }

    pub fn set_exposure(&mut self, exp_ms: f64) -> MmResult<()> {
        let label = self.camera_label()?;
        self.devices
            .get_mut(&label)?
            .as_camera_mut()
            .ok_or(MmError::WrongDeviceType)?
            .set_exposure(exp_ms);
        Ok(())
    }

    pub fn get_exposure(&self) -> MmResult<f64> {
        let label = self.camera_label()?;
        Ok(self.devices
            .get(&label)?
            .as_camera()
            .ok_or(MmError::WrongDeviceType)?
            .get_exposure())
    }

    pub fn start_sequence_acquisition(&mut self, count: i64, interval_ms: f64) -> MmResult<()> {
        let label = self.camera_label()?;
        self.devices
            .get_mut(&label)?
            .as_camera_mut()
            .ok_or(MmError::WrongDeviceType)?
            .start_sequence_acquisition(count, interval_ms)
    }

    pub fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        let label = self.camera_label()?;
        self.devices
            .get_mut(&label)?
            .as_camera_mut()
            .ok_or(MmError::WrongDeviceType)?
            .stop_sequence_acquisition()
    }

    pub fn is_sequence_running(&self) -> MmResult<bool> {
        let label = self.camera_label()?;
        Ok(self.devices
            .get(&label)?
            .as_camera()
            .ok_or(MmError::WrongDeviceType)?
            .is_capturing())
    }

    // ─── Stage (Z focus) operations ───────────────────────────────────────────

    fn focus_label(&self) -> MmResult<String> {
        self.focus_label.clone().ok_or(MmError::CoreFocusStageUndef)
    }

    pub fn set_position(&mut self, pos_um: f64) -> MmResult<()> {
        let label = self.focus_label()?;
        self.devices
            .get_mut(&label)?
            .as_stage_mut()
            .ok_or(MmError::WrongDeviceType)?
            .set_position_um(pos_um)
    }

    pub fn get_position(&self) -> MmResult<f64> {
        let label = self.focus_label()?;
        self.devices
            .get(&label)?
            .as_stage()
            .ok_or(MmError::WrongDeviceType)?
            .get_position_um()
    }

    pub fn set_relative_position(&mut self, d_um: f64) -> MmResult<()> {
        let label = self.focus_label()?;
        self.devices
            .get_mut(&label)?
            .as_stage_mut()
            .ok_or(MmError::WrongDeviceType)?
            .set_relative_position_um(d_um)
    }

    // ─── XY Stage operations ─────────────────────────────────────────────────

    fn xy_stage_label(&self) -> MmResult<String> {
        self.xy_stage_label
            .clone()
            .ok_or(MmError::CoreFocusStageUndef)
    }

    pub fn set_xy_position(&mut self, x: f64, y: f64) -> MmResult<()> {
        let label = self.xy_stage_label()?;
        self.devices
            .get_mut(&label)?
            .as_xystage_mut()
            .ok_or(MmError::WrongDeviceType)?
            .set_xy_position_um(x, y)
    }

    pub fn get_xy_position(&self) -> MmResult<(f64, f64)> {
        let label = self.xy_stage_label()?;
        self.devices
            .get(&label)?
            .as_xystage()
            .ok_or(MmError::WrongDeviceType)?
            .get_xy_position_um()
    }

    // ─── Shutter operations ───────────────────────────────────────────────────

    fn shutter_label(&self) -> MmResult<String> {
        self.shutter_label.clone().ok_or(MmError::NotConnected)
    }

    pub fn set_shutter_open(&mut self, open: bool) -> MmResult<()> {
        let label = self.shutter_label()?;
        self.devices
            .get_mut(&label)?
            .as_shutter_mut()
            .ok_or(MmError::WrongDeviceType)?
            .set_open(open)
    }

    pub fn get_shutter_open(&self) -> MmResult<bool> {
        let label = self.shutter_label()?;
        self.devices
            .get(&label)?
            .as_shutter()
            .ok_or(MmError::WrongDeviceType)?
            .get_open()
    }

    // ─── Circular buffer ─────────────────────────────────────────────────────

    pub fn pop_next_image(&self) -> Option<ImageFrame> {
        self.circular_buffer.pop()
    }

    pub fn get_remaining_image_count(&self) -> usize {
        self.circular_buffer.len()
    }

    /// Insert a frame directly into the ring buffer (called by adapters during sequence acq.).
    pub fn insert_image(&self, frame: ImageFrame) {
        self.circular_buffer.push(frame);
    }

    // ─── Config groups ────────────────────────────────────────────────────────

    pub fn define_config(
        &mut self,
        group: &str,
        preset: &str,
        device_label: &str,
        prop: &str,
        value: &str,
    ) {
        self.config_groups
            .entry(group.to_string())
            .or_default()
            .add_setting(preset, device_label, prop, value);
    }

    pub fn set_config(&mut self, group: &str, preset: &str) -> MmResult<()> {
        let settings = self
            .config_groups
            .get(group)
            .ok_or_else(|| MmError::UnknownLabel(group.to_string()))?
            .get_preset(preset)
            .ok_or_else(|| MmError::UnknownLabel(preset.to_string()))?
            .to_vec();

        for s in settings {
            let val = PropertyValue::String(s.value.clone());
            self.devices.get_device_mut(&s.device_label)?.set_property(&s.property_name, val)?;
        }
        Ok(())
    }

    // ─── Config file I/O ──────────────────────────────────────────────────────

    /// Load a configuration file, creating and initializing all devices.
    pub fn load_system_configuration(&mut self, text: &str) -> MmResult<()> {
        let cfg = ConfigFile::parse(text)?;

        for (label, module, device) in &cfg.devices {
            self.load_device(label, module, device)?;
        }
        for (label, prop, value) in &cfg.properties {
            let val = PropertyValue::String(value.clone());
            self.devices.get_device_mut(label)?.set_property(prop, val)?;
        }
        for (group_name, group) in cfg.config_groups {
            self.config_groups.insert(group_name, group);
        }
        Ok(())
    }

    /// Serialize the current system configuration to a .cfg string.
    pub fn save_system_configuration(&self) -> MmResult<String> {
        let mut devices = Vec::new();
        let mut properties = Vec::new();

        for label in self.devices.labels() {
            let handle = self.devices.entry_ref(label)?;
            let dev = handle.device.as_device();
            devices.push((
                label.to_string(),
                handle.module_name.clone(),
                handle.device_name.clone(),
            ));
            for prop_name in dev.property_names() {
                if dev.is_property_read_only(&prop_name) {
                    continue;
                }
                if let Ok(val) = dev.get_property(&prop_name) {
                    properties.push((label.to_string(), prop_name, val.to_string()));
                }
            }
        }

        let cfg = ConfigFile {
            devices,
            properties,
            config_groups: self.config_groups.clone(),
            parents: Vec::new(),
        };
        Ok(cfg.to_text())
    }

    // ─── Utility ─────────────────────────────────────────────────────────────

    pub fn device_labels(&self) -> Vec<&str> {
        self.devices.labels()
    }

    pub fn get_device_type(&self, label: &str) -> MmResult<DeviceType> {
        Ok(self.devices.get_device(label)?.device_type())
    }
}

impl Default for CMMCore {
    fn default() -> Self {
        Self::new()
    }
}
