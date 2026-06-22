/// Generic XLight V3 state device implementation.
///
/// The V3 protocol uses a unified command structure:
///   - Query position:   `r<PREFIX>\r`   → `r<PREFIX><N>`
///   - Query num pos:    `r<PREFIX>N\r`  → `r<PREFIX>N<M>`
///   - Set position:     `<PREFIX><N>\r` → `<PREFIX><N>` (echo)
///
/// For filter wheels (one_based=true): MM 0-based ↔ wire 1-based.
/// For mechanical / motor (one_based=false): 0-based on both sides.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

const MAX_COMMAND_ATTEMPTS: usize = 10;

pub struct XLightV3StateDevice {
    name: &'static str,
    description: &'static str,
    prefix: &'static str,
    label_prefix: &'static str,
    one_based: bool, // true for filter wheels
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position: u64,
    num_positions: u64,
    labels: Vec<String>,
    gate_open: bool,
}

impl XLightV3StateDevice {
    fn new(
        name: &'static str,
        description: &'static str,
        prefix: &'static str,
        label_prefix: &'static str,
        one_based: bool,
        num_positions: u64,
    ) -> Self {
        let labels = (0..num_positions)
            .map(|i| label_for(label_prefix, one_based, num_positions, i))
            .collect();
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Name", PropertyValue::String(name.into()), true)
            .unwrap();
        props
            .define_property(
                "Description",
                PropertyValue::String(description.into()),
                true,
            )
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .define_property("Label", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        set_state_allowed_values(&mut props, num_positions);
        Self {
            name,
            description,
            prefix,
            label_prefix,
            one_based,
            props,
            transport: None,
            initialized: false,
            position: 0,
            num_positions,
            labels,
            gate_open: true,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        let mut last_timeout = None;
        for _ in 0..MAX_COMMAND_ATTEMPTS {
            self.call_transport(|t| t.purge())?;
            let attempt = self.call_transport(|t| {
                t.send(&full)?;
                Ok(t.receive_line()?.trim().to_string())
            });
            match attempt {
                Ok(response) => return Ok(response),
                Err(MmError::SerialTimeout) => last_timeout = Some(MmError::SerialTimeout),
                Err(err) => return Err(err),
            }
        }
        Err(last_timeout.unwrap_or(MmError::SerialTimeout))
    }

    /// Parse the integer that follows the command echo prefix in a response.
    fn parse_after_prefix(resp: &str, prefix: &str) -> Option<i64> {
        if resp.starts_with(prefix) {
            resp[prefix.len()..].parse::<i64>().ok()
        } else {
            None
        }
    }

    fn rebuild_labels(&mut self) {
        self.labels = (0..self.num_positions)
            .map(|i| label_for(self.label_prefix, self.one_based, self.num_positions, i))
            .collect();
        set_state_allowed_values(&mut self.props, self.num_positions);
    }
}

fn label_for(prefix: &str, one_based: bool, num_positions: u64, pos: u64) -> String {
    if prefix == "Motor" {
        if pos == 0 {
            "OFF".into()
        } else {
            "ON".into()
        }
    } else if prefix == "Spinning pos." {
        if pos == 0 {
            "Spinning pos. out".into()
        } else {
            format!("Spinning pos.{}", pos)
        }
    } else if prefix == "Slider pos." {
        if pos + 1 == num_positions {
            "Slider pos. out".into()
        } else {
            format!("Slider pos.{}", pos)
        }
    } else if one_based {
        format!("{}{}", prefix, pos + 1)
    } else {
        format!("{}{}", prefix, pos)
    }
}

fn set_state_allowed_values(props: &mut PropertyMap, num_positions: u64) {
    let allowed: Vec<String> = (0..num_positions).map(|i| i.to_string()).collect();
    let refs: Vec<&str> = allowed.iter().map(String::as_str).collect();
    let _ = props.set_allowed_values("State", &refs);
}

impl Device for XLightV3StateDevice {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        self.description
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Query number of positions
        let num_cmd = format!("r{}N", self.prefix);
        let num_resp = self.cmd(&num_cmd)?;
        let expected_prefix = format!("r{}N", self.prefix);
        let n = Self::parse_after_prefix(&num_resp, &expected_prefix)
            .ok_or(MmError::SerialInvalidResponse)?;
        if n < 0 {
            return Err(MmError::SerialInvalidResponse);
        }
        if n > 0 {
            self.num_positions = n as u64;
            self.rebuild_labels();
        }

        // Query current position
        let pos_cmd = format!("r{}", self.prefix);
        let pos_resp = self.cmd(&pos_cmd)?;
        let expected_prefix2 = format!("r{}", self.prefix);
        let wire_pos = Self::parse_after_prefix(&pos_resp, &expected_prefix2)
            .ok_or(MmError::SerialInvalidResponse)?;
        if wire_pos < 0 {
            return Err(MmError::SerialInvalidResponse);
        }
        self.position = if self.one_based {
            if wire_pos == 0 {
                return Err(MmError::SerialInvalidResponse);
            }
            wire_pos as u64 - 1
        } else {
            wire_pos as u64
        };
        if let Some(entry) = self.props.entry_mut("State") {
            entry.value = PropertyValue::Integer(self.position as i64);
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
            "State" => Ok(PropertyValue::Integer(self.position as i64)),
            "Label" => Ok(PropertyValue::String(
                self.labels
                    .get(self.position as usize)
                    .cloned()
                    .unwrap_or_default(),
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if pos < 0 {
                    return Err(MmError::UnknownPosition);
                }
                self.set_position(pos as u64)
            }
            "Label" => {
                let label = val.as_str().to_string();
                self.set_position_by_label(&label)
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
        DeviceType::State
    }
    fn busy(&self) -> bool {
        false
    }
}

impl StateDevice for XLightV3StateDevice {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized {
            let wire = if self.one_based { pos + 1 } else { pos };
            let cmd = format!("{}{}", self.prefix, wire);
            let resp = self.cmd(&cmd)?;
            let echoed = Self::parse_after_prefix(&resp, self.prefix)
                .ok_or_else(|| MmError::LocallyDefined("XLight V3 command echo mismatch".into()))?;
            if echoed < 0 || echoed as u64 != wire {
                return Err(MmError::LocallyDefined(
                    "XLight V3 command echo mismatch".into(),
                ));
            }
        }
        self.position = pos;
        if let Some(entry) = self.props.entry_mut("State") {
            entry.value = PropertyValue::Integer(pos as i64);
        }
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        Ok(self.position)
    }
    fn get_number_of_positions(&self) -> u64 {
        self.num_positions
    }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels
            .get(pos as usize)
            .cloned()
            .ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self
            .labels
            .iter()
            .position(|l| l == label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64;
        self.set_position(pos)
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        self.labels[pos as usize] = label.to_string();
        Ok(())
    }

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
        Ok(())
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(self.gate_open)
    }
}

// --- Public type aliases ---

pub struct XLightV3EmissionWheel(XLightV3StateDevice);
impl XLightV3EmissionWheel {
    pub fn new() -> Self {
        Self(XLightV3StateDevice::new(
            "Emission wheel",
            "Emission filter wheel",
            "B",
            "Emission pos.",
            true,
            8,
        ))
    }
    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.0 = self.0.with_transport(t);
        self
    }
}
impl Default for XLightV3EmissionWheel {
    fn default() -> Self {
        Self::new()
    }
}
impl Device for XLightV3EmissionWheel {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn description(&self) -> &str {
        self.0.description()
    }
    fn initialize(&mut self) -> MmResult<()> {
        self.0.initialize()
    }
    fn shutdown(&mut self) -> MmResult<()> {
        self.0.shutdown()
    }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.0.get_property(name)
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        self.0.set_property(name, val)
    }
    fn property_names(&self) -> Vec<String> {
        self.0.property_names()
    }
    fn has_property(&self, name: &str) -> bool {
        self.0.has_property(name)
    }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.0.is_property_read_only(name)
    }
    fn device_type(&self) -> DeviceType {
        self.0.device_type()
    }
    fn busy(&self) -> bool {
        self.0.busy()
    }
}
impl StateDevice for XLightV3EmissionWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        self.0.set_position(pos)
    }
    fn get_position(&self) -> MmResult<u64> {
        self.0.get_position()
    }
    fn get_number_of_positions(&self) -> u64 {
        self.0.get_number_of_positions()
    }
    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.0.get_position_label(pos)
    }
    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        self.0.set_position_by_label(label)
    }
    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        self.0.set_position_label(pos, label)
    }
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.0.set_gate_open(open)
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        self.0.get_gate_open()
    }
}

