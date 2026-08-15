//! ASIFW1000 controller hub.

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, DeviceChildMetadata, DeviceDetectedChild, Hub};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

pub struct AsiFw1000Hub {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    firmware_version: String,
}

impl AsiFw1000Hub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            firmware_version: String::new(),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = RefCell::new(Some(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.borrow_mut().as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    fn define_init_property(
        &mut self,
        name: &str,
        value: PropertyValue,
        read_only: bool,
    ) -> MmResult<()> {
        if let Some(entry) = self.props.entry_mut(name) {
            entry.value = value;
            entry.read_only = read_only;
            Ok(())
        } else {
            self.props.define_property(name, value, read_only)
        }
    }
}

impl Default for AsiFw1000Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for AsiFw1000Hub {
    fn name(&self) -> &str {
        "ASIFWController"
    }

    fn description(&self) -> &str {
        "ASIFW1000 Controller"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }

        let version = self.cmd("VN ")?;
        if version.len() < 3 {
            return Err(MmError::LocallyDefined("No version response".into()));
        }
        self.firmware_version = version
            .strip_prefix("VN ")
            .or_else(|| version.strip_prefix("VN"))
            .unwrap_or(&version)
            .trim()
            .to_string();

        let verbose = self.cmd("VB 6")?;
        if !verbose.ends_with('6') {
            return Err(MmError::SerialInvalidResponse);
        }
        let current_wheel = self.cmd("FW")?;
        let wheel = current_wheel
            .split_whitespace()
            .last()
            .ok_or(MmError::SerialInvalidResponse)?
            .parse::<u8>()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        if wheel > 1 {
            return Err(MmError::SerialInvalidResponse);
        }

        self.define_init_property(
            "Name",
            PropertyValue::String("ASIFWController".into()),
            true,
        )?;
        self.define_init_property(
            "Description",
            PropertyValue::String("ASIFW1000 controller".into()),
            true,
        )?;
        self.define_init_property(
            "Firmware version",
            PropertyValue::String(self.firmware_version.clone()),
            true,
        )?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Firmware version" => Ok(PropertyValue::String(self.firmware_version.clone())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
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
        DeviceType::Hub
    }

    fn busy(&self) -> bool {
        false
    }
}

impl Hub for AsiFw1000Hub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        Ok(vec!["ASIFilterWheel".into(), "ASIShutter".into()])
    }

    fn detect_installed_child_devices(
        &mut self,
        parent_label: &str,
    ) -> MmResult<Vec<DeviceDetectedChild>> {
        Ok(vec![
            DeviceDetectedChild {
                device_name: "ASIFilterWheel".into(),
                label_hint: Some("ASIFilterWheel".into()),
                device_type: DeviceType::State,
                parent_label: parent_label.to_string(),
                metadata: DeviceChildMetadata::None,
            },
            DeviceDetectedChild {
                device_name: "ASIShutter".into(),
                label_hint: Some("ASIShutter".into()),
                device_type: DeviceType::Shutter,
                parent_label: parent_label.to_string(),
                metadata: DeviceChildMetadata::None,
            },
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_creates_upstream_hub_properties() {
        let t = MockTransport::new()
            .expect("VN \r", "VN 2.3")
            .expect("VB 6\r", "VB 6")
            .expect("FW\r", "FW 0");
        let mut hub = AsiFw1000Hub::new().with_transport(Box::new(t));

        hub.initialize().unwrap();

        assert_eq!(
            hub.get_property("Name").unwrap(),
            PropertyValue::String("ASIFWController".into())
        );
        assert_eq!(
            hub.get_property("Description").unwrap(),
            PropertyValue::String("ASIFW1000 controller".into())
        );
        assert_eq!(
            hub.get_property("Firmware version").unwrap(),
            PropertyValue::String("2.3".into())
        );
        assert!(hub.is_property_read_only("Name"));
        assert!(hub.is_property_read_only("Description"));
        assert!(hub.is_property_read_only("Firmware version"));
    }

    #[test]
    fn hub_reports_filter_wheel_and_shutter_children() {
        let mut hub = AsiFw1000Hub::new();

        let children = hub.detect_installed_child_devices("hub").unwrap();

        assert_eq!(children.len(), 2);
        assert_eq!(children[0].device_name, "ASIFilterWheel");
        assert_eq!(children[0].device_type, DeviceType::State);
        assert_eq!(children[0].parent_label, "hub");
        assert_eq!(children[1].device_name, "ASIShutter");
        assert_eq!(children[1].device_type, DeviceType::Shutter);
        assert_eq!(children[1].parent_label, "hub");
    }
}
