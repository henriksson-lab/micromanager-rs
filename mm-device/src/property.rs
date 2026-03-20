use std::collections::HashMap;
use crate::error::{MmError, MmResult};
use crate::types::{PropertyType, PropertyValue};

/// A single property definition with its current value and constraints
#[derive(Debug, Clone)]
pub struct PropertyEntry {
    pub value: PropertyValue,
    pub property_type: PropertyType,
    pub read_only: bool,
    pub pre_init: bool,
    pub allowed_values: Vec<String>,
    pub has_limits: bool,
    pub lower_limit: f64,
    pub upper_limit: f64,
}

impl PropertyEntry {
    pub fn new(value: PropertyValue, read_only: bool) -> Self {
        let property_type = value.property_type();
        Self {
            value,
            property_type,
            read_only,
            pre_init: false,
            allowed_values: Vec::new(),
            has_limits: false,
            lower_limit: 0.0,
            upper_limit: 0.0,
        }
    }
}

/// Helper struct for managing a device's property set.
/// Analogous to the `CDeviceBase<T,U>` property system in C++.
#[derive(Debug, Default)]
pub struct PropertyMap {
    props: HashMap<String, PropertyEntry>,
    /// Insertion-order key list so property_names() is deterministic
    order: Vec<String>,
}

impl PropertyMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Define a new property with a default value.
    pub fn define_property(
        &mut self,
        name: impl Into<String>,
        value: impl Into<PropertyValue>,
        read_only: bool,
    ) -> MmResult<()> {
        let name = name.into();
        if self.props.contains_key(&name) {
            return Err(MmError::DuplicateProperty);
        }
        self.order.push(name.clone());
        self.props.insert(name, PropertyEntry::new(value.into(), read_only));
        Ok(())
    }

    /// Define a property that must be set before initialization.
    pub fn define_pre_init_property(
        &mut self,
        name: impl Into<String>,
        value: impl Into<PropertyValue>,
    ) -> MmResult<()> {
        let name = name.into();
        if self.props.contains_key(&name) {
            return Err(MmError::DuplicateProperty);
        }
        let mut entry = PropertyEntry::new(value.into(), false);
        entry.pre_init = true;
        self.order.push(name.clone());
        self.props.insert(name, entry);
        Ok(())
    }

    /// Restrict a property to a set of allowed string values.
    pub fn set_allowed_values(&mut self, name: &str, values: &[&str]) -> MmResult<()> {
        let entry = self.props.get_mut(name).ok_or_else(|| MmError::UnknownLabel(name.to_string()))?;
        entry.allowed_values = values.iter().map(|s| s.to_string()).collect();
        Ok(())
    }

    /// Set numeric limits for a property.
    pub fn set_property_limits(&mut self, name: &str, lower: f64, upper: f64) -> MmResult<()> {
        let entry = self.props.get_mut(name).ok_or_else(|| MmError::UnknownLabel(name.to_string()))?;
        entry.has_limits = true;
        entry.lower_limit = lower;
        entry.upper_limit = upper;
        Ok(())
    }

    /// Get a property value.
    pub fn get(&self, name: &str) -> MmResult<&PropertyValue> {
        self.props
            .get(name)
            .map(|e| &e.value)
            .ok_or_else(|| MmError::UnknownLabel(name.to_string()))
    }

    /// Set a property value, enforcing read-only and allowed-value constraints.
    pub fn set(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        let entry = self
            .props
            .get_mut(name)
            .ok_or_else(|| MmError::UnknownLabel(name.to_string()))?;

        if entry.read_only {
            return Err(MmError::CanNotSetProperty);
        }

        if !entry.allowed_values.is_empty() {
            let val_str = val.to_string();
            if !entry.allowed_values.iter().any(|v| v == &val_str) {
                return Err(MmError::InvalidPropertyValue);
            }
        }

        entry.value = val;
        Ok(())
    }

    /// Returns property names in definition order.
    pub fn property_names(&self) -> &[String] {
        &self.order
    }

    /// Returns true if the property exists.
    pub fn has_property(&self, name: &str) -> bool {
        self.props.contains_key(name)
    }

    /// Access the full entry for a property.
    pub fn entry(&self, name: &str) -> Option<&PropertyEntry> {
        self.props.get(name)
    }

    pub fn entry_mut(&mut self, name: &str) -> Option<&mut PropertyEntry> {
        self.props.get_mut(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn define_and_get() {
        let mut map = PropertyMap::new();
        map.define_property("Exposure", PropertyValue::Float(10.0), false).unwrap();
        let v = map.get("Exposure").unwrap();
        assert_eq!(*v, PropertyValue::Float(10.0));
    }

    #[test]
    fn set_and_get() {
        let mut map = PropertyMap::new();
        map.define_property("Binning", PropertyValue::Integer(1), false).unwrap();
        map.set("Binning", PropertyValue::Integer(2)).unwrap();
        assert_eq!(*map.get("Binning").unwrap(), PropertyValue::Integer(2));
    }

    #[test]
    fn read_only_rejected() {
        let mut map = PropertyMap::new();
        map.define_property("Width", PropertyValue::Integer(512), true).unwrap();
        assert!(map.set("Width", PropertyValue::Integer(256)).is_err());
    }

    #[test]
    fn allowed_values_enforced() {
        let mut map = PropertyMap::new();
        map.define_property("PixelType", PropertyValue::String("GRAY8".into()), false).unwrap();
        map.set_allowed_values("PixelType", &["GRAY8", "GRAY16"]).unwrap();
        assert!(map.set("PixelType", PropertyValue::String("GRAY16".into())).is_ok());
        assert!(map.set("PixelType", PropertyValue::String("RGB32".into())).is_err());
    }

    #[test]
    fn duplicate_property_rejected() {
        let mut map = PropertyMap::new();
        map.define_property("Gain", PropertyValue::Float(1.0), false).unwrap();
        assert!(map.define_property("Gain", PropertyValue::Float(2.0), false).is_err());
    }
}
