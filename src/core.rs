use crate::error::{MmError, MmResult};
use crate::traits::{AdapterModule, SequenceImageSink};
use crate::types::{DeviceType, PropertyValue};
use std::collections::HashMap;
use std::sync::Arc;

use crate::adapter_registry::AdapterRegistry;
use crate::adapters;
use crate::circular_buffer::{CircularBuffer, ImageFrame};
use crate::config::{ConfigFile, ConfigGroup, ConfigRecord};
use crate::device_manager::DeviceManager;
use std::thread;
use std::time::Duration;

impl SequenceImageSink for CircularBuffer {
    fn insert_sequence_image(&self, frame: ImageFrame) -> bool {
        self.push(frame)
    }
}

/// The main MicroManager engine.  Mirrors the public API of `CMMCore`.
pub struct CMMCore {
    registry: AdapterRegistry,
    devices: DeviceManager,
    config_groups: HashMap<String, ConfigGroup>,
    circular_buffer: Arc<CircularBuffer>,
    camera_label: Option<String>,
    shutter_label: Option<String>,
    focus_label: Option<String>,
    xy_stage_label: Option<String>,
    auto_shutter: bool,
    parent_labels: HashMap<String, String>,
    core_initialized: bool,
    sequence_auto_shutter_opened: bool,
}

impl CMMCore {
    pub fn new() -> Self {
        let mut core = Self {
            registry: AdapterRegistry::new(),
            devices: DeviceManager::new(),
            config_groups: HashMap::new(),
            circular_buffer: Arc::new(CircularBuffer::new(256)),
            camera_label: None,
            shutter_label: None,
            focus_label: None,
            xy_stage_label: None,
            auto_shutter: true,
            parent_labels: HashMap::new(),
            core_initialized: false,
            sequence_auto_shutter_opened: false,
        };
        core.register_all_adapters();
        core
    }

    // ─── Adapter registration ────────────────────────────────────────────────

