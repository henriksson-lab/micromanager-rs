//! Typed child devices for the Universal Serial Hub.

use super::hub::{
    parse_controller_reply, ControllerReply, DeviceDescription, PropertyDescription,
    SharedTransport, UniversalHub, CTR_BUSY, CTR_OK, TIMEOUT,
};
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter, Stage, StateDevice, XYStage};
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

const CASHED: &str = "cashed";
const NOT_SUPPORTED: &str = "not supported";
const SET_OPEN: &str = "SetOpen";
const GET_OPEN: &str = "GetOpen";
const FIRE: &str = "fire";
const SET_POSITION_UM: &str = "SetPositionUm";
const GET_POSITION_UM: &str = "GetPositionUm";
const HOME: &str = "Home";
const STOP: &str = "Stop";
const STATE: &str = "State";
const LABEL: &str = "Label";
const POSITION: &str = "Position";
const POSITION_X: &str = "PositionX";
const POSITION_Y: &str = "PositionY";

struct UniversalChildCore {
    name: String,
    description: String,
    device_type: DeviceType,
    props: PropertyMap,
    described_properties: HashMap<String, PropertyDescription>,
    action_by_command: HashMap<String, String>,
    method_by_command: HashMap<String, String>,
    command_by_method: HashMap<String, String>,
    transport: Option<SharedTransport>,
    initialized: bool,
    busy: bool,
    timeout: Duration,
    last_command: Option<Instant>,
    open: bool,
    position_um: f64,
    x_um: f64,
    y_um: f64,
    state: u64,
    labels: Vec<String>,
    state_first: u64,
    state_count: u64,
    z_limits: (f64, f64),
    xy_limits: (f64, f64, f64, f64),
}

