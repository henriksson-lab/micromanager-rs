/// CARVII hub.
///
/// Upstream uses a global helper behind the MM hub object.  This translation
/// keeps the pointer-free visible hub surface and startup/shutdown commands.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{AnyDevice, Device, DeviceDetectedChild, Hub, HubContext, ParentHandle};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::sync::{Arc, Mutex};

pub type SharedCarviiTransport = Arc<Mutex<Box<dyn Transport>>>;

pub struct CarviiHubContext {
    transport: SharedCarviiTransport,
}

impl CarviiHubContext {
    pub fn new(transport: SharedCarviiTransport) -> Self {
        Self { transport }
    }

    pub fn transport(&self) -> SharedCarviiTransport {
        self.transport.clone()
    }
}

impl HubContext for CarviiHubContext {
    fn context_type(&self) -> &'static str {
        "CarviiHubContext"
    }
}

pub struct CarviiHub {
    props: PropertyMap,
    transport: Option<SharedCarviiTransport>,
    initialized: bool,
    port: String,
}

impl CarviiHub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "Name",
                PropertyValue::String(super::DEVICE_NAME_HUB.into()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Description",
                PropertyValue::String("CARVII hub".into()),
                true,
            )
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            port: "Undefined".into(),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(Arc::new(Mutex::new(t)));
        self
    }

    pub fn with_shared_transport(mut self, transport: SharedCarviiTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn shared_transport(&self) -> Option<SharedCarviiTransport> {
        self.transport.clone()
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        let transport = self.transport.as_ref().ok_or(MmError::NotConnected)?;
        let mut transport = transport
            .lock()
            .map_err(|_| MmError::LocallyDefined("CARVII transport lock poisoned".into()))?;
        f(transport.as_mut())
    }

    fn send_cmd(&self, command: &str) -> MmResult<()> {
        let full = format!("{command}\r");
        self.call_transport(|t| {
            t.purge()?;
            t.send(&full)
        })
    }
}

impl Default for CarviiHub {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CarviiHub {
    fn name(&self) -> &str {
        super::DEVICE_NAME_HUB
    }

    fn description(&self) -> &str {
        "CARVII hub"
    }

    fn initialize(&mut self) -> MmResult<()> {
        self.send_cmd("M1")?;
        self.send_cmd("D1N1")?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            self.send_cmd("M0")?;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Port" => Ok(PropertyValue::String(self.port.clone())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" => {
                if self.initialized {
                    return Ok(());
                }
                self.port = val.as_str().to_string();
                self.props
                    .set(name, PropertyValue::String(self.port.clone()))
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
        DeviceType::Hub
    }

    fn busy(&self) -> bool {
        false
    }
}

impl Hub for CarviiHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        Ok(vec![
            super::DEVICE_NAME_SHUTTER.into(),
            super::DEVICE_NAME_EX_FILTER.into(),
            super::DEVICE_NAME_EM_FILTER.into(),
            super::DEVICE_NAME_DICHROIC.into(),
            super::DEVICE_NAME_FRAP_IRIS.into(),
            super::DEVICE_NAME_INTENSITY_IRIS.into(),
            super::DEVICE_NAME_DISK_SLIDER.into(),
            super::DEVICE_NAME_SPIN_MOTOR.into(),
            super::DEVICE_NAME_PRISM_SLIDER.into(),
            super::DEVICE_NAME_TOUCHSCREEN.into(),
        ])
    }

    fn parent_handle(&self, parent_label: &str) -> MmResult<ParentHandle> {
        let transport = self.shared_transport().ok_or(MmError::NotConnected)?;
        Ok(ParentHandle::new(
            parent_label,
            Arc::new(CarviiHubContext::new(transport)),
        ))
    }

