/// TriggerScope CAM trigger output.
///
/// Upstream implements CAM1/CAM2 as state devices with a single writable
/// integer property named `CAM`, not as Micro-Manager camera devices.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, DeviceChildMetadata, ParentHandle, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

use super::hub::SharedTriggerScopeTransport;

pub struct TriggerScopeCAM {
    props: PropertyMap,
    transport: Option<SharedTriggerScopeTransport>,
    initialized: bool,
    channel: u8,
    name: String,
    cam: u64,
    parent_label: Option<String>,
}

impl TriggerScopeCAM {
    pub fn new(channel: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Channel", PropertyValue::Integer(channel as i64), true)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            channel,
            name: format!("TriggerScope-CAM{}", channel),
            cam: 0,
            parent_label: None,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(std::sync::Arc::new(std::sync::Mutex::new(t)));
        self
    }

    pub fn with_shared_transport(mut self, transport: SharedTriggerScopeTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_mut() {
            Some(t) => {
                let mut guard = t.lock().map_err(|_| {
                    MmError::LocallyDefined("TriggerScope transport lock poisoned".into())
                })?;
                f(guard.as_mut())
            }
            None => Err(MmError::NotConnected),
        }
    }

    fn send_recv(&mut self, cmd: &str) -> MmResult<String> {
        self.call_transport(|t| {
            t.purge()?;
            match t.send_recv(cmd) {
                Ok(resp) if !resp.trim().is_empty() => Ok(resp.trim().to_string()),
                _ => {
                    t.purge()?;
                    Ok(t.send_recv(cmd)?.trim().to_string())
                }
            }
        })
    }

    fn ensure_runtime_properties(&mut self) -> MmResult<()> {
        if !self.props.has_property("CAM") {
            self.props
                .define_property("CAM", PropertyValue::Integer(0), false)?;
            self.props.set_property_limits("CAM", 0.0, 1.0)?;
        }
        Ok(())
    }

    fn write_to_port(&mut self, value: u64) -> MmResult<()> {
        let value = value.min(1);
        let cmd = format!("CAM{},{}\n", self.channel, value);
        let _ = self.send_recv(&cmd)?;
        Ok(())
    }
}

impl Device for TriggerScopeCAM {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "ARC TriggerScope CAM trigger output"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.initialized {
            return Ok(());
        }
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.ensure_runtime_properties()?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "CAM" if self.props.has_property("CAM") => Ok(PropertyValue::Integer(self.cam as i64)),
            "State" if self.props.has_property("CAM") => {
                Ok(PropertyValue::Integer(self.cam as i64))
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "CAM" if self.props.has_property("CAM") => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_position(pos as u64)
            }
            "State" if self.props.has_property("CAM") => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_position(pos as u64)
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> {
        self.props.property_names().to_vec()
    }

    fn has_property(&self, name: &str) -> bool {
        self.props.has_property(name) || (name == "State" && self.props.has_property("CAM"))
    }

    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::State
    }

    fn busy(&self) -> bool {
        false
    }

    fn bind_parent(&mut self, parent: ParentHandle, metadata: DeviceChildMetadata) -> MmResult<()> {
        match metadata {
            DeviceChildMetadata::Channel(channel) if channel == self.channel as u32 => {
                self.parent_label = Some(parent.label().to_string());
                Ok(())
            }
            _ => Err(MmError::InvalidPropertyValue),
        }
    }

    fn parent_label(&self) -> Option<&str> {
        self.parent_label.as_deref()
    }
}

impl StateDevice for TriggerScopeCAM {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos > 1 {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized {
            self.write_to_port(pos)?;
        }
        self.cam = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        Ok(self.cam)
    }

    fn get_number_of_positions(&self) -> u64 {
        2
    }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        match pos {
            0 => Ok("0".into()),
            1 => Ok("1".into()),
            _ => Err(MmError::UnknownPosition),
        }
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = label
            .parse::<u64>()
            .map_err(|_| MmError::UnknownLabel(label.to_string()))?;
        self.set_position(pos)
    }

    fn set_position_label(&mut self, _pos: u64, _label: &str) -> MmResult<()> {
        Err(MmError::NotSupported)
    }

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.set_position(if open { 1 } else { 0 })
    }

    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(self.cam != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn cam_initializes_state_property_and_writes_channel() {
        let t = MockTransport::new().expect("CAM2,1\n", "CAM2,1");
        let mut cam = TriggerScopeCAM::new(2).with_transport(Box::new(t));

        assert!(!cam.has_property("CAM"));
        cam.initialize().unwrap();
        assert!(cam.has_property("CAM"));
        cam.set_position(1).unwrap();
        assert_eq!(cam.get_position().unwrap(), 1);
    }
}
