/// Universal MM Hub Serial - configurable serial hub.
///
/// Upstream UniversalMMHubSerial discovers devices with a controller setup
/// stream: send `Start;`, then repeatedly receive setup records and send
/// `Next;` until `End`. Setup records are `|` separated. Runtime output
/// commands use `device>command>value;`; controller replies use
/// `device<command<error:value;`.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Hub};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const SETUP_SEP: char = '|';
pub const OUT_SEP: char = '>';
pub const IN_SEP: char = '<';
pub const WITHIN_SEP: char = ':';
pub const END_SEP: char = ';';

const DEVICE_LIST_START: &str = "Start";
const DEVICE_LIST_CONTINUE: &str = "Next";
const DEVICE_LIST_END: &str = "End";
const WORD_TRUE: &str = "true";
const WORD_FALSE: &str = "false";
pub(crate) const TIMEOUT: &str = "Timeout";
const COMMAND: &str = "Command";

pub(crate) const CTR_OK: i64 = 0;
pub(crate) const CTR_BUSY: i64 = 1;

pub type SharedTransport = Arc<Mutex<Box<dyn Transport>>>;

#[derive(Debug, Clone, PartialEq)]
pub struct MethodDescription {
    pub method: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropertyDescription {
    pub name: String,
    pub read_only: bool,
    pub pre_init: bool,
    pub action: bool,
    pub command: Option<String>,
    pub value: PropertyValue,
    pub allowed_values: Vec<String>,
    pub limits: Option<(f64, f64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceDescription {
    pub valid: bool,
    pub invalid_reason: String,
    pub device_type: String,
    pub name: String,
    pub description: String,
    pub timeout: Duration,
    pub methods: Vec<MethodDescription>,
    pub properties: Vec<PropertyDescription>,
}

/// Description of a sub-device discovered by the hub.
#[derive(Debug, Clone, PartialEq)]
pub struct SubDeviceInfo {
    pub name: String,
    pub device_type: String,
    pub description: String,
}

pub struct UniversalHub {
    props: PropertyMap,
    transport: Option<SharedTransport>,
    initialized: bool,
    busy: bool,
    last_command: Option<Instant>,
    timeout: Duration,
    sub_devices: Vec<SubDeviceInfo>,
    descriptions: Vec<DeviceDescription>,
    last_error: i64,
    last_error_description: String,
}

impl UniversalHub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            busy: false,
            last_command: None,
            timeout: Duration::from_secs(1),
            sub_devices: Vec::new(),
            descriptions: Vec::new(),
            last_error: 0,
            last_error_description: "none".into(),
        }
    }

    fn ensure_error_properties(&mut self) -> MmResult<()> {
        if !self.props.has_property("Error") {
            self.props
                .define_property("Error", PropertyValue::Integer(self.last_error), false)?;
        }
        if !self.props.has_property("Error Description") {
            self.props.define_property(
                "Error Description",
                PropertyValue::String(self.last_error_description.clone()),
                false,
            )?;
        }
        Ok(())
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(Arc::new(Mutex::new(t)));
        self
    }