impl UniversalChildCore {
    fn prototype(name: &str, device_type: DeviceType) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            name: name.to_string(),
            description: "Universal Serial Hub child prototype".into(),
            device_type,
            props,
            described_properties: HashMap::new(),
            action_by_command: HashMap::new(),
            method_by_command: HashMap::new(),
            command_by_method: HashMap::new(),
            transport: None,
            initialized: false,
            busy: false,
            timeout: Duration::from_secs(1),
            last_command: None,
            open: false,
            position_um: 0.0,
            x_um: 0.0,
            y_um: 0.0,
            state: 0,
            labels: Vec::new(),
            state_first: 0,
            state_count: 0,
            z_limits: (0.0, 0.0),
            xy_limits: (0.0, 0.0, 0.0, 0.0),
        }
    }

    fn from_description(description: DeviceDescription, expected: &str) -> MmResult<Self> {
        if !description.valid {
            return Err(MmError::SerialInvalidResponse);
        }
        if description.device_type != expected {
            return Err(MmError::WrongDeviceType);
        }
        let device_type = match expected {
            "Shutter" => DeviceType::Shutter,
            "State" => DeviceType::State,
            "Stage" => DeviceType::Stage,
            "XYStage" => DeviceType::XYStage,
            _ => return Err(MmError::WrongDeviceType),
        };
        let mut core = Self::prototype(&description.name, device_type);
        core.description = description.description;
        core.timeout = description.timeout;
        for method in description.methods {
            core.command_by_method
                .insert(method.method.clone(), method.command.clone());
            core.method_by_command.insert(method.command, method.method);
        }
        for prop in description.properties {
            if prop.pre_init {
                continue;
            }
            core.define_described_property(prop)?;
        }
        core.load_special_state();
        Ok(core)
    }

    fn with_shared_transport(mut self, transport: SharedTransport) -> Self {
        self.transport = Some(transport);
        self
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

    fn load_special_state(&mut self) {
        if let Some(prop) = self.described_properties.get(STATE) {
            self.state = prop.value.as_i64().unwrap_or(0).max(0) as u64;
            if let Some((lower, upper)) = prop.limits {
                self.state_first = lower.max(0.0) as u64;
                self.state_count = (upper - lower + 1.0).max(0.0) as u64;
            }
        }
        if let Some(prop) = self.described_properties.get(LABEL) {
            self.labels = prop.allowed_values.clone();
        }
        if let Some(prop) = self.described_properties.get(POSITION) {
            self.position_um = prop.value.as_f64().unwrap_or(0.0);
            self.z_limits = prop.limits.unwrap_or((0.0, 0.0));
        }
        if let Some(prop) = self.described_properties.get(POSITION_X) {
            self.x_um = prop.value.as_f64().unwrap_or(0.0);
            let (lower, upper) = prop.limits.unwrap_or((0.0, 0.0));
            self.xy_limits.0 = lower;
            self.xy_limits.1 = upper;
        }
        if let Some(prop) = self.described_properties.get(POSITION_Y) {
            self.y_um = prop.value.as_f64().unwrap_or(0.0);
            let (lower, upper) = prop.limits.unwrap_or((0.0, 0.0));
            self.xy_limits.2 = lower;
            self.xy_limits.3 = upper;
        }
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn crate::transport::Transport) -> MmResult<R>,
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

    fn command_for_method(&self, method: &str) -> MmResult<Option<String>> {
        let command = self
            .command_by_method
            .get(method)
            .ok_or(MmError::UnsupportedCommand)?;
        if command == NOT_SUPPORTED {
            return Err(MmError::UnsupportedCommand);
        }
        if command == CASHED {
            return Ok(None);
        }
        Ok(Some(command.clone()))
    }

    fn send_method(&mut self, method: &str, values: &[String]) -> MmResult<()> {
        let Some(command) = self.command_for_method(method)? else {
            return Ok(());
        };
        let cmd = UniversalHub::make_output_command(&self.name, &command, values);
        self.call_transport(|t| t.send(&cmd))?;
        self.last_command = Some(Instant::now());
        self.busy = true;
        Ok(())
    }

    fn set_described_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        let prop = self
            .described_properties
            .get(name)
            .cloned()
            .ok_or_else(|| MmError::UnknownLabel(name.to_string()))?;

        if !prop.action {
            self.props.set(name, val)?;
            self.load_special_state();
            return Ok(());
        }
        if prop.read_only {
            return Ok(());
        }

        let command = prop.command.as_deref().ok_or(MmError::UnsupportedCommand)?;
        let value = coerce_property_value(&prop.value, val)?;
        validate_property_value(&prop, &value)?;
        let cmd = UniversalHub::make_output_command(&self.name, command, &[value.to_string()]);
        self.call_transport(|t| t.send(&cmd))?;
        self.last_command = Some(Instant::now());
        self.busy = true;
        self.props.set(name, value)?;
        self.load_special_state();
        Ok(())
    }

    fn apply_controller_reply(&mut self, reply: &ControllerReply) -> MmResult<()> {
        if reply.device_name != self.name {
            return Err(MmError::DeviceNotFound(reply.device_name.clone()));
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
            return Ok(());
        }
        if let Some(method) = self.method_by_command.get(&reply.command).cloned() {
            self.apply_method_values(&method, &reply.values)?;
            return Ok(());
        }
        if let Some(value) = reply.values.first() {
            self.apply_controller_value(&reply.command, value)?;
        }
        Ok(())
    }

    fn apply_method_values(&mut self, method: &str, values: &[String]) -> MmResult<()> {
        match (self.device_type, method) {
            (DeviceType::Shutter, SET_OPEN) | (DeviceType::Shutter, GET_OPEN) => {
                self.open = parse_bool_value(values.first())?;
            }
            (DeviceType::Stage, SET_POSITION_UM)
            | (DeviceType::Stage, GET_POSITION_UM)
            | (DeviceType::Stage, HOME)
            | (DeviceType::Stage, STOP) => {
                self.position_um = parse_f64_value(values.first())?;
                self.set_property_cache(POSITION, PropertyValue::Float(self.position_um));
            }
            (DeviceType::XYStage, SET_POSITION_UM)
            | (DeviceType::XYStage, GET_POSITION_UM)
            | (DeviceType::XYStage, HOME)
            | (DeviceType::XYStage, STOP) => {
                self.x_um = parse_f64_value(values.first())?;
                self.y_um = parse_f64_value(values.get(1))?;
                self.set_property_cache(POSITION_X, PropertyValue::Float(self.x_um));
                self.set_property_cache(POSITION_Y, PropertyValue::Float(self.y_um));
            }
            _ => {}
        }
        Ok(())
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
        self.set_property_cache(&name, value);
        self.load_special_state();
        Ok(())
    }

    fn set_property_cache(&mut self, name: &str, value: PropertyValue) {
        if let Some(entry) = self.props.entry_mut(name) {
            entry.value = value;
        }
    }

    fn poll_update_once(&mut self) -> MmResult<Option<ControllerReply>> {
        match self.call_transport(|t| t.receive_line()) {
            Ok(line) => {
                let reply = parse_controller_reply(&line)?;
                self.apply_controller_reply(&reply)?;
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
        }
    }

    fn busy(&self) -> bool {
        self.busy
            && self
                .last_command
                .map(|t| t.elapsed() <= self.timeout)
                .unwrap_or(true)
    }
}

