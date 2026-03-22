use std::collections::HashMap;
use crate::error::{MmError, MmResult};
use crate::traits::{AnyDevice, Device};
use crate::types::DeviceType;

/// Wraps an `AnyDevice` along with the module/device names used to load it.
pub struct DeviceHandle {
    pub device: AnyDevice,
    pub module_name: String,
    pub device_name: String,
}

/// Stores all loaded devices keyed by label.
#[derive(Default)]
pub struct DeviceManager {
    devices: HashMap<String, DeviceHandle>,
    /// Maintains insertion order for deterministic save_config output.
    order: Vec<String>,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a device under the given label.
    pub fn add_device(
        &mut self,
        label: impl Into<String>,
        module_name: impl Into<String>,
        device_name: impl Into<String>,
        device: AnyDevice,
    ) -> MmResult<()> {
        let label = label.into();
        if self.devices.contains_key(&label) {
            return Err(MmError::DuplicateLabel);
        }
        self.order.push(label.clone());
        self.devices.insert(
            label,
            DeviceHandle {
                device,
                module_name: module_name.into(),
                device_name: device_name.into(),
            },
        );
        Ok(())
    }

    /// Remove a device, returning its handle.
    pub fn remove_device(&mut self, label: &str) -> MmResult<DeviceHandle> {
        self.order.retain(|l| l != label);
        self.devices
            .remove(label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))
    }

    /// Get a shared reference to the `AnyDevice`.
    pub fn get(&self, label: &str) -> MmResult<&AnyDevice> {
        self.devices
            .get(label)
            .map(|h| &h.device)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))
    }

    /// Get a mutable reference to the `AnyDevice`.
    pub fn get_mut(&mut self, label: &str) -> MmResult<&mut AnyDevice> {
        self.devices
            .get_mut(label)
            .map(|h| &mut h.device)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))
    }

    /// Get a reference to the full `DeviceHandle`.
    pub fn entry_ref(&self, label: &str) -> MmResult<&DeviceHandle> {
        self.devices
            .get(label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))
    }

    /// Convenience: get the inner `&dyn Device` for property/general operations.
    pub fn get_device(&self, label: &str) -> MmResult<&dyn Device> {
        Ok(self.get(label)?.as_device())
    }

    pub fn get_device_mut(&mut self, label: &str) -> MmResult<&mut dyn Device> {
        Ok(self.get_mut(label)?.as_device_mut())
    }

    /// Return all labels of devices with the given type, in insertion order.
    pub fn labels_of_type(&self, device_type: DeviceType) -> Vec<&str> {
        self.order
            .iter()
            .filter(|label| {
                self.devices
                    .get(*label)
                    .map(|h| h.device.as_device().device_type() == device_type)
                    .unwrap_or(false)
            })
            .map(String::as_str)
            .collect()
    }

    /// Return all labels in insertion order.
    pub fn labels(&self) -> Vec<&str> {
        self.order.iter().map(String::as_str).collect()
    }

    pub fn contains(&self, label: &str) -> bool {
        self.devices.contains_key(label)
    }
}