pub struct XLightV3DichroicWheel(XLightV3StateDevice);
impl XLightV3DichroicWheel {
    pub fn new() -> Self {
        Self(XLightV3StateDevice::new(
            "Dichroic wheel",
            "Dichroic filter wheel",
            "C",
            "Dichroic pos.",
            true,
            5,
        ))
    }
    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.0 = self.0.with_transport(t);
        self
    }
}
impl Default for XLightV3DichroicWheel {
    fn default() -> Self {
        Self::new()
    }
}
impl Device for XLightV3DichroicWheel {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn description(&self) -> &str {
        self.0.description()
    }
    fn initialize(&mut self) -> MmResult<()> {
        self.0.initialize()
    }
    fn shutdown(&mut self) -> MmResult<()> {
        self.0.shutdown()
    }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.0.get_property(name)
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        self.0.set_property(name, val)
    }
    fn property_names(&self) -> Vec<String> {
        self.0.property_names()
    }
    fn has_property(&self, name: &str) -> bool {
        self.0.has_property(name)
    }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.0.is_property_read_only(name)
    }
    fn device_type(&self) -> DeviceType {
        self.0.device_type()
    }
    fn busy(&self) -> bool {
        self.0.busy()
    }
}
impl StateDevice for XLightV3DichroicWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        self.0.set_position(pos)
    }
    fn get_position(&self) -> MmResult<u64> {
        self.0.get_position()
    }
    fn get_number_of_positions(&self) -> u64 {
        self.0.get_number_of_positions()
    }
    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.0.get_position_label(pos)
    }
    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        self.0.set_position_by_label(label)
    }
    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        self.0.set_position_label(pos, label)
    }
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.0.set_gate_open(open)
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        self.0.get_gate_open()
    }
}