fn lock_core<'a>(
    core: &'a Mutex<UniversalChildCore>,
) -> MmResult<MutexGuard<'a, UniversalChildCore>> {
    core.lock()
        .map_err(|_| MmError::LocallyDefined("Universal child lock poisoned".into()))
}

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

fn parse_bool_value(value: Option<&String>) -> MmResult<bool> {
    let value = value.ok_or(MmError::SerialInvalidResponse)?;
    match value.as_str() {
        "0" | "false" => Ok(false),
        "1" | "true" => Ok(true),
        _ => Err(MmError::InvalidPropertyValue),
    }
}

fn parse_f64_value(value: Option<&String>) -> MmResult<f64> {
    value
        .ok_or(MmError::SerialInvalidResponse)?
        .parse::<f64>()
        .map_err(|_| MmError::SerialInvalidResponse)
}

macro_rules! impl_device {
    ($ty:ty, $dtype:expr) => {
        impl Device for $ty {
            fn name(&self) -> &str {
                &self.name
            }

            fn description(&self) -> &str {
                &self.description
            }

            fn initialize(&mut self) -> MmResult<()> {
                lock_core(&self.core)?.initialized = true;
                Ok(())
            }

            fn shutdown(&mut self) -> MmResult<()> {
                let mut core = lock_core(&self.core)?;
                core.initialized = false;
                core.busy = false;
                core.last_command = None;
                Ok(())
            }

            fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
                lock_core(&self.core)?.props.get(name).cloned()
            }

            fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
                let mut core = lock_core(&self.core)?;
                if core.described_properties.contains_key(name) {
                    core.set_described_property(name, val)
                } else {
                    core.props.set(name, val)
                }
            }

            fn property_names(&self) -> Vec<String> {
                lock_core(&self.core)
                    .map(|core| core.props.property_names().to_vec())
                    .unwrap_or_default()
            }

            fn has_property(&self, name: &str) -> bool {
                lock_core(&self.core)
                    .map(|core| core.props.has_property(name))
                    .unwrap_or(false)
            }

            fn is_property_read_only(&self, name: &str) -> bool {
                lock_core(&self.core)
                    .ok()
                    .and_then(|core| core.props.entry(name).map(|e| e.read_only))
                    .unwrap_or(false)
            }

            fn device_type(&self) -> DeviceType {
                $dtype
            }

            fn busy(&self) -> bool {
                lock_core(&self.core)
                    .map(|core| core.busy())
                    .unwrap_or(false)
            }
        }
    };
}

