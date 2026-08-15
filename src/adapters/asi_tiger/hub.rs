//! ASI Tiger controller hub.
//!
//! The Tiger controller is an addressed multi-device controller.  Existing
//! translated XY/Z stage devices still work standalone; this hub path lets
//! MiniCore create children sharing one serial controller resource.

use super::{AsiTigerXYStage, AsiTigerZStage, SharedTigerTransport};
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{
    AnyDevice, Device, DeviceChildMetadata, DeviceDetectedChild, Hub, HubContext, ParentHandle,
};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub struct AsiTigerHubContext {
    transport: SharedTigerTransport,
}

impl AsiTigerHubContext {
    pub fn new(transport: SharedTigerTransport) -> Self {
        Self { transport }
    }

    pub fn transport(&self) -> SharedTigerTransport {
        self.transport.clone()
    }
}

impl HubContext for AsiTigerHubContext {
    fn context_type(&self) -> &'static str {
        "ASITiger"
    }
}

pub struct AsiTigerHub {
    props: PropertyMap,
    transport: Option<SharedTigerTransport>,
    initialized: bool,
    firmware_version: String,
}

impl AsiTigerHub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_property(
                "FirmwareVersion",
                PropertyValue::String(String::new()),
                true,
            )
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            firmware_version: String::new(),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(Arc::new(Mutex::new(t)));
        self
    }

    pub fn with_shared_transport(mut self, transport: SharedTigerTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn shared_transport(&self) -> Option<SharedTigerTransport> {
        self.transport.clone()
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_ref() {
            Some(t) => {
                let mut guard = t.lock().map_err(|_| {
                    MmError::LocallyDefined("ASI Tiger transport lock poisoned".into())
                })?;
                f(guard.as_mut())
            }
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        self.call_transport(|t| {
            t.purge()?;
            let response = t.send_recv(&full)?;
            Ok(response.trim().to_string())
        })
    }

    fn cmd_ok(&self, command: &str) -> MmResult<String> {
        let response = self.cmd(command)?;
        if response.starts_with(":N") {
            Err(MmError::LocallyDefined(format!(
                "ASI Tiger error: {}",
                response
            )))
        } else if !response.starts_with(":A") {
            Err(MmError::SerialInvalidResponse)
        } else {
            Ok(response)
        }
    }
}

impl Default for AsiTigerHub {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for AsiTigerHub {
    fn name(&self) -> &str {
        "TigerCommHub"
    }

    fn description(&self) -> &str {
        "ASI TigerComm Hub (TG-1000)"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let version = self.cmd_ok("0 V")?;
        self.firmware_version = version
            .trim_start_matches(":A")
            .trim()
            .trim_start_matches('v')
            .to_string();
        if let Some(entry) = self.props.entry_mut("FirmwareVersion") {
            entry.value = PropertyValue::String(self.firmware_version.clone());
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "FirmwareVersion" => Ok(PropertyValue::String(self.firmware_version.clone())),
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

impl Hub for AsiTigerHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        Ok(vec!["XYStage".into(), "ZStage".into()])
    }

    fn detect_installed_child_devices(
        &mut self,
        parent_label: &str,
    ) -> MmResult<Vec<DeviceDetectedChild>> {
        let xy_metadata = [
            ("axes".to_string(), "X,Y".to_string()),
            ("controller".to_string(), "ASI Tiger".to_string()),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>();
        Ok(vec![
            DeviceDetectedChild {
                device_name: "XYStage".into(),
                label_hint: Some("XY".into()),
                device_type: DeviceType::XYStage,
                parent_label: parent_label.to_string(),
                metadata: DeviceChildMetadata::KeyValues(xy_metadata),
            },
            DeviceDetectedChild {
                device_name: "ZStage".into(),
                label_hint: Some("Z".into()),
                device_type: DeviceType::Stage,
                parent_label: parent_label.to_string(),
                metadata: DeviceChildMetadata::Axis {
                    card: None,
                    axis: "Z".into(),
                },
            },
        ])
    }

    fn parent_handle(&self, parent_label: &str) -> MmResult<ParentHandle> {
        let transport = self.shared_transport().ok_or(MmError::NotConnected)?;
        Ok(ParentHandle::new(
            parent_label,
            Arc::new(AsiTigerHubContext::new(transport)),
        ))
    }

    fn create_child(&mut self, child: &DeviceDetectedChild) -> MmResult<AnyDevice> {
        let transport = self.shared_transport().ok_or(MmError::NotConnected)?;
        match child.device_name.as_str() {
            "XYStage" | "AsiTigerXYStage" => Ok(AnyDevice::XYStage(Box::new(
                AsiTigerXYStage::new().with_shared_transport(transport),
            ))),
            "ZStage" | "AsiTigerZStage" => Ok(AnyDevice::Stage(Box::new(
                AsiTigerZStage::new().with_shared_transport(transport),
            ))),
            _ => Err(MmError::DeviceNotFound(child.device_name.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minicore::{DependencyRole, DeviceDependency, MiniCore, MiniOrchestrator};
    use crate::transport::MockTransport;

    #[test]
    fn hub_reports_structured_tiger_children() {
        let t = MockTransport::new().expect("0 V\r", ":A v3.01");
        let mut hub = AsiTigerHub::new().with_transport(Box::new(t));

        hub.initialize().unwrap();
        let children = hub.detect_installed_child_devices("hub").unwrap();

        assert_eq!(children.len(), 2);
        assert_eq!(children[0].device_name, "XYStage");
        assert_eq!(children[0].device_type, DeviceType::XYStage);
        assert_eq!(children[0].parent_label, "hub");
        assert_eq!(children[1].device_name, "ZStage");
        assert_eq!(children[1].device_type, DeviceType::Stage);
        assert!(matches!(
            &children[1].metadata,
            DeviceChildMetadata::Axis { axis, .. } if axis == "Z"
        ));
    }

    #[test]
    fn minicore_can_create_and_drive_tiger_z_child_from_hub_descriptor() {
        let t = MockTransport::new()
            .expect("0 V\r", ":A v3.01")
            .expect("UM Z?\r", ":A Z=10000")
            .expect("VB Z=1\r", ":A")
            .expect("W Z\r", ":A Z=0")
            .expect("M Z=500\r", ":A")
            .expect("W Z\r", ":A Z=500");
        let hub = AsiTigerHub::new().with_transport(Box::new(t));
        let mut core = MiniCore::new();
        core.add_device(
            "hub",
            "ASITiger",
            "TigerCommHub",
            AnyDevice::Hub(Box::new(hub)),
        )
        .unwrap();

        core.initialize_device("hub").unwrap();
        let z_child = core
            .detect_installed_devices("hub")
            .unwrap()
            .into_iter()
            .find(|child| child.device_name == "ZStage")
            .unwrap();
        core.create_detected_child("z", "ASITiger", &z_child)
            .unwrap();

        assert_eq!(
            core.dependencies("z").unwrap(),
            &[DeviceDependency {
                role: DependencyRole::ParentHub,
                label: "hub".into(),
                required: true,
            }]
        );
        core.initialize_device("z").unwrap();
        core.set_stage_position_um("z", 50.0).unwrap();

        assert!((core.get_stage_position_um("z").unwrap() - 50.0).abs() < 0.01);
    }

    #[test]
    fn mini_orchestrator_can_create_and_drive_addressed_tiger_children() {
        let t = MockTransport::new()
            .expect("0 V\r", ":A v3.01")
            .expect("0 V\r", ":A v3.01")
            .expect("UM X?\r", ":A X=10000")
            .expect("UM Y?\r", ":A Y=10000")
            .expect("VB Z=1\r", ":A")
            .expect("W X\r", ":A X=0")
            .expect("W Y\r", ":A Y=0")
            .expect("UM Z?\r", ":A Z=10000")
            .expect("VB Z=1\r", ":A")
            .expect("W Z\r", ":A Z=0")
            .expect("M X=120 Y=340\r", ":A")
            .expect("W X\r", ":A X=120")
            .expect("W Y\r", ":A Y=340")
            .expect("M Z=500\r", ":A")
            .expect("W Z\r", ":A Z=500");
        let hub = AsiTigerHub::new().with_transport(Box::new(t));
        let mut core = MiniCore::new();
        core.add_device(
            "hub",
            "ASITiger",
            "TigerCommHub",
            AnyDevice::Hub(Box::new(hub)),
        )
        .unwrap();
        core.initialize_device("hub").unwrap();

        let mut orchestrator = MiniOrchestrator::new(&mut core);
        assert_eq!(
            orchestrator
                .detect_and_create_children("hub", "ASITiger")
                .unwrap(),
            vec!["XY".to_string(), "Z".to_string()]
        );
        assert_eq!(
            orchestrator.dependencies("XY").unwrap(),
            &[DeviceDependency {
                role: DependencyRole::ParentHub,
                label: "hub".into(),
                required: true,
            }]
        );
        assert_eq!(
            orchestrator.dependencies("Z").unwrap(),
            &[DeviceDependency {
                role: DependencyRole::ParentHub,
                label: "hub".into(),
                required: true,
            }]
        );

        orchestrator.initialize_device("XY").unwrap();
        orchestrator.initialize_device("Z").unwrap();
        orchestrator.set_xy_position_um("XY", 12.0, 34.0).unwrap();
        assert_eq!(orchestrator.get_xy_position_um("XY").unwrap(), (12.0, 34.0));
        orchestrator.set_stage_position_um("Z", 50.0).unwrap();
        assert!((orchestrator.get_stage_position_um("Z").unwrap() - 50.0).abs() < 0.01);
    }
}