    pub fn with_shared_transport(mut self, transport: SharedTransport) -> Self {
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
                    MmError::LocallyDefined("Universal transport lock poisoned".into())
                })?;
                f(guard.as_mut())
            }
            None => Err(MmError::NotConnected),
        }
    }

    pub fn shared_transport(&self) -> Option<SharedTransport> {
        self.transport.clone()
    }

    pub fn sub_devices(&self) -> &[SubDeviceInfo] {
        &self.sub_devices
    }

    pub fn device_descriptions(&self) -> &[DeviceDescription] {
        &self.descriptions
    }

    pub fn description_for(&self, name: &str) -> Option<&DeviceDescription> {
        self.descriptions.iter().find(|d| d.name == name)
    }

    pub fn command_for_method(&self, device_name: &str, method_name: &str) -> Option<&str> {
        self.description_for(device_name)?
            .methods
            .iter()
            .find(|m| m.method == method_name)
            .map(|m| m.command.as_str())
    }

    pub fn create_generic_child(
        &self,
        device_name: &str,
    ) -> MmResult<super::generic_device::UniversalGeneric> {
        let description = self
            .description_for(device_name)
            .cloned()
            .ok_or_else(|| MmError::DeviceNotFound(device_name.to_string()))?;
        let transport = self.shared_transport().ok_or(MmError::NotConnected)?;
        super::generic_device::UniversalGeneric::from_description(description)
            .map(|child| child.with_shared_transport(transport))
    }

    pub fn create_shutter_child(
        &self,
        device_name: &str,
    ) -> MmResult<super::device::UniversalShutter> {
        let description = self
            .description_for(device_name)
            .cloned()
            .ok_or_else(|| MmError::DeviceNotFound(device_name.to_string()))?;
        let transport = self.shared_transport().ok_or(MmError::NotConnected)?;
        super::device::UniversalShutter::from_description(description)
            .map(|child| child.with_shared_transport(transport))
    }

    pub fn create_state_child(
        &self,
        device_name: &str,
    ) -> MmResult<super::device::UniversalStateDevice> {
        let description = self
            .description_for(device_name)
            .cloned()
            .ok_or_else(|| MmError::DeviceNotFound(device_name.to_string()))?;
        let transport = self.shared_transport().ok_or(MmError::NotConnected)?;
        super::device::UniversalStateDevice::from_description(description)
            .map(|child| child.with_shared_transport(transport))
    }

    pub fn create_stage_child(&self, device_name: &str) -> MmResult<super::device::UniversalStage> {
        let description = self
            .description_for(device_name)
            .cloned()
            .ok_or_else(|| MmError::DeviceNotFound(device_name.to_string()))?;
        let transport = self.shared_transport().ok_or(MmError::NotConnected)?;
        super::device::UniversalStage::from_description(description)
            .map(|child| child.with_shared_transport(transport))
    }

    pub fn create_xy_stage_child(
        &self,
        device_name: &str,
    ) -> MmResult<super::device::UniversalXYStage> {
        let description = self
            .description_for(device_name)
            .cloned()
            .ok_or_else(|| MmError::DeviceNotFound(device_name.to_string()))?;
        let transport = self.shared_transport().ok_or(MmError::NotConnected)?;
        super::device::UniversalXYStage::from_description(description)
            .map(|child| child.with_shared_transport(transport))
    }

    pub fn make_output_command(device_name: &str, command: &str, values: &[String]) -> String {
        let joined = values.join(&WITHIN_SEP.to_string());
        format!("{device_name}{OUT_SEP}{command}{OUT_SEP}{joined}{END_SEP}")
    }

    pub fn send_output_command(
        &mut self,
        device_name: &str,
        command: &str,
        values: &[String],
    ) -> MmResult<()> {
        let cmd = Self::make_output_command(device_name, command, values);
        self.last_command = Some(Instant::now());
        self.busy = true;
        self.call_transport(|t| t.send(&cmd))
    }

    pub fn poll_update_once(&mut self) -> MmResult<Option<ControllerReply>> {
        match self.call_transport(|t| t.receive_line()) {
            Ok(line) => {
                let reply = parse_controller_reply(&line)?;
                if reply.error == CTR_BUSY {
                    self.busy = true;
                    self.last_command = Some(Instant::now());
                } else {
                    self.busy = false;
                }
                if reply.error != CTR_OK && reply.error != CTR_BUSY {
                    self.record_error(
                        reply.error,
                        format!("{}<{}<{}", reply.device_name, reply.command, reply.error),
                    );
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

    fn update_timeout_state(&mut self) {
        if self.busy
            && self
                .last_command
                .map(|t| t.elapsed() > self.timeout)
                .unwrap_or(false)
        {
            self.busy = false;
            self.record_error(402, "Lost communication with the controller".into());
        }
    }

    fn record_error(&mut self, code: i64, description: String) {
        self.last_error = code;
        self.last_error_description = description.clone();
        let _ = self.props.set("Error", PropertyValue::Integer(code));
        let _ = self
            .props
            .set("Error Description", PropertyValue::String(description));
    }

    fn populate_device_descriptions(&mut self) -> MmResult<Vec<DeviceDescription>> {
        self.call_transport(|t| t.send(&format!("{DEVICE_LIST_START}{END_SEP}")))?;

        let mut records = Vec::new();
        loop {
            let ans = self
                .call_transport(|t| t.receive_line())?
                .trim()
                .trim_end_matches(END_SEP)
                .to_string();
            if ans == DEVICE_LIST_END {
                break;
            }
            records.push(ans);
            self.call_transport(|t| t.send(&format!("{DEVICE_LIST_CONTINUE}{END_SEP}")))?;
        }
        parse_device_descriptions(&records)
    }
}

impl Default for UniversalHub {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for UniversalHub {
    fn name(&self) -> &str {
        "UniversalMMHubSerial"
    }

    fn description(&self) -> &str {
        "Universal hardware hub (serial)"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.ensure_error_properties()?;
        self.call_transport(|t| t.purge())?;
        let descriptions = self.populate_device_descriptions()?;
        self.sub_devices = descriptions
            .iter()
            .filter(|d| d.valid)
            .map(|d| SubDeviceInfo {
                name: d.name.clone(),
                device_type: d.device_type.clone(),
                description: d.description.clone(),
            })
            .collect();
        self.descriptions = descriptions;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        self.busy = false;
        self.last_command = None;
        self.descriptions.clear();
        self.sub_devices.clear();
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Port" && self.initialized {
            return Err(MmError::CanNotSetProperty);
        }
        self.props.set(name, val)
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
        self.busy
            && self
                .last_command
                .map(|t| t.elapsed() <= self.timeout)
                .unwrap_or(true)
    }
}

impl Hub for UniversalHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        Ok(self.sub_devices.iter().map(|d| d.name.clone()).collect())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ControllerReply {
    pub device_name: String,
    pub command: String,
    pub error: i64,
    pub values: Vec<String>,
}

pub fn parse_controller_reply(line: &str) -> MmResult<ControllerReply> {
    let parts = split_trimmed(line, IN_SEP);
    if parts.len() != 3 {
        return Err(MmError::SerialInvalidResponse);
    }
    let mut values = split_trimmed(&parts[2], WITHIN_SEP);
    if values.is_empty() {
        return Err(MmError::SerialInvalidResponse);
    }
    let error = values
        .remove(0)
        .parse::<i64>()
        .map_err(|_| MmError::SerialInvalidResponse)?;
    Ok(ControllerReply {
        device_name: parts[0].clone(),
        command: parts[1].clone(),
        error,
        values,
    })
}

pub fn parse_device_descriptions(records: &[String]) -> MmResult<Vec<DeviceDescription>> {
    let mut devices = Vec::new();
    let mut current = Vec::new();
    for record in records {
        let words = split_trimmed(record, SETUP_SEP);
        if words.first().map(String::as_str) == Some("Name") && !current.is_empty() {
            devices.push(parse_device_description(&current));
            current.clear();
        }
        current.push(record.clone());
    }
    if !current.is_empty() {
        devices.push(parse_device_description(&current));
    }
    Ok(devices)
}

fn parse_device_description(records: &[String]) -> DeviceDescription {
    let mut dev = DeviceDescription {
        valid: true,
        invalid_reason: String::new(),
        device_type: String::new(),
        name: String::new(),
        description: String::new(),
        timeout: Duration::from_secs(1),
        methods: Vec::new(),
        properties: Vec::new(),
    };

    for record in records {
        let words = split_trimmed(record, SETUP_SEP);
        if words.len() < 2 {
            invalidate(&mut dev, format!("Invalid string: {record}"));
            break;
        }

        match words[0].as_str() {
            "Name" => {
                dev.name = words[1].clone();
                match device_type_from_name(&dev.name) {
                    Some(device_type) => dev.device_type = device_type.to_string(),
                    None => {
                        invalidate(
                            &mut dev,
                            format!("Unable to determine device type for {record}"),
                        );
                        break;
                    }
                }
            }
            "Description" => dev.description = words[1].clone(),
            TIMEOUT => match words[1].parse::<f64>() {
                Ok(seconds) => dev.timeout = Duration::from_secs_f64(seconds.max(0.0)),
                Err(_) => {
                    invalidate(&mut dev, format!("Invalid timeout: {record}"));
                    break;
                }
            },
            COMMAND => {
                if words.len() < 3 {
                    invalidate(&mut dev, format!("Invalid command: {record}"));
                    break;
                }
                dev.methods.push(MethodDescription {
                    method: words[1].clone(),
                    command: words[2].clone(),
                });
            }
            key if key.starts_with("Property") => match parse_property_description(&words) {
                Ok(prop) => dev.properties.push(prop),
                Err(reason) => {
                    invalidate(&mut dev, format!("{reason}: {record}"));
                    break;
                }
            },
            _ => {}
        }
    }
    dev
}

fn parse_property_description(words: &[String]) -> Result<PropertyDescription, &'static str> {
    let action = words[0].contains("Action");
    let expected = if action { 7 } else { 5 };
    if words.len() != expected {
        return Err("Invalid property");
    }
    let read_only = parse_bool(&words[3]).ok_or("Unable to determine read-only status")?;
    let pre_init = if action {
        parse_bool(&words[5]).ok_or("Unable to determine pre-initialization status")?
    } else {
        false
    };
    let limits_or_values = if action { &words[6] } else { &words[4] };
    let (value, allowed_values, limits) = if words[0].starts_with("PropertyString") {
        let allowed = if read_only {
            Vec::new()
        } else {
            split_trimmed(limits_or_values, WITHIN_SEP)
        };
        (PropertyValue::String(words[2].clone()), allowed, None)
    } else if words[0].starts_with("PropertyInteger") {
        let value = words[2]
            .parse::<i64>()
            .map_err(|_| "Invalid integer property value")?;
        let limits = if read_only {
            None
        } else {
            Some(parse_limits(limits_or_values)?)
        };
        (PropertyValue::Integer(value), Vec::new(), limits)
    } else if words[0].starts_with("PropertyFloat") {
        let value = words[2]
            .parse::<f64>()
            .map_err(|_| "Invalid float property value")?;
        let limits = if read_only {
            None
        } else {
            Some(parse_limits(limits_or_values)?)
        };
        (PropertyValue::Float(value), Vec::new(), limits)
    } else {
        return Err("Unable to determine property type");
    };

    Ok(PropertyDescription {
        name: words[1].clone(),
        read_only,
        pre_init,
        action,
        command: action.then(|| words[4].clone()),
        value,
        allowed_values,
        limits,
    })
}

fn parse_limits(s: &str) -> Result<(f64, f64), &'static str> {
    let vals = split_trimmed(s, WITHIN_SEP);
    if vals.len() != 2 {
        return Err("Unable to determine property limits");
    }
    let lower = vals[0]
        .parse::<f64>()
        .map_err(|_| "Invalid property limit")?;
    let upper = vals[1]
        .parse::<f64>()
        .map_err(|_| "Invalid property limit")?;
    Ok((lower, upper))
}

fn split_trimmed(s: &str, sep: char) -> Vec<String> {
    s.trim()
        .trim_end_matches(END_SEP)
        .split(sep)
        .map(|part| part.trim().to_string())
        .collect()
}

fn parse_bool(s: &str) -> Option<bool> {
    match s {
        WORD_TRUE => Some(true),
        WORD_FALSE => Some(false),
        _ => None,
    }
}

fn device_type_from_name(name: &str) -> Option<&'static str> {
    if name.starts_with("Shutter") {
        Some("Shutter")
    } else if name.starts_with("State") {
        Some("State")
    } else if name.starts_with("Stage") {
        Some("Stage")
    } else if name.starts_with("XYStage") {
        Some("XYStage")
    } else if name.starts_with("Generic") {
        Some("Generic")
    } else {
        None
    }
}