pub struct UniversalShutter {
    name: String,
    description: String,
    core: Mutex<UniversalChildCore>,
}

impl UniversalShutter {
    pub fn new(name: &str) -> Self {
        let core = UniversalChildCore::prototype(name, DeviceType::Shutter);
        Self {
            name: core.name.clone(),
            description: core.description.clone(),
            core: Mutex::new(core),
        }
    }

    pub fn from_description(description: DeviceDescription) -> MmResult<Self> {
        let core = UniversalChildCore::from_description(description, "Shutter")?;
        Ok(Self {
            name: core.name.clone(),
            description: core.description.clone(),
            core: Mutex::new(core),
        })
    }

    pub fn with_shared_transport(self, transport: SharedTransport) -> Self {
        let core = self
            .core
            .into_inner()
            .expect("Universal child lock poisoned");
        Self {
            name: self.name,
            description: self.description,
            core: Mutex::new(core.with_shared_transport(transport)),
        }
    }

    pub fn poll_update_once(&self) -> MmResult<Option<ControllerReply>> {
        lock_core(&self.core)?.poll_update_once()
    }
}

impl_device!(UniversalShutter, DeviceType::Shutter);

impl Shutter for UniversalShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let mut core = lock_core(&self.core)?;
        core.open = open;
        core.send_method(SET_OPEN, &[if open { "1" } else { "0" }.to_string()])
    }

    fn get_open(&self) -> MmResult<bool> {
        let mut core = lock_core(&self.core)?;
        let cached = if core.open { "1" } else { "0" }.to_string();
        core.send_method(GET_OPEN, &[cached])?;
        Ok(core.open)
    }

    fn fire(&mut self, delta_t: f64) -> MmResult<()> {
        lock_core(&self.core)?.send_method(FIRE, &[delta_t.to_string()])
    }
}

pub struct UniversalStateDevice {
    name: String,
    description: String,
    core: Mutex<UniversalChildCore>,
}

impl UniversalStateDevice {
    pub fn new(name: &str) -> Self {
        let core = UniversalChildCore::prototype(name, DeviceType::State);
        Self {
            name: core.name.clone(),
            description: core.description.clone(),
            core: Mutex::new(core),
        }
    }

    pub fn from_description(description: DeviceDescription) -> MmResult<Self> {
        let core = UniversalChildCore::from_description(description, "State")?;
        Ok(Self {
            name: core.name.clone(),
            description: core.description.clone(),
            core: Mutex::new(core),
        })
    }

    pub fn with_shared_transport(self, transport: SharedTransport) -> Self {
        let core = self
            .core
            .into_inner()
            .expect("Universal child lock poisoned");
        Self {
            name: self.name,
            description: self.description,
            core: Mutex::new(core.with_shared_transport(transport)),
        }
    }

    pub fn poll_update_once(&self) -> MmResult<Option<ControllerReply>> {
        lock_core(&self.core)?.poll_update_once()
    }
}

impl_device!(UniversalStateDevice, DeviceType::State);

impl StateDevice for UniversalStateDevice {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        self.set_property(STATE, PropertyValue::Integer(pos as i64))
    }

    fn get_position(&self) -> MmResult<u64> {
        Ok(lock_core(&self.core)?.state)
    }

    fn get_number_of_positions(&self) -> u64 {
        lock_core(&self.core)
            .map(|core| core.state_count)
            .unwrap_or(0)
    }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        let core = lock_core(&self.core)?;
        let index = pos
            .checked_sub(core.state_first)
            .ok_or(MmError::UnknownPosition)? as usize;
        core.labels
            .get(index)
            .cloned()
            .ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = {
            let core = lock_core(&self.core)?;
            core.labels
                .iter()
                .position(|l| l == label)
                .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64
                + core.state_first
        };
        self.set_position(pos)
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        let mut core = lock_core(&self.core)?;
        let index = pos
            .checked_sub(core.state_first)
            .ok_or(MmError::UnknownPosition)? as usize;
        if index >= core.labels.len() {
            return Err(MmError::UnknownPosition);
        }
        core.labels[index] = label.to_string();
        Ok(())
    }

    fn set_gate_open(&mut self, _open: bool) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }

    fn get_gate_open(&self) -> MmResult<bool> {
        Err(MmError::UnsupportedCommand)
    }
}