pub struct XLightV3ExcitationWheel(XLightV3StateDevice);
impl XLightV3ExcitationWheel {
    pub fn new() -> Self {
        Self(XLightV3StateDevice::new(
            "Excitation wheel",
            "Excitation filter wheel",
            "A",
            "Excitation pos.",
            true,
            8,
        ))
    }
    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.0 = self.0.with_transport(t);
        self
    }
}
impl Default for XLightV3ExcitationWheel {
    fn default() -> Self {
        Self::new()
    }
}
impl Device for XLightV3ExcitationWheel {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn description(&self) -> &str {
        self.0.description()
    }
    fn initialize(&mut self) -> MmResult<()> {
        self.0.initialize()
    }
    fn shutdown(&mut self) -> MmResult<()> {
        self.0.shutdown()
    }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.0.get_property(name)
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        self.0.set_property(name, val)
    }
    fn property_names(&self) -> Vec<String> {
        self.0.property_names()
    }
    fn has_property(&self, name: &str) -> bool {
        self.0.has_property(name)
    }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.0.is_property_read_only(name)
    }
    fn device_type(&self) -> DeviceType {
        self.0.device_type()
    }
    fn busy(&self) -> bool {
        self.0.busy()
    }
}
impl StateDevice for XLightV3ExcitationWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        self.0.set_position(pos)
    }
    fn get_position(&self) -> MmResult<u64> {
        self.0.get_position()
    }
    fn get_number_of_positions(&self) -> u64 {
        self.0.get_number_of_positions()
    }
    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.0.get_position_label(pos)
    }
    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        self.0.set_position_by_label(label)
    }
    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        self.0.set_position_label(pos, label)
    }
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.0.set_gate_open(open)
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        self.0.get_gate_open()
    }
}

