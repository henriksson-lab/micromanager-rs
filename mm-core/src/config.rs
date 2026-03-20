use std::collections::HashMap;
use mm_device::error::{MmError, MmResult};

/// A single configuration setting: (device_label, property_name) → value
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigSetting {
    pub device_label: String,
    pub property_name: String,
    pub value: String,
}

/// A named preset containing a list of device/property/value triplets.
#[derive(Debug, Clone, Default)]
pub struct ConfigGroup {
    /// preset_name → list of settings
    presets: HashMap<String, Vec<ConfigSetting>>,
}

impl ConfigGroup {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn define_preset(&mut self, preset: impl Into<String>) {
        self.presets.entry(preset.into()).or_default();
    }

    pub fn add_setting(
        &mut self,
        preset: &str,
        device_label: impl Into<String>,
        property_name: impl Into<String>,
        value: impl Into<String>,
    ) {
        let settings = self.presets.entry(preset.to_string()).or_default();
        settings.push(ConfigSetting {
            device_label: device_label.into(),
            property_name: property_name.into(),
            value: value.into(),
        });
    }

    pub fn get_preset(&self, preset: &str) -> Option<&[ConfigSetting]> {
        self.presets.get(preset).map(Vec::as_slice)
    }

    pub fn preset_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.presets.keys().map(String::as_str).collect();
        names.sort();
        names
    }
}

/// Represents a parsed MicroManager .cfg file.
pub struct ConfigFile {
    /// Devices: (label, module_name, device_name)
    pub devices: Vec<(String, String, String)>,
    /// Property settings to apply after loading: (device_label, prop_name, value)
    pub properties: Vec<(String, String, String)>,
    /// Config groups: group_name → ConfigGroup
    pub config_groups: HashMap<String, ConfigGroup>,
    /// Parent hub assignments: (peripheral_label, hub_label)
    pub parents: Vec<(String, String)>,
}

impl ConfigFile {
    pub fn parse(text: &str) -> MmResult<Self> {
        let mut devices = Vec::new();
        let mut properties = Vec::new();
        let mut config_groups: HashMap<String, ConfigGroup> = HashMap::new();
        let mut parents = Vec::new();

        for (lineno, raw) in text.lines().enumerate() {
            let line = raw.trim();
            // Skip comments and empty lines
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let parts: Vec<&str> = line.split(',').collect();
            if parts.is_empty() {
                continue;
            }

            match parts[0] {
                "Device" => {
                    if parts.len() < 4 {
                        return Err(MmError::LocallyDefined(format!(
                            "line {}: Device requires 3 arguments",
                            lineno + 1
                        )));
                    }
                    devices.push((
                        parts[1].trim().to_string(),
                        parts[2].trim().to_string(),
                        parts[3].trim().to_string(),
                    ));
                }
                "Property" => {
                    if parts.len() < 4 {
                        return Err(MmError::LocallyDefined(format!(
                            "line {}: Property requires 3 arguments",
                            lineno + 1
                        )));
                    }
                    properties.push((
                        parts[1].trim().to_string(),
                        parts[2].trim().to_string(),
                        parts[3].trim().to_string(),
                    ));
                }
                "ConfigGroup" => {
                    if parts.len() < 6 {
                        return Err(MmError::LocallyDefined(format!(
                            "line {}: ConfigGroup requires 5 arguments",
                            lineno + 1
                        )));
                    }
                    let group = config_groups.entry(parts[1].trim().to_string()).or_default();
                    group.add_setting(
                        parts[2].trim(),
                        parts[3].trim(),
                        parts[4].trim(),
                        parts[5].trim(),
                    );
                }
                "Parent" => {
                    if parts.len() < 3 {
                        return Err(MmError::LocallyDefined(format!(
                            "line {}: Parent requires 2 arguments",
                            lineno + 1
                        )));
                    }
                    parents.push((parts[1].trim().to_string(), parts[2].trim().to_string()));
                }
                _ => {
                    // Unknown command — silently skip for forward compatibility
                }
            }
        }

        Ok(ConfigFile {
            devices,
            properties,
            config_groups,
            parents,
        })
    }

    /// Serialize back to the MM .cfg text format.
    pub fn to_text(&self) -> String {
        let mut out = String::new();

        for (label, module, device) in &self.devices {
            out.push_str(&format!("Device,{},{},{}\n", label, module, device));
        }
        for (label, prop, value) in &self.properties {
            out.push_str(&format!("Property,{},{},{}\n", label, prop, value));
        }
        for (parent_label, hub_label) in &self.parents {
            out.push_str(&format!("Parent,{},{}\n", parent_label, hub_label));
        }
        for (group_name, group) in &self.config_groups {
            for preset in group.preset_names() {
                if let Some(settings) = group.get_preset(preset) {
                    for s in settings {
                        out.push_str(&format!(
                            "ConfigGroup,{},{},{},{},{}\n",
                            group_name, preset, s.device_label, s.property_name, s.value
                        ));
                    }
                }
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_round_trip() {
        let text = "Device,Camera,demo,DCamera\n\
                    Property,Camera,Exposure,10\n\
                    ConfigGroup,Channel,DAPI,Camera,Binning,1\n";
        let cfg = ConfigFile::parse(text).unwrap();
        assert_eq!(cfg.devices.len(), 1);
        assert_eq!(cfg.properties.len(), 1);
        assert!(cfg.config_groups.contains_key("Channel"));
        let serialized = cfg.to_text();
        let cfg2 = ConfigFile::parse(&serialized).unwrap();
        assert_eq!(cfg2.devices, cfg.devices);
        assert_eq!(cfg2.properties, cfg.properties);
    }
}