    /// Register all built-in adapter modules.
    pub fn register_all_adapters(&mut self) {
        self.register_adapter(Box::new(adapters::arduino::ArduinoAdapter));
        self.register_adapter(Box::new(adapters::arduino32::Arduino32Adapter));
        self.register_adapter(Box::new(adapters::arduino_counter::ArduinoCounterAdapter));
        self.register_adapter(Box::new(adapters::asi_stage::AsiStageAdapter));
        self.register_adapter(Box::new(adapters::asi_tiger::AsiTigerAdapter));
        self.register_adapter(Box::new(adapters::chuoseiki::ChuoSeikiAdapter));
        self.register_adapter(Box::new(adapters::chuoseiki_qt::ChuoSeikiQtAdapter));
        self.register_adapter(Box::new(adapters::cobolt::CoboltAdapter));
        self.register_adapter(Box::new(adapters::cobolt_official::CoboltOfficialAdapter));
        self.register_adapter(Box::new(adapters::coherent_cube::CoherentCubeAdapter));
        self.register_adapter(Box::new(
            adapters::coherent_scientific_remote::CoherentScientificRemoteAdapter,
        ));
        self.register_adapter(Box::new(adapters::conix::ConixAdapter));
        self.register_adapter(Box::new(adapters::corvus::CorvusAdapter));
        self.register_adapter(Box::new(adapters::demo::DemoAdapter));
        self.register_adapter(Box::new(adapters::esp32::Esp32Adapter));
        self.register_adapter(Box::new(adapters::hydra_lmt200::HydraLmt200Adapter));
        self.register_adapter(Box::new(adapters::marzhauser::MarzhauserAdapter));
        self.register_adapter(Box::new(adapters::marzhauser_lstep::MarzhauserLStepAdapter));
        self.register_adapter(Box::new(
            adapters::marzhauser_lstep_old::MarzhauserLStepOldAdapter,
        ));
        self.register_adapter(Box::new(adapters::microfpga::MicroFpgaAdapter));
        self.register_adapter(Box::new(adapters::mpb_laser::MpbLaserAdapter));
        self.register_adapter(Box::new(adapters::newport_stage::NewportStageAdapter));
        self.register_adapter(Box::new(adapters::openflexure::OpenFlexureAdapter));
        self.register_adapter(Box::new(adapters::openuc2::Uc2Adapter));
        self.register_adapter(Box::new(adapters::oxxius_laserboxx::OxxiusLaserBoxxAdapter));
        self.register_adapter(Box::new(adapters::prior::PriorAdapter));
        self.register_adapter(Box::new(adapters::prior_legacy::PriorLegacyAdapter));
        self.register_adapter(Box::new(adapters::prior_purefocus::PriorPureFocusAdapter));
        self.register_adapter(Box::new(adapters::prizmatix::PrizmatixAdapter));
        self.register_adapter(Box::new(adapters::scientifica::ScientificaAdapter));
        self.register_adapter(Box::new(
            adapters::scientifica_motion8::ScientificaMotion8Adapter,
        ));
        self.register_adapter(Box::new(adapters::squid_plus::SquidPlusAdapter));
        self.register_adapter(Box::new(adapters::sutter_stage::SutterStageAdapter));
        self.register_adapter(Box::new(adapters::teensy_pulse::TeensyPulseAdapter));
        self.register_adapter(Box::new(adapters::tofra::TofraAdapter));
        self.register_adapter(Box::new(adapters::toptica_ibeam::TopticaIBeamAdapter));
        self.register_adapter(Box::new(adapters::wienecke_sinske::WieneckeSinskeAdapter));
        self.register_adapter(Box::new(adapters::xeryon::XeryonAdapter));
        self.register_adapter(Box::new(adapters::yodn_e600::YodnE600Adapter));
        self.register_adapter(Box::new(adapters::zaber::ZaberAdapter));
    }

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
        self.devices
            .add_device(label, module_name, device_name, device)?;
        Ok(())
    }

    /// Unload a device (calls shutdown first).
    pub fn unload_device(&mut self, label: &str) -> MmResult<()> {
        {
            let dev = self.devices.get_device_mut(label)?;
            dev.shutdown()?;
        }
        self.devices.remove_device(label)?;
        self.parent_labels
            .retain(|child, parent| child != label && parent != label);

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
        let labels: Vec<String> = self
            .devices
            .labels()
            .iter()
            .map(|s| s.to_string())
            .collect();
        for label in labels {
            self.devices.get_device_mut(&label)?.initialize()?;
        }
        self.core_initialized = true;
        Ok(())
    }

    /// Initialize a single device by label.
    pub fn initialize_device(&mut self, label: &str) -> MmResult<()> {
        self.devices.get_device_mut(label)?.initialize()
    }

    // ─── Role assignment ──────────────────────────────────────────────────────

    pub fn set_camera_device(&mut self, label: &str) -> MmResult<()> {
        if label.is_empty() {
            self.camera_label = None;
            return Ok(());
        }
        self.ensure_type(label, DeviceType::Camera)?;
        self.camera_label = Some(label.to_string());
        Ok(())
    }

    pub fn set_shutter_device(&mut self, label: &str) -> MmResult<()> {
        if label.is_empty() {
            self.shutter_label = None;
            return Ok(());
        }
        self.ensure_type(label, DeviceType::Shutter)?;
        self.shutter_label = Some(label.to_string());
        Ok(())
    }

    pub fn set_focus_device(&mut self, label: &str) -> MmResult<()> {
        if label.is_empty() {
            self.focus_label = None;
            return Ok(());
        }
        self.ensure_type(label, DeviceType::Stage)?;
        self.focus_label = Some(label.to_string());
        Ok(())
    }

    pub fn set_xy_stage_device(&mut self, label: &str) -> MmResult<()> {
        if label.is_empty() {
            self.xy_stage_label = None;
            return Ok(());
        }
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
        if label == "Core" {
            return self.get_core_property(prop);
        }
        self.devices.get_device(label)?.get_property(prop)
    }

    pub fn set_property(&mut self, label: &str, prop: &str, value: PropertyValue) -> MmResult<()> {
        if label == "Core" {
            return self.set_core_property(prop, value);
        }
        self.devices
            .get_device_mut(label)?
            .set_property(prop, value)
    }

    pub fn get_property_names(&self, label: &str) -> MmResult<Vec<String>> {
        if label == "Core" {
            return Ok(vec![
                "Camera".to_string(),
                "Shutter".to_string(),
                "Focus".to_string(),
                "XYStage".to_string(),
                "AutoShutter".to_string(),
                "Initialize".to_string(),
            ]);
        }
        Ok(self.devices.get_device(label)?.property_names())
    }

    fn get_core_property(&self, prop: &str) -> MmResult<PropertyValue> {
        let value = match prop {
            "Camera" => self.camera_label.clone().unwrap_or_default(),
            "Shutter" => self.shutter_label.clone().unwrap_or_default(),
            "Focus" => self.focus_label.clone().unwrap_or_default(),
            "XYStage" => self.xy_stage_label.clone().unwrap_or_default(),
            "AutoShutter" => {
                return Ok(PropertyValue::Integer(if self.auto_shutter {
                    1
                } else {
                    0
                }));
            }
            "Initialize" => {
                return Ok(PropertyValue::Integer(if self.core_initialized {
                    1
                } else {
                    0
                }))
            }
            _ => return Err(MmError::InvalidProperty),
        };
        Ok(PropertyValue::String(value))
    }

    fn set_core_property(&mut self, prop: &str, value: PropertyValue) -> MmResult<()> {
        let value = value.to_string();
        match prop {
            "Camera" => self.set_role_from_core_property(value, DeviceType::Camera),
            "Shutter" => self.set_role_from_core_property(value, DeviceType::Shutter),
            "Focus" => self.set_role_from_core_property(value, DeviceType::Stage),
            "XYStage" => self.set_role_from_core_property(value, DeviceType::XYStage),
            "AutoShutter" => {
                self.auto_shutter = parse_core_bool(&value)?;
                Ok(())
            }
            "Initialize" => match value.as_str() {
                "0" => self.reset_loaded_devices(),
                "1" => self.initialize_all_devices(),
                _ => Err(MmError::InvalidPropertyValue),
            },
            _ => Err(MmError::InvalidProperty),
        }
    }

    fn set_role_from_core_property(
        &mut self,
        value: String,
        device_type: DeviceType,
    ) -> MmResult<()> {
        if value.is_empty() {
            match device_type {
                DeviceType::Camera => self.camera_label = None,
                DeviceType::Shutter => self.shutter_label = None,
                DeviceType::Stage => self.focus_label = None,
                DeviceType::XYStage => self.xy_stage_label = None,
                _ => return Err(MmError::WrongDeviceType),
            }
            return Ok(());
        }
        self.ensure_type(&value, device_type)?;
        match device_type {
            DeviceType::Camera => self.camera_label = Some(value),
            DeviceType::Shutter => self.shutter_label = Some(value),
            DeviceType::Stage => self.focus_label = Some(value),
            DeviceType::XYStage => self.xy_stage_label = Some(value),
            _ => return Err(MmError::WrongDeviceType),
        }
        Ok(())
    }

    fn reset_loaded_devices(&mut self) -> MmResult<()> {
        let labels: Vec<String> = self
            .devices
            .labels()
            .iter()
            .map(|label| (*label).to_string())
            .collect();
        for label in labels {
            self.unload_device(&label)?;
        }
        self.circular_buffer.clear();
        self.parent_labels.clear();
        self.config_groups.clear();
        self.core_initialized = false;
        self.sequence_auto_shutter_opened = false;
        Ok(())
    }

    // ─── Camera operations ────────────────────────────────────────────────────

    fn camera_label(&self) -> MmResult<String> {
        self.camera_label
            .clone()
            .ok_or(MmError::CoreCameraNotAvailable)
    }

    /// Snap a single image using the current camera.
    pub fn snap_image(&mut self) -> MmResult<()> {
        let label = self.camera_label()?;
        if self
            .devices
            .get(&label)?
            .as_camera()
            .ok_or(MmError::WrongDeviceType)?
            .is_capturing()
        {
            return Err(MmError::CameraBusyAcquiring);
        }
        let shutter_was_open = if self.auto_shutter && self.shutter_label.is_some() {
            let was_open = self.get_shutter_open()?;
            if !was_open {
                self.set_shutter_open(true)?;
                self.wait_for_shutter()?;
            }
            Some(was_open)
        } else {
            None
        };
        let snap_result = self
            .devices
            .get_mut(&label)?
            .as_camera_mut()
            .ok_or(MmError::WrongDeviceType)?
            .snap_image();
        if shutter_was_open == Some(false) {
            let close_result = self
                .set_shutter_open(false)
                .and_then(|_| self.wait_for_shutter());
            snap_result.and(close_result)
        } else {
            snap_result
        }
    }

    fn wait_for_shutter(&self) -> MmResult<()> {
        if let Some(label) = self.shutter_label.as_deref() {
            while self.devices.get_device(label)?.busy() {
                thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(())
    }

    fn current_camera_frame(&self) -> MmResult<ImageFrame> {
        let label = self.camera_label()?;
        let cam = self
            .devices
            .get(&label)?
            .as_camera()
            .ok_or(MmError::WrongDeviceType)?;
        let data = cam.get_image_buffer()?.to_vec();
        Ok(ImageFrame::new(
            data,
            cam.get_image_width(),
            cam.get_image_height(),
            cam.get_image_bytes_per_pixel(),
        ))
    }

    /// CoreCallback-style helper for adapters/tests that drive sequence frames synchronously.
    pub fn snap_sequence_image_to_buffer(&mut self) -> MmResult<()> {
        let label = self.camera_label()?;
        if !self
            .devices
            .get(&label)?
            .as_camera()
            .ok_or(MmError::WrongDeviceType)?
            .is_capturing()
        {
            return Err(MmError::SequenceNotRunning);
        }
        self.devices
            .get_mut(&label)?
            .as_camera_mut()
            .ok_or(MmError::WrongDeviceType)?
            .snap_image()?;
        if !self
            .devices
            .get(&label)?
            .as_camera()
            .ok_or(MmError::WrongDeviceType)?
            .sequence_images_delivered_to_sink()
        {
            let frame = self.current_camera_frame()?;
            if self.insert_image(frame) {
                self.stop_sequence_acquisition()?;
                return Err(MmError::BufferOverflow);
            }
        }
        Ok(())
    }

    /// Get the image from the last snap as an `ImageFrame`.
    pub fn get_image(&self) -> MmResult<ImageFrame> {
        self.current_camera_frame()
    }

    pub fn set_exposure(&mut self, exp_ms: f64) -> MmResult<()> {
        let label = self.camera_label()?;
        self.devices
            .get_mut(&label)?
            .as_camera_mut()
            .ok_or(MmError::WrongDeviceType)?
            .set_exposure(exp_ms)
    }

    pub fn get_exposure(&self) -> MmResult<f64> {
        let label = self.camera_label()?;
        Ok(self
            .devices
            .get(&label)?
            .as_camera()
            .ok_or(MmError::WrongDeviceType)?
            .get_exposure())
    }

    pub fn start_sequence_acquisition(&mut self, count: i64, interval_ms: f64) -> MmResult<()> {
        let label = self.camera_label()?;
        let shutter_was_open = if self.auto_shutter && self.shutter_label.is_some() {
            let was_open = self.get_shutter_open()?;
            if !was_open {
                self.set_shutter_open(true)?;
                self.wait_for_shutter()?;
            }
            Some(was_open)
        } else {
            None
        };
        let start_result = self
            .devices
            .get_mut(&label)?
            .as_camera_mut()
            .ok_or(MmError::WrongDeviceType)?
            .set_sequence_image_sink(Some(self.circular_buffer.clone()));
        if let Err(err) = start_result {
            if shutter_was_open == Some(false) {
                let _ = self.set_shutter_open(false);
            }
            return Err(err);
        }
        let start_result = self
            .devices
            .get_mut(&label)?
            .as_camera_mut()
            .ok_or(MmError::WrongDeviceType)?
            .start_sequence_acquisition(count, interval_ms);
        if let Err(err) = start_result {
            if shutter_was_open == Some(false) {
                let _ = self.set_shutter_open(false);
            }
            return Err(err);
        }
        self.sequence_auto_shutter_opened = shutter_was_open == Some(false);
        self.circular_buffer.clear();
        Ok(())
    }

    pub fn stop_sequence_acquisition(&mut self) -> MmResult<()> {
        let label = self.camera_label()?;
        let stop_result = self
            .devices
            .get_mut(&label)?
            .as_camera_mut()
            .ok_or(MmError::WrongDeviceType)?
            .stop_sequence_acquisition();
        let sink_clear_result = self
            .devices
            .get_mut(&label)?
            .as_camera_mut()
            .ok_or(MmError::WrongDeviceType)?
            .set_sequence_image_sink(None);
        if self.sequence_auto_shutter_opened {
            self.sequence_auto_shutter_opened = false;
            let close_result = self
                .set_shutter_open(false)
                .and_then(|_| self.wait_for_shutter());
            stop_result.and(sink_clear_result).and(close_result)
        } else {
            stop_result.and(sink_clear_result)
        }
    }

    pub fn is_sequence_running(&self) -> MmResult<bool> {
        let label = self.camera_label()?;
        Ok(self
            .devices
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
            .ok_or(MmError::CoreInvalidXYStageDevice)
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
        self.shutter_label
            .clone()
            .ok_or(MmError::CoreInvalidShutterDevice)
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

    pub fn set_auto_shutter(&mut self, enabled: bool) {
        self.auto_shutter = enabled;
    }

    pub fn get_auto_shutter(&self) -> bool {
        self.auto_shutter
    }

    // ─── Circular buffer ─────────────────────────────────────────────────────

    pub fn pop_next_image(&self) -> Option<ImageFrame> {
        self.circular_buffer.pop()
    }

    pub fn get_remaining_image_count(&self) -> usize {
        self.circular_buffer.len()
    }

    pub fn get_buffer_overflow_count(&self) -> u64 {
        self.circular_buffer.overflow_count()
    }

    /// Insert a frame directly into the ring buffer (called by adapters during sequence acq.).
    pub fn insert_image(&self, frame: ImageFrame) -> bool {
        self.circular_buffer.push(frame)
    }

    pub fn set_parent_label(&mut self, device_label: &str, parent_label: &str) -> MmResult<()> {
        if device_label == parent_label {
            return Err(MmError::SelfReference);
        }
        self.devices.get_device(device_label)?;
        self.devices.get_device(parent_label)?;
        self.parent_labels
            .insert(device_label.to_string(), parent_label.to_string());
        Ok(())
    }

    pub fn get_parent_label(&self, device_label: &str) -> MmResult<Option<String>> {
        self.devices.get_device(device_label)?;
        Ok(self.parent_labels.get(device_label).cloned())
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
            self.set_property(&s.device_label, &s.property_name, val)?;
        }
        Ok(())
    }

    // ─── Config file I/O ──────────────────────────────────────────────────────

    /// Load a configuration file, creating and initializing all devices.
    pub fn load_system_configuration(&mut self, text: &str) -> MmResult<()> {
        let cfg = ConfigFile::parse(text)?;

        if !cfg.records.is_empty() {
            for record in &cfg.records {
                match record {
                    ConfigRecord::Device(label, module, device) => {
                        self.load_device(label, module, device)?;
                    }
                    ConfigRecord::Property(label, prop, value) => {
                        self.set_property(label, prop, PropertyValue::String(value.clone()))?;
                    }
                    ConfigRecord::Parent(device_label, parent_label) => {
                        self.set_parent_label(device_label, parent_label)?;
                    }
                    ConfigRecord::Label(device_label, position, label) => {
                        self.devices
                            .get_mut(device_label)?
                            .as_state_device_mut()
                            .ok_or(MmError::WrongDeviceType)?
                            .set_position_label(*position, label)?;
                    }
                    ConfigRecord::ConfigGroup(group_name, preset, setting) => {
                        self.config_groups
                            .entry(group_name.clone())
                            .or_default()
                            .add_setting(
                                preset,
                                setting.device_label.clone(),
                                setting.property_name.clone(),
                                setting.value.clone(),
                            );
                    }
                }
            }
        } else {
            for (label, module, device) in &cfg.devices {
                self.load_device(label, module, device)?;
            }
            for (device_label, position, label) in &cfg.labels {
                self.devices
                    .get_mut(device_label)?
                    .as_state_device_mut()
                    .ok_or(MmError::WrongDeviceType)?
                    .set_position_label(*position, label)?;
            }
            for (device_label, parent_label) in &cfg.parents {
                self.set_parent_label(device_label, parent_label)?;
            }
            for (label, prop, value) in &cfg.properties {
                self.set_property(label, prop, PropertyValue::String(value.clone()))?;
            }
            for (group_name, group) in cfg.config_groups {
                self.config_groups.insert(group_name, group);
            }
        }
        if self
            .config_groups
            .get("System")
            .and_then(|group| group.get_preset("Startup"))
            .is_some()
        {
            self.set_config("System", "Startup")?;
        }
        Ok(())
    }

    /// Serialize the current system configuration to a .cfg string.
    pub fn save_system_configuration(&self) -> MmResult<String> {
        let mut devices = Vec::new();
        let mut properties = Vec::new();
        let mut labels = Vec::new();

        for label in self.devices.labels() {
            let handle = self.devices.entry_ref(label)?;
            let dev = handle.device.as_device();
            devices.push((
                label.to_string(),
                handle.module_name.clone(),
                handle.device_name.clone(),
            ));
            for prop_name in dev.property_names() {
                if prop_name == "Label" && handle.device.as_state_device().is_some() {
                    continue;
                }
                if dev.is_property_read_only(&prop_name) {
                    continue;
                }
                if let Ok(val) = dev.get_property(&prop_name) {
                    properties.push((label.to_string(), prop_name, val.to_string()));
                }
            }
            if let Some(state) = handle.device.as_state_device() {
                for pos in 0..state.get_number_of_positions() {
                    if let Ok(position_label) = state.get_position_label(pos) {
                        labels.push((label.to_string(), pos, position_label));
                    }
                }
            }
        }
        for (prop, value) in [
            ("Camera", self.camera_label.clone()),
            ("Shutter", self.shutter_label.clone()),
            ("Focus", self.focus_label.clone()),
            ("XYStage", self.xy_stage_label.clone()),
        ] {
            if let Some(value) = value {
                properties.push(("Core".to_string(), prop.to_string(), value));
            }
        }
        properties.push((
            "Core".to_string(),
            "AutoShutter".to_string(),
            if self.auto_shutter { "1" } else { "0" }.to_string(),
        ));

        let mut parents: Vec<(String, String)> = self
            .parent_labels
            .iter()
            .map(|(child, parent)| (child.clone(), parent.clone()))
            .collect();
        parents.sort();
        Ok(ConfigFile {
            devices,
            properties,
            config_groups: self.config_groups.clone(),
            parents,
            labels,
            records: Vec::new(),
        }
        .to_core_text())
    }

    // ─── Utility ─────────────────────────────────────────────────────────────

    pub fn device_labels(&self) -> Vec<&str> {
        let mut labels = Vec::with_capacity(self.devices.labels().len() + 1);
        labels.push("Core");
        labels.extend(self.devices.labels());
        labels
    }

    pub fn get_device_type(&self, label: &str) -> MmResult<DeviceType> {
        if label == "Core" {
            return Ok(DeviceType::Core);
        }
        Ok(self.devices.get_device(label)?.device_type())
    }

    /// List all registered adapter module names.
    pub fn get_adapter_names(&self) -> Vec<&str> {
        self.registry.module_names()
    }

    /// List available device names for a given adapter module.
    pub fn get_available_devices(&self, module_name: &str) -> MmResult<Vec<String>> {
        let module = self.registry.get_module(module_name)?;
        Ok(module
            .devices()
            .iter()
            .map(|d| d.name.to_string())
            .collect())
    }
}

impl Default for CMMCore {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_core_bool(value: &str) -> MmResult<bool> {
    match value {
        "1" | "Yes" | "yes" | "True" | "true" | "On" | "on" => Ok(true),
        "0" | "No" | "no" | "False" | "false" | "Off" | "off" => Ok(false),
        _ => Err(MmError::InvalidPropertyValue),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_loads_chuoseiki_md5000_devices() {
        let mut core = CMMCore::new();

        core.load_device("xy", "ChuoSeiki", "ChuoSeiki_MD 2-Axis")
            .unwrap();
        core.load_device("z", "ChuoSeiki", "ChuoSeiki_MD 1-Axis")
            .unwrap();

        assert_eq!(core.get_device_type("xy").unwrap(), DeviceType::XYStage);
        assert_eq!(core.get_device_type("z").unwrap(), DeviceType::Stage);
    }

    #[test]
    fn core_registers_motion_adapter_modules() {
        let core = CMMCore::new();

        for module in [
            "ASIStage",
            "ASITiger",
            "ChuoSeiki_QT",
            "Conix",
            "Corvus",
            "HydraLMT200",
            "Marzhauser",
            "MarzhauserLStep",
            "MarzhauserLStepOld",
            "NewportStage",
            "Prior",
            "PriorLegacy",
            "PriorPureFocus",
            "Scientifica",
            "ScientificaMotion8",
            "SutterStage",
            "Tofra",
            "WieneckeSinske",
            "Zaber",
        ] {
            assert!(
                !core.get_available_devices(module).unwrap().is_empty(),
                "{module} should expose at least one device"
            );
        }
    }
}