pub struct XLightV3SpinningSlider(XLightV3StateDevice);
impl XLightV3SpinningSlider {
    pub fn new() -> Self {
        Self(XLightV3StateDevice::new(
            "Spinning slider",
            "Spinning slider",
            "D",
            "Spinning pos.",
            false,
            3,
        ))
    }
    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.0 = self.0.with_transport(t);
        self
    }
}
impl Default for XLightV3SpinningSlider {
    fn default() -> Self {
        Self::new()
    }
}
impl Device for XLightV3SpinningSlider {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn description(&self) -> &str {
        self.0.description()
    }
    fn initialize(&mut self) -> MmResult<()> {
        self.0.initialize()
    }
    fn shutdown(&mut self) -> MmResult<()> {
        self.0.shutdown()
    }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.0.get_property(name)
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        self.0.set_property(name, val)
    }
    fn property_names(&self) -> Vec<String> {
        self.0.property_names()
    }
    fn has_property(&self, name: &str) -> bool {
        self.0.has_property(name)
    }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.0.is_property_read_only(name)
    }
    fn device_type(&self) -> DeviceType {
        self.0.device_type()
    }
    fn busy(&self) -> bool {
        self.0.busy()
    }
}
impl StateDevice for XLightV3SpinningSlider {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        self.0.set_position(pos)
    }
    fn get_position(&self) -> MmResult<u64> {
        self.0.get_position()
    }
    fn get_number_of_positions(&self) -> u64 {
        self.0.get_number_of_positions()
    }
    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.0.get_position_label(pos)
    }
    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        self.0.set_position_by_label(label)
    }
    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        self.0.set_position_label(pos, label)
    }
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.0.set_gate_open(open)
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        self.0.get_gate_open()
    }
}

pub struct XLightV3CameraSlider(XLightV3StateDevice);
impl XLightV3CameraSlider {
    pub fn new() -> Self {
        Self(XLightV3StateDevice::new(
            "Camera slider",
            "Dual camera slider",
            "P",
            "Slider pos.",
            false,
            2,
        ))
    }
    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.0 = self.0.with_transport(t);
        self
    }
}
impl Default for XLightV3CameraSlider {
    fn default() -> Self {
        Self::new()
    }
}
impl Device for XLightV3CameraSlider {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn description(&self) -> &str {
        self.0.description()
    }
    fn initialize(&mut self) -> MmResult<()> {
        self.0.initialize()
    }
    fn shutdown(&mut self) -> MmResult<()> {
        self.0.shutdown()
    }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.0.get_property(name)
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        self.0.set_property(name, val)
    }
    fn property_names(&self) -> Vec<String> {
        self.0.property_names()
    }
    fn has_property(&self, name: &str) -> bool {
        self.0.has_property(name)
    }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.0.is_property_read_only(name)
    }
    fn device_type(&self) -> DeviceType {
        self.0.device_type()
    }
    fn busy(&self) -> bool {
        self.0.busy()
    }
}
impl StateDevice for XLightV3CameraSlider {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        self.0.set_position(pos)
    }
    fn get_position(&self) -> MmResult<u64> {
        self.0.get_position()
    }
    fn get_number_of_positions(&self) -> u64 {
        self.0.get_number_of_positions()
    }
    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.0.get_position_label(pos)
    }
    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        self.0.set_position_by_label(label)
    }
    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        self.0.set_position_label(pos, label)
    }
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.0.set_gate_open(open)
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        self.0.get_gate_open()
    }
}

