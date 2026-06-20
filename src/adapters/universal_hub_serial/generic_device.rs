//! UniversalGeneric - described generic device for the Universal Serial Hub.
use super::hub::{
    parse_controller_reply, DeviceDescription, PropertyDescription, SharedTransport, UniversalHub,
    CTR_BUSY, CTR_OK, TIMEOUT,
};
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub struct UniversalGeneric {
    name: String,
    description: String,
    props: PropertyMap,
    described_properties: HashMap<String, PropertyDescription>,
    action_by_command: HashMap<String, String>,
    transport: Option<SharedTransport>,
    initialized: bool,
    busy: bool,
    timeout: Duration,
    last_command: Option<Instant>,
}

impl UniversalGeneric {
    pub fn new(name: &str) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            name: name.to_string(),
            description: "Universal Serial Hub generic device".into(),
            props,
            described_properties: HashMap::new(),
            action_by_command: HashMap::new(),
            transport: None,
            initialized: false,
            busy: false,
            timeout: Duration::from_secs(1),
            last_command: None,
        }
    }

    pub fn from_description(description: DeviceDescription) -> MmResult<Self> {
        let mut device = Self::new(&description.name);
        device.apply_description(description)?;
        Ok(device)
    }

    pub fn with_description(mut self, description: DeviceDescription) -> MmResult<Self> {
        self.apply_description(description)?;
        Ok(self)
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(std::sync::Arc::new(std::sync::Mutex::new(t)));
        self
    }

    pub fn with_shared_transport(mut self, transport: SharedTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn last_command(&self) -> Option<Instant> {
        self.last_command
    }

    pub fn poll_update_once(&mut self) -> MmResult<Option<super::hub::ControllerReply>> {
        match self.call_transport(|t| t.receive_line()) {
            Ok(line) => {
                let reply = parse_controller_reply(&line)?;
                if reply.device_name != self.name {
                    return Err(MmError::DeviceNotFound(reply.device_name));
                }
                if reply.error == CTR_BUSY {
                    self.busy = true;
                    self.last_command = Some(Instant::now());
                } else {
                    self.busy = false;
                }
                if reply.error != CTR_OK && reply.error != CTR_BUSY {
                    return Err(MmError::LocallyDefined(format!(
                        "controller error {} for {}",
                        reply.error, self.name
                    )));
                }
                if reply.command == TIMEOUT {
                    let value = reply
                        .values
                        .first()
                        .ok_or(MmError::SerialInvalidResponse)?
                        .parse::<f64>()
                        .map_err(|_| MmError::SerialInvalidResponse)?;
                    self.timeout = Duration::from_secs_f64(value.max(0.0));
                    self.last_command = Some(Instant::now());
                    return Ok(Some(reply));
                }
                if let Some(value) = reply.values.first() {
                    self.apply_controller_value(&reply.command, value)?;
                }
                Ok(Some(reply))
            }
            Err(MmError::SerialTimeout) => {
                self.update_timeout_state();
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    fn apply_description(&mut self, description: DeviceDescription) -> MmResult<()> {
        if !description.valid {
            return Err(MmError::SerialInvalidResponse);
        }
        if description.device_type != "Generic" {
            return Err(MmError::WrongDeviceType);
        }
        self.name = description.name;
        self.description = description.description;
        self.timeout = description.timeout;

        for prop in description.properties {
            if prop.pre_init {
                continue;
            }
            self.define_described_property(prop)?;
        }
        Ok(())
    }

    fn define_described_property(&mut self, prop: PropertyDescription) -> MmResult<()> {
        self.props
            .define_property(&prop.name, prop.value.clone(), prop.read_only)?;
        if !prop.allowed_values.is_empty() {
            let allowed: Vec<&str> = prop.allowed_values.iter().map(String::as_str).collect();
            self.props.set_allowed_values(&prop.name, &allowed)?;
        }
        if let Some((lower, upper)) = prop.limits {
            self.props.set_property_limits(&prop.name, lower, upper)?;
        }
        if let Some(command) = &prop.command {
            self.action_by_command
                .insert(command.clone(), prop.name.clone());
        }
        self.described_properties.insert(prop.name.clone(), prop);
        Ok(())
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_mut() {
            Some(t) => {
                let mut guard = t.lock().map_err(|_| {
                    MmError::LocallyDefined("Universal transport lock poisoned".into())
                })?;
                f(guard.as_mut())
            }
            None => Err(MmError::NotConnected),
        }
    }

    fn set_described_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        let prop = self
            .described_properties
            .get(name)
            .cloned()
            .ok_or_else(|| MmError::UnknownLabel(name.to_string()))?;

        if !prop.action {
            return self.props.set(name, val);
        }
        if prop.read_only {
            return Ok(());
        }

        let command = prop.command.as_deref().ok_or(MmError::UnsupportedCommand)?;
        let value = coerce_property_value(&prop.value, val)?;
        validate_property_value(&prop, &value)?;
        let cmd = UniversalHub::make_output_command(&self.name, command, &[value.to_string()]);
        self.call_transport(|t| t.send(&cmd))?;

        if prop.pre_init {
            let _ = self.call_transport(|t| t.receive_line())?;
        } else {
            self.last_command = Some(Instant::now());
            self.busy = true;
        }
        self.props.set(name, value)
    }

    fn apply_controller_value(&mut self, command: &str, value: &str) -> MmResult<()> {
        let Some(name) = self.action_by_command.get(command).cloned() else {
            return Err(MmError::UnsupportedCommand);
        };
        let prop = self
            .described_properties
            .get(&name)
            .ok_or_else(|| MmError::UnknownLabel(name.clone()))?;
        let value = coerce_property_value(&prop.value, PropertyValue::String(value.to_string()))?;
        validate_property_value(prop, &value)?;
        if let Some(entry) = self.props.entry_mut(&name) {
            entry.value = value;
        }
        Ok(())
    }

    fn update_timeout_state(&mut self) {
        if self.busy
            && self
                .last_command
                .map(|t| t.elapsed() > self.timeout)
                .unwrap_or(false)
        {
            self.busy = false;
        }
    }
}

impl Device for UniversalGeneric {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn initialize(&mut self) -> MmResult<()> {
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        self.busy = false;
        self.last_command = None;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if self.described_properties.contains_key(name) {
            self.set_described_property(name, val)
        } else {
            self.props.set(name, val)
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
        DeviceType::Generic
    }

    fn busy(&self) -> bool {
        self.busy
            && self
                .last_command
                .map(|t| t.elapsed() <= self.timeout)
                .unwrap_or(true)
    }
}

impl Generic for UniversalGeneric {}

fn coerce_property_value(template: &PropertyValue, val: PropertyValue) -> MmResult<PropertyValue> {
    match template {
        PropertyValue::String(_) => Ok(PropertyValue::String(val.to_string())),
        PropertyValue::Integer(_) => val
            .as_i64()
            .map(PropertyValue::Integer)
            .ok_or(MmError::InvalidPropertyValue),
        PropertyValue::Float(_) => val
            .as_f64()
            .map(PropertyValue::Float)
            .ok_or(MmError::InvalidPropertyValue),
    }
}

fn validate_property_value(prop: &PropertyDescription, value: &PropertyValue) -> MmResult<()> {
    if !prop.allowed_values.is_empty()
        && !prop.allowed_values.iter().any(|v| v == &value.to_string())
    {
        return Err(MmError::InvalidPropertyValue);
    }
    if let Some((lower, upper)) = prop.limits {
        let numeric = value.as_f64().ok_or(MmError::InvalidPropertyValue)?;
        if numeric < lower || numeric > upper {
            return Err(MmError::InvalidPropertyValue);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::universal_hub_serial::hub::parse_device_descriptions;
    use crate::transport::MockTransport;

    fn generic_description() -> DeviceDescription {
        let records = vec![
            "Name|Generic-A".to_string(),
            "Description|Generic device".to_string(),
            "Timeout|2".to_string(),
            "PropertyString|Mode|Auto|false|Auto:Manual".to_string(),
            "PropertyIntegerAction|Power|5|false|SetPower|false|0:10".to_string(),
        ];
        parse_device_descriptions(&records).unwrap().remove(0)
    }

    #[test]
    fn registers_described_properties() {
        let dev = UniversalGeneric::from_description(generic_description()).unwrap();

        assert_eq!(dev.description(), "Generic device");
        assert_eq!(dev.timeout(), Duration::from_secs(2));
        assert!(dev.has_property("Mode"));
        assert!(dev.has_property("Power"));
        assert_eq!(
            dev.get_property("Power").unwrap(),
            PropertyValue::Integer(5)
        );
    }

    #[test]
    fn action_property_sends_upstream_command_and_marks_busy() {
        let t = MockTransport::new().expect("Generic-A>SetPower>7;", "Generic-A<SetPower<0:7;");
        let mut dev = UniversalGeneric::from_description(generic_description())
            .unwrap()
            .with_transport(Box::new(t));

        dev.set_property("Power", PropertyValue::Integer(7))
            .unwrap();

        assert!(dev.busy());
        assert_eq!(
            dev.get_property("Power").unwrap(),
            PropertyValue::Integer(7)
        );
    }

    #[test]
    fn controller_reply_updates_action_property_and_busy_state() {
        let t = MockTransport::new().any("Generic-A<SetPower<0:9;");
        let mut dev = UniversalGeneric::from_description(generic_description())
            .unwrap()
            .with_transport(Box::new(t));
        dev.busy = true;

        let reply = dev.poll_update_once().unwrap().unwrap();

        assert_eq!(reply.values, ["9"]);
        assert!(!dev.busy());
        assert_eq!(
            dev.get_property("Power").unwrap(),
            PropertyValue::Integer(9)
        );
    }

    #[test]
    fn described_limits_are_enforced_before_command() {
        let t = MockTransport::new();
        let mut dev = UniversalGeneric::from_description(generic_description())
            .unwrap()
            .with_transport(Box::new(t));

        assert_eq!(
            dev.set_property("Power", PropertyValue::Integer(11))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn controller_timeout_reply_updates_timeout_without_property_command() {
        let t = MockTransport::new().any("Generic-A<Timeout<0:3.5;");
        let mut dev = UniversalGeneric::from_description(generic_description())
            .unwrap()
            .with_transport(Box::new(t));
        dev.busy = true;

        let reply = dev.poll_update_once().unwrap().unwrap();

        assert_eq!(reply.command, "Timeout");
        assert_eq!(dev.timeout(), Duration::from_millis(3500));
        assert!(!dev.busy());
    }
}