    fn create_child(&mut self, child: &DeviceDetectedChild) -> MmResult<AnyDevice> {
        let transport = self.shared_transport().ok_or(MmError::NotConnected)?;
        match child.device_name.as_str() {
            super::DEVICE_NAME_SHUTTER => Ok(AnyDevice::Shutter(Box::new(
                super::CarviiShutter::new().with_shared_transport(transport),
            ))),
            super::DEVICE_NAME_EX_FILTER => Ok(AnyDevice::StateDevice(Box::new(
                super::CarviiStateDevice::new('A', 8).with_shared_transport(transport),
            ))),
            super::DEVICE_NAME_EM_FILTER => Ok(AnyDevice::StateDevice(Box::new(
                super::CarviiStateDevice::new('B', 8).with_shared_transport(transport),
            ))),
            super::DEVICE_NAME_DICHROIC => Ok(AnyDevice::StateDevice(Box::new(
                super::CarviiStateDevice::new('C', 5).with_shared_transport(transport),
            ))),
            super::DEVICE_NAME_FRAP_IRIS => Ok(AnyDevice::Generic(Box::new(
                super::CarviiIris::new(super::CarviiIrisKind::Frap)
                    .with_shared_transport(transport),
            ))),
            super::DEVICE_NAME_INTENSITY_IRIS => Ok(AnyDevice::Generic(Box::new(
                super::CarviiIris::new(super::CarviiIrisKind::Intensity)
                    .with_shared_transport(transport),
            ))),
            super::DEVICE_NAME_DISK_SLIDER => Ok(AnyDevice::StateDevice(Box::new(
                super::CarviiStateDevice::new('D', 2).with_shared_transport(transport),
            ))),
            super::DEVICE_NAME_SPIN_MOTOR => Ok(AnyDevice::StateDevice(Box::new(
                super::CarviiStateDevice::new('N', 2).with_shared_transport(transport),
            ))),
            super::DEVICE_NAME_PRISM_SLIDER => Ok(AnyDevice::StateDevice(Box::new(
                super::CarviiStateDevice::new('P', 2).with_shared_transport(transport),
            ))),
            super::DEVICE_NAME_TOUCHSCREEN => Ok(AnyDevice::StateDevice(Box::new(
                super::CarviiStateDevice::new('M', 2).with_shared_transport(transport),
            ))),
            _ => Err(MmError::NotSupported),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::DeviceChildMetadata;
    use crate::transport::Transport;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    struct RecordingTransport {
        sent: Arc<Mutex<Vec<String>>>,
        responses: VecDeque<String>,
    }

    impl RecordingTransport {
        fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
            Self::new_with_responses(&[])
        }

        fn new_with_responses(responses: &[&str]) -> (Self, Arc<Mutex<Vec<String>>>) {
            let sent = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    sent: Arc::clone(&sent),
                    responses: responses.iter().map(|r| r.to_string()).collect(),
                },
                sent,
            )
        }
    }

    impl Transport for RecordingTransport {
        fn send(&mut self, cmd: &str) -> MmResult<()> {
            self.sent.lock().unwrap().push(cmd.to_string());
            Ok(())
        }

        fn receive_line(&mut self) -> MmResult<String> {
            self.responses.pop_front().ok_or(MmError::SerialTimeout)
        }

        fn purge(&mut self) -> MmResult<()> {
            Ok(())
        }
    }

    #[test]
    fn startup_and_shutdown_match_upstream_helper_commands() {
        let (transport, sent) = RecordingTransport::new();
        let mut hub = CarviiHub::new().with_transport(Box::new(transport));

        hub.initialize().unwrap();
        hub.shutdown().unwrap();

        assert_eq!(
            *sent.lock().unwrap(),
            vec!["M1\r".to_string(), "D1N1\r".to_string(), "M0\r".to_string()]
        );
    }

    #[test]
    fn detects_all_upstream_children() {
        let mut hub = CarviiHub::new();
        let children = hub.detect_installed_devices().unwrap();
        assert!(children.contains(&super::super::DEVICE_NAME_FRAP_IRIS.to_string()));
        assert!(children.contains(&super::super::DEVICE_NAME_INTENSITY_IRIS.to_string()));
    }

    #[test]
    fn hub_created_children_share_serial_route() {
        let (transport, sent) = RecordingTransport::new_with_responses(&["rS0", "S1"]);
        let mut hub = CarviiHub::new().with_transport(Box::new(transport));

        hub.initialize().unwrap();

        let shutter_child = DeviceDetectedChild {
            device_name: super::super::DEVICE_NAME_SHUTTER.to_string(),
            label_hint: None,
            device_type: DeviceType::Shutter,
            parent_label: "hub".to_string(),
            metadata: DeviceChildMetadata::None,
        };
        let mut shutter = match hub.create_child(&shutter_child).unwrap() {
            AnyDevice::Shutter(shutter) => shutter,
            _ => panic!("expected shutter child"),
        };
        shutter.initialize().unwrap();
        shutter.set_open(true).unwrap();

        let disk_child = DeviceDetectedChild {
            device_name: super::super::DEVICE_NAME_DISK_SLIDER.to_string(),
            label_hint: None,
            device_type: DeviceType::State,
            parent_label: "hub".to_string(),
            metadata: DeviceChildMetadata::None,
        };
        let mut disk = match hub.create_child(&disk_child).unwrap() {
            AnyDevice::StateDevice(device) => device,
            _ => panic!("expected state child"),
        };
        disk.initialize().unwrap();
        disk.set_position(0).unwrap();

        assert_eq!(
            *sent.lock().unwrap(),
            vec![
                "M1\r".to_string(),
                "D1N1\r".to_string(),
                "rS\r".to_string(),
                "S1\r".to_string(),
                "D0\r".to_string(),
            ]
        );
    }
}