pub struct XLightV3SpinningMotor(XLightV3StateDevice);
impl XLightV3SpinningMotor {
    pub fn new() -> Self {
        Self(XLightV3StateDevice::new(
            "Spinning motor",
            "Spinning motor",
            "N",
            "Motor",
            false,
            2,
        ))
    }
    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.0 = self.0.with_transport(t);
        self
    }
}
impl Default for XLightV3SpinningMotor {
    fn default() -> Self {
        Self::new()
    }
}
impl Device for XLightV3SpinningMotor {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn description(&self) -> &str {
        self.0.description()
    }
    fn initialize(&mut self) -> MmResult<()> {
        self.0.initialize()
    }
    fn shutdown(&mut self) -> MmResult<()> {
        self.0.shutdown()
    }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.0.get_property(name)
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        self.0.set_property(name, val)
    }
    fn property_names(&self) -> Vec<String> {
        self.0.property_names()
    }
    fn has_property(&self, name: &str) -> bool {
        self.0.has_property(name)
    }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.0.is_property_read_only(name)
    }
    fn device_type(&self) -> DeviceType {
        self.0.device_type()
    }
    fn busy(&self) -> bool {
        self.0.busy()
    }
}
impl StateDevice for XLightV3SpinningMotor {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        self.0.set_position(pos)
    }
    fn get_position(&self) -> MmResult<u64> {
        self.0.get_position()
    }
    fn get_number_of_positions(&self) -> u64 {
        self.0.get_number_of_positions()
    }
    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.0.get_position_label(pos)
    }
    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        self.0.set_position_by_label(label)
    }
    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        self.0.set_position_label(pos, label)
    }
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.0.set_gate_open(open)
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        self.0.get_gate_open()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn emission_wheel_initialize() {
        // rBN → rBN8 (8 positions); rB → rB3 (position 3, 1-based = MM 2)
        let t = MockTransport::new()
            .expect("rBN\r", "rBN8")
            .expect("rB\r", "rB3");
        let mut d = XLightV3EmissionWheel::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 2);
        assert_eq!(d.get_number_of_positions(), 8);
    }

    #[test]
    fn emission_wheel_set_position() {
        let t = MockTransport::new()
            .expect("rBN\r", "rBN8")
            .expect("rB\r", "rB1")
            .expect("B5\r", "B5"); // MM pos 4 → wire 5
        let mut d = XLightV3EmissionWheel::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert!(!d.busy());
        d.set_position(4).unwrap();
        assert!(!d.busy());
        assert_eq!(d.get_position().unwrap(), 4);
    }

    #[test]
    fn spinning_slider_0based() {
        let t = MockTransport::new()
            .expect("rDN\r", "rDN3")
            .expect("rD\r", "rD1")
            .expect("D2\r", "D2");
        let mut d = XLightV3SpinningSlider::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 1); // 0-based
        d.set_position(2).unwrap();
        assert_eq!(d.get_position().unwrap(), 2);
    }

    #[test]
    fn dichroic_out_of_range() {
        let t = MockTransport::new()
            .expect("rCN\r", "rCN5")
            .expect("rC\r", "rC1");
        let mut d = XLightV3DichroicWheel::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert!(d.set_position(5).is_err());
    }

    #[test]
    fn negative_state_property_is_rejected_without_wraparound() {
        let t = MockTransport::new()
            .expect("rCN\r", "rCN5")
            .expect("rC\r", "rC1");
        let mut d = XLightV3DichroicWheel::new().with_transport(Box::new(t));
        d.initialize().unwrap();

        assert_eq!(
            d.set_property("State", PropertyValue::Integer(-1))
                .unwrap_err(),
            MmError::UnknownPosition
        );
        assert_eq!(d.get_position().unwrap(), 0);
    }

    #[test]
    fn set_position_rejects_echoed_wrong_value() {
        let t = MockTransport::new()
            .expect("rBN\r", "rBN8")
            .expect("rB\r", "rB1")
            .expect("B5\r", "B4");
        let mut d = XLightV3EmissionWheel::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert!(d.set_position(4).is_err());
        assert_eq!(d.get_position().unwrap(), 0);
    }

    #[test]
    fn initialize_rejects_wrong_query_echo() {
        let t = MockTransport::new().expect("rBN\r", "rCN8");
        let mut d = XLightV3EmissionWheel::new().with_transport(Box::new(t));
        assert_eq!(d.initialize(), Err(MmError::SerialInvalidResponse));
    }

    #[test]
    fn initialize_rejects_negative_position_response() {
        let t = MockTransport::new()
            .expect("rBN\r", "rBN8")
            .expect("rB\r", "rB-1");
        let mut d = XLightV3EmissionWheel::new().with_transport(Box::new(t));
        assert_eq!(d.initialize(), Err(MmError::SerialInvalidResponse));
    }

    #[test]
    fn initialize_rejects_zero_position_response_for_one_based_wheel() {
        let t = MockTransport::new()
            .expect("rBN\r", "rBN8")
            .expect("rB\r", "rB0");
        let mut d = XLightV3EmissionWheel::new().with_transport(Box::new(t));
        assert_eq!(d.initialize(), Err(MmError::SerialInvalidResponse));
    }

    #[test]
    fn no_transport_error() {
        assert!(XLightV3EmissionWheel::new().initialize().is_err());
    }
}
