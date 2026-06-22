use crate::error::{MmError, MmResult};
use std::collections::HashMap;

/// A single configuration setting: (device_label, property_name) → value
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigSetting {
    pub device_label: String,
    pub property_name: String,
    pub value: String,
}

/// A supported configuration command in source order.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigRecord {
    Device(String, String, String),
    Property(String, String, String),
    Parent(String, String),
    Label(String, u64, String),
    ConfigGroup(String, String, ConfigSetting),
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
    /// State device labels: (device_label, position, label)
    pub labels: Vec<(String, u64, String)>,
    /// Supported config records in original source order.
    pub records: Vec<ConfigRecord>,
}

impl ConfigFile {
    pub fn parse(text: &str) -> MmResult<Self> {
        let mut devices = Vec::new();
        let mut properties = Vec::new();
        let mut config_groups: HashMap<String, ConfigGroup> = HashMap::new();
        let mut parents = Vec::new();
        let mut labels = Vec::new();
        let mut records = Vec::new();

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
                    let parts: Vec<&str> = line.splitn(4, ',').collect();
                    if parts.len() < 4 {
                        return Err(MmError::LocallyDefined(format!(
                            "line {}: Device requires 3 arguments",
                            lineno + 1
                        )));
                    }
                    let record = ConfigRecord::Device(
                        parts[1].trim().to_string(),
                        parts[2].trim().to_string(),
                        parts[3].trim().to_string(),
                    );
                    if let ConfigRecord::Device(label, module, device) = &record {
                        devices.push((label.clone(), module.clone(), device.clone()));
                    }
                    records.push(record);
                }
                "Property" => {
                    let parts: Vec<&str> = line.splitn(4, ',').collect();
                    if parts.len() < 4 {
                        return Err(MmError::LocallyDefined(format!(
                            "line {}: Property requires 3 arguments",
                            lineno + 1
                        )));
                    }
                    let record = ConfigRecord::Property(
                        parts[1].trim().to_string(),
                        parts[2].trim().to_string(),
                        parts[3].trim().to_string(),
                    );
                    if let ConfigRecord::Property(label, prop, value) = &record {
                        properties.push((label.clone(), prop.clone(), value.clone()));
                    }
                    records.push(record);
                }
                "ConfigGroup" => {
                    let parts: Vec<&str> = line.splitn(6, ',').collect();
                    if parts.len() < 6 {
                        return Err(MmError::LocallyDefined(format!(
                            "line {}: ConfigGroup requires 5 arguments",
                            lineno + 1
                        )));
                    }
                    let setting = ConfigSetting {
                        device_label: parts[3].trim().to_string(),
                        property_name: parts[4].trim().to_string(),
                        value: parts[5].trim().to_string(),
                    };
                    let group_name = parts[1].trim().to_string();
                    let preset = parts[2].trim().to_string();
                    let group = config_groups.entry(group_name.clone()).or_default();
                    group.add_setting(
                        &preset,
                        setting.device_label.clone(),
                        setting.property_name.clone(),
                        setting.value.clone(),
                    );
                    records.push(ConfigRecord::ConfigGroup(group_name, preset, setting));
                }
                "Parent" => {
                    let parts: Vec<&str> = line.splitn(3, ',').collect();
                    if parts.len() < 3 {
                        return Err(MmError::LocallyDefined(format!(
                            "line {}: Parent requires 2 arguments",
                            lineno + 1
                        )));
                    }
                    let record = ConfigRecord::Parent(
                        parts[1].trim().to_string(),
                        parts[2].trim().to_string(),
                    );
                    if let ConfigRecord::Parent(child, parent) = &record {
                        parents.push((child.clone(), parent.clone()));
                    }
                    records.push(record);
                }
                "Label" => {
                    let parts: Vec<&str> = line.splitn(4, ',').collect();
                    if parts.len() < 4 {
                        return Err(MmError::LocallyDefined(format!(
                            "line {}: Label requires 3 arguments",
                            lineno + 1
                        )));
                    }
                    let position = parts[2].trim().parse::<u64>().map_err(|_| {
                        MmError::LocallyDefined(format!(
                            "line {}: Label position must be an unsigned integer",
                            lineno + 1
                        ))
                    })?;
                    let record = ConfigRecord::Label(
                        parts[1].trim().to_string(),
                        position,
                        parts[3].trim().to_string(),
                    );
                    if let ConfigRecord::Label(device, position, label) = &record {
                        labels.push((device.clone(), *position, label.clone()));
                    }
                    records.push(record);
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
            labels,
            records,
        })
    }

    fn append_record(out: &mut String, record: &ConfigRecord) {
        match record {
            ConfigRecord::Device(label, module, device) => {
                out.push_str(&format!("Device,{},{},{}\n", label, module, device));
            }
            ConfigRecord::Property(label, prop, value) => {
                out.push_str(&format!("Property,{},{},{}\n", label, prop, value));
            }
            ConfigRecord::Parent(parent_label, hub_label) => {
                out.push_str(&format!("Parent,{},{}\n", parent_label, hub_label));
            }
            ConfigRecord::Label(device_label, position, label) => {
                out.push_str(&format!("Label,{},{},{}\n", device_label, position, label));
            }
            ConfigRecord::ConfigGroup(group_name, preset, setting) => {
                out.push_str(&format!(
                    "ConfigGroup,{},{},{},{},{}\n",
                    group_name, preset, setting.device_label, setting.property_name, setting.value
                ));
            }
        }
    }

    /// Serialize back to the MM .cfg text format.
    pub fn to_text(&self) -> String {
        let mut out = String::new();

        if !self.records.is_empty() {
            for record in &self.records {
                Self::append_record(&mut out, record);
            }
            return out;
        }

        for (label, module, device) in &self.devices {
            out.push_str(&format!("Device,{},{},{}\n", label, module, device));
        }
        for (label, prop, value) in &self.properties {
            out.push_str(&format!("Property,{},{},{}\n", label, prop, value));
        }
        for (parent_label, hub_label) in &self.parents {
            out.push_str(&format!("Parent,{},{}\n", parent_label, hub_label));
        }
        for (device_label, position, label) in &self.labels {
            out.push_str(&format!("Label,{},{},{}\n", device_label, position, label));
        }
        let mut group_names: Vec<&str> = self.config_groups.keys().map(String::as_str).collect();
        group_names.sort();
        for group_name in group_names {
            let group = &self.config_groups[group_name];
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

    /// Serialize in the order expected by MMCore system configuration files.
    pub fn to_core_text(&self) -> String {
        let mut out = String::new();

        out.push_str("Property,Core,Initialize,0\n");
        if !self.records.is_empty() {
            for record in &self.records {
                if matches!(record, ConfigRecord::Property(label, prop, _) if label == "Core" && prop == "Initialize")
                {
                    continue;
                }
                Self::append_record(&mut out, record);
            }
            out.push_str("Property,Core,Initialize,1\n");
            return out;
        }
        for (label, module, device) in &self.devices {
            out.push_str(&format!("Device,{},{},{}\n", label, module, device));
        }
        for (parent_label, hub_label) in &self.parents {
            out.push_str(&format!("Parent,{},{}\n", parent_label, hub_label));
        }
        for (device_label, position, label) in &self.labels {
            out.push_str(&format!("Label,{},{},{}\n", device_label, position, label));
        }
        for (label, prop, value) in &self.properties {
            if label == "Core" && prop == "Initialize" {
                continue;
            }
            out.push_str(&format!("Property,{},{},{}\n", label, prop, value));
        }
        out.push_str("Property,Core,Initialize,1\n");

        let mut group_names: Vec<&str> = self.config_groups.keys().map(String::as_str).collect();
        group_names.sort();
        for group_name in group_names {
            let group = &self.config_groups[group_name];
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
        let text = "Device,Camera,demo,DCam\n\
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
        assert_eq!(cfg2.labels, cfg.labels);
    }

    #[test]
    fn parse_round_trip_preserves_mixed_record_order() {
        let text = "Device,Hub,demo,DHub\n\
                    Device,Wheel,demo,DWheel\n\
                    Parent,Wheel,Hub\n\
                    ConfigGroup,B,Two,Wheel,State,2\n\
                    Label,Wheel,2,Filter, DAPI\n\
                    Property,Core,Initialize,1\n\
                    ConfigGroup,A,One,Wheel,State,1\n\
                    Property,Core,Camera,Camera\n";
        let cfg = ConfigFile::parse(text).unwrap();

        assert_eq!(cfg.to_text(), text);
        assert_eq!(cfg.records.len(), 8);
    }

    #[test]
    fn parse_preserves_commas_in_trailing_values() {
        let text = "Property,Camera,Description,alpha,beta\n\
                    ConfigGroup,Channel,DAPI,Wheel,Label,DAPI,wide\n\
                    Label,Wheel,2,Filter, DAPI\n";
        let cfg = ConfigFile::parse(text).unwrap();

        assert_eq!(
            cfg.properties,
            vec![(
                "Camera".to_string(),
                "Description".to_string(),
                "alpha,beta".to_string()
            )]
        );
        let channel = cfg.config_groups.get("Channel").unwrap();
        let setting = &channel.get_preset("DAPI").unwrap()[0];
        assert_eq!(setting.value, "DAPI,wide");
        assert_eq!(
            cfg.labels,
            vec![("Wheel".to_string(), 2, "Filter, DAPI".to_string())]
        );
    }
}