fn invalidate(dev: &mut DeviceDescription, reason: String) {
    dev.valid = false;
    dev.invalid_reason = reason;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn parses_upstream_setup_stream_with_properties() {
        let t = MockTransport::new()
            .expect("Start;", "Name|Generic-A")
            .expect("Next;", "Description|Generic device")
            .expect("Next;", "Timeout|2.5")
            .expect("Next;", "PropertyString|Mode|Auto|false|Auto:Manual")
            .expect(
                "Next;",
                "PropertyIntegerAction|Power|5|false|SetPower|false|0:10",
            )
            .expect("Next;", "Name|Shutter-A")
            .expect("Next;", "Description|Shutter device")
            .expect("Next;", "Command|SetOpen|SO")
            .expect("Next;", "End");
        let mut hub = UniversalHub::new().with_transport(Box::new(t));

        hub.initialize().unwrap();

        assert_eq!(hub.sub_devices().len(), 2);
        assert_eq!(hub.sub_devices()[0].name, "Generic-A");
        assert_eq!(hub.sub_devices()[0].device_type, "Generic");
        assert_eq!(hub.sub_devices()[1].device_type, "Shutter");
        let generic = hub.description_for("Generic-A").unwrap();
        assert_eq!(generic.timeout, Duration::from_millis(2500));
        assert_eq!(generic.properties.len(), 2);
        assert_eq!(generic.properties[0].allowed_values, ["Auto", "Manual"]);
        assert_eq!(generic.properties[1].command.as_deref(), Some("SetPower"));
        assert_eq!(hub.command_for_method("Shutter-A", "SetOpen"), Some("SO"));
    }

    #[test]
    fn invalid_device_name_is_retained_but_not_registered() {
        let t = MockTransport::new()
            .expect("Start;", "Name|Mystery")
            .expect("Next;", "Description|Unknown")
            .expect("Next;", "End");
        let mut hub = UniversalHub::new().with_transport(Box::new(t));

        hub.initialize().unwrap();

        assert!(hub.sub_devices().is_empty());
        assert_eq!(hub.device_descriptions().len(), 1);
        assert!(!hub.device_descriptions()[0].valid);
    }

    #[test]
    fn generic_child_created_from_hub_uses_shared_transport() {
        let t = MockTransport::new()
            .expect("Start;", "Name|Generic-A")
            .expect("Next;", "Description|Generic device")
            .expect(
                "Next;",
                "PropertyIntegerAction|Power|5|false|SetPower|false|0:10",
            )
            .expect("Next;", "End")
            .expect("Generic-A>SetPower>7;", "Generic-A<SetPower<0:7;");
        let mut hub = UniversalHub::new().with_transport(Box::new(t));

        hub.initialize().unwrap();
        let mut child = hub.create_generic_child("Generic-A").unwrap();
        child.initialize().unwrap();
        child
            .set_property("Power", PropertyValue::Integer(7))
            .unwrap();

        assert!(child.busy());
        assert_eq!(
            child.get_property("Power").unwrap(),
            PropertyValue::Integer(7)
        );
    }

    #[test]
    fn parses_controller_reply() {
        let reply = parse_controller_reply("Generic-A<SetPower<0:7;").unwrap();
        assert_eq!(reply.device_name, "Generic-A");
        assert_eq!(reply.command, "SetPower");
        assert_eq!(reply.error, 0);
        assert_eq!(reply.values, ["7"]);
    }

    #[test]
    fn no_transport_error() {
        let mut hub = UniversalHub::new();
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn error_reporter_properties_are_created_on_initialize() {
        let t = MockTransport::new().expect("Start;", "End");
        let mut hub = UniversalHub::new().with_transport(Box::new(t));

        assert_eq!(hub.property_names(), ["Port"]);

        hub.initialize().unwrap();

        assert_eq!(hub.property_names(), ["Port", "Error", "Error Description"]);
        assert_eq!(
            hub.get_property("Error").unwrap(),
            PropertyValue::Integer(0)
        );
        assert_eq!(
            hub.get_property("Error Description").unwrap(),
            PropertyValue::String("none".into())
        );
    }

    #[test]
    fn initialized_hub_rejects_port_changes() {
        let t = MockTransport::new()
            .expect("Start;", "Name|Generic-A")
            .expect("Next;", "End");
        let mut hub = UniversalHub::new().with_transport(Box::new(t));
        hub.set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        hub.initialize().unwrap();

        assert_eq!(
            hub.set_property("Port", PropertyValue::String("COM2".into()))
                .unwrap_err(),
            MmError::CanNotSetProperty
        );
        assert_eq!(
            hub.get_property("Port").unwrap(),
            PropertyValue::String("COM1".into())
        );
    }
}