pub struct UniversalStage {
    name: String,
    description: String,
    core: Mutex<UniversalChildCore>,
}

impl UniversalStage {
    pub fn new(name: &str) -> Self {
        let core = UniversalChildCore::prototype(name, DeviceType::Stage);
        Self {
            name: core.name.clone(),
            description: core.description.clone(),
            core: Mutex::new(core),
        }
    }

    pub fn from_description(description: DeviceDescription) -> MmResult<Self> {
        let core = UniversalChildCore::from_description(description, "Stage")?;
        Ok(Self {
            name: core.name.clone(),
            description: core.description.clone(),
            core: Mutex::new(core),
        })
    }

    pub fn with_shared_transport(self, transport: SharedTransport) -> Self {
        let core = self
            .core
            .into_inner()
            .expect("Universal child lock poisoned");
        Self {
            name: self.name,
            description: self.description,
            core: Mutex::new(core.with_shared_transport(transport)),
        }
    }

    pub fn poll_update_once(&self) -> MmResult<Option<ControllerReply>> {
        lock_core(&self.core)?.poll_update_once()
    }
}

impl_device!(UniversalStage, DeviceType::Stage);

impl Stage for UniversalStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let mut core = lock_core(&self.core)?;
        core.position_um = pos;
        core.set_property_cache(POSITION, PropertyValue::Float(pos));
        core.send_method(SET_POSITION_UM, &[pos.to_string()])
    }

    fn get_position_um(&self) -> MmResult<f64> {
        let mut core = lock_core(&self.core)?;
        let cached = core.position_um.to_string();
        core.send_method(GET_POSITION_UM, &[cached])?;
        Ok(core.position_um)
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let pos = lock_core(&self.core)?.position_um + d;
        self.set_position_um(pos)
    }

    fn home(&mut self) -> MmResult<()> {
        lock_core(&self.core)?.send_method(HOME, &["0".to_string()])
    }

    fn stop(&mut self) -> MmResult<()> {
        lock_core(&self.core)?.send_method(STOP, &["0".to_string()])
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok(lock_core(&self.core)?.z_limits)
    }

    fn get_focus_direction(&self) -> FocusDirection {
        FocusDirection::Unknown
    }

    fn is_continuous_focus_drive(&self) -> bool {
        false
    }
}

pub struct UniversalXYStage {
    name: String,
    description: String,
    core: Mutex<UniversalChildCore>,
}

impl UniversalXYStage {
    pub fn new(name: &str) -> Self {
        let core = UniversalChildCore::prototype(name, DeviceType::XYStage);
        Self {
            name: core.name.clone(),
            description: core.description.clone(),
            core: Mutex::new(core),
        }
    }

    pub fn from_description(description: DeviceDescription) -> MmResult<Self> {
        let core = UniversalChildCore::from_description(description, "XYStage")?;
        Ok(Self {
            name: core.name.clone(),
            description: core.description.clone(),
            core: Mutex::new(core),
        })
    }

    pub fn with_shared_transport(self, transport: SharedTransport) -> Self {
        let core = self
            .core
            .into_inner()
            .expect("Universal child lock poisoned");
        Self {
            name: self.name,
            description: self.description,
            core: Mutex::new(core.with_shared_transport(transport)),
        }
    }

    pub fn poll_update_once(&self) -> MmResult<Option<ControllerReply>> {
        lock_core(&self.core)?.poll_update_once()
    }
}

