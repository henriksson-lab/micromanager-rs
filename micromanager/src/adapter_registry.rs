use std::collections::HashMap;
use crate::traits::{AdapterModule, AnyDevice};
use crate::error::{MmError, MmResult};

/// Registry of all known adapter modules.
/// Adapters are registered explicitly via `register()`.
pub struct AdapterRegistry {
    modules: HashMap<String, Box<dyn AdapterModule>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
        }
    }

    /// Register an adapter module under its declared module name.
    pub fn register(&mut self, module: Box<dyn AdapterModule>) {
        self.modules.insert(module.module_name().to_string(), module);
    }

    /// Instantiate a device by module name and device name.
    pub fn create_device(&self, module_name: &str, device_name: &str) -> MmResult<AnyDevice> {
        let module = self
            .modules
            .get(module_name)
            .ok_or(MmError::NativeModuleFailed)?;
        module
            .create_device(device_name)
            .ok_or_else(|| MmError::UnknownLabel(device_name.to_string()))
    }

    pub fn module_names(&self) -> Vec<&str> {
        self.modules.keys().map(String::as_str).collect()
    }

    pub fn has_module(&self, name: &str) -> bool {
        self.modules.contains_key(name)
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}