impl_device!(UniversalXYStage, DeviceType::XYStage);

impl XYStage for UniversalXYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        let mut core = lock_core(&self.core)?;
        core.x_um = x;
        core.y_um = y;
        core.set_property_cache(POSITION_X, PropertyValue::Float(x));
        core.set_property_cache(POSITION_Y, PropertyValue::Float(y));
        core.send_method(SET_POSITION_UM, &[x.to_string(), y.to_string()])
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        let mut core = lock_core(&self.core)?;
        let cached_x = core.x_um.to_string();
        let cached_y = core.y_um.to_string();
        core.send_method(GET_POSITION_UM, &[cached_x, cached_y])?;
        Ok((core.x_um, core.y_um))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let (x, y) = {
            let core = lock_core(&self.core)?;
            (core.x_um + dx, core.y_um + dy)
        };
        self.set_xy_position_um(x, y)
    }

    fn home(&mut self) -> MmResult<()> {
        lock_core(&self.core)?.send_method(HOME, &["0".to_string(), "0".to_string()])
    }

    fn stop(&mut self) -> MmResult<()> {
        lock_core(&self.core)?.send_method(STOP, &["0".to_string(), "0".to_string()])
    }

    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok(lock_core(&self.core)?.xy_limits)
    }

    fn get_step_size_um(&self) -> (f64, f64) {
        (1.0, 1.0)
    }

    fn set_origin(&mut self) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::universal_hub_serial::hub::parse_device_descriptions;
    use crate::transport::MockTransport;

    #[test]
    fn shutter_methods_send_upstream_commands_and_apply_reply() {
        let records = vec![
            "Name|Shutter-A".to_string(),
            "Description|Shutter device".to_string(),
            "Command|SetOpen|SO".to_string(),
            "Command|GetOpen|GO".to_string(),
            "Command|fire|FI".to_string(),
        ];
        let description = parse_device_descriptions(&records).unwrap().remove(0);
        let t = MockTransport::new()
            .expect("Shutter-A>SO>1;", "Shutter-A<SO<0:1;")
            .expect("Shutter-A>GO>1;", "Shutter-A<GO<0:1;");
        let mut shutter = UniversalShutter::from_description(description)
            .unwrap()
            .with_shared_transport(std::sync::Arc::new(std::sync::Mutex::new(Box::new(t))));

        shutter.set_open(true).unwrap();
        assert!(shutter.busy());
        shutter.poll_update_once().unwrap();
        assert!(!shutter.busy());
        assert!(shutter.get_open().unwrap());
    }

    #[test]
    fn stage_methods_send_commands_and_update_position_from_reply() {
        let records = vec![
            "Name|Stage-A".to_string(),
            "Description|Stage device".to_string(),
            "PropertyFloat|Position|0|false|-100:100".to_string(),
            "Command|SetPositionUm|SP".to_string(),
            "Command|GetPositionUm|GP".to_string(),
            "Command|Home|HM".to_string(),
            "Command|Stop|ST".to_string(),
        ];
        let description = parse_device_descriptions(&records).unwrap().remove(0);
        let t = MockTransport::new()
            .expect("Stage-A>SP>12.5;", "Stage-A<SP<0:12.5;")
            .expect("Stage-A>GP>12.5;", "Stage-A<GP<0:13;");
        let mut stage = UniversalStage::from_description(description)
            .unwrap()
            .with_shared_transport(std::sync::Arc::new(std::sync::Mutex::new(Box::new(t))));

        stage.set_position_um(12.5).unwrap();
        stage.poll_update_once().unwrap();
        assert_eq!(stage.get_position_um().unwrap(), 12.5);
        stage.poll_update_once().unwrap();
        assert_eq!(
            stage.get_property("Position").unwrap(),
            PropertyValue::Float(13.0)
        );
        assert_eq!(stage.get_limits().unwrap(), (-100.0, 100.0));
    }
}
