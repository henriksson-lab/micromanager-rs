/// TriggerScope Focus stage.
///
/// The legacy adapter maps focus position to the controller's DAC scale and
/// writes it with `FOCUS,<count>`.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, DeviceChildMetadata, ParentHandle, Stage};
use crate::transport::Transport;
use crate::types::{DeviceType, FocusDirection, PropertyValue};

use super::hub::SharedTriggerScopeTransport;

pub struct TriggerScopeFocus {
    props: PropertyMap,
    transport: Option<SharedTriggerScopeTransport>,
    initialized: bool,
    position_um: f64,
    lower_limit: f64,
    upper_limit: f64,
    sequence_on: bool,
    sequence: Vec<f64>,
    is_ts16: bool,
    parent_label: Option<String>,
}

impl TriggerScopeFocus {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Upper Limit", PropertyValue::Float(1000.0), false)
            .unwrap();
        props
            .define_property("Lower Limit", PropertyValue::Float(0.0), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            position_um: 0.0,
            lower_limit: 0.0,
            upper_limit: 1000.0,
            sequence_on: true,
            sequence: Vec::new(),
            is_ts16: false,
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

    pub fn set_ts16(&mut self, is_ts16: bool) {
        self.is_ts16 = is_ts16;
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
        if !self.props.has_property("Position") {
            self.props
                .define_property("Position", PropertyValue::Float(0.0), false)?;
            self.props
                .set_property_limits("Position", self.lower_limit, self.upper_limit)?;
        }
        if !self.props.has_property("Sequence") {
            self.props
                .define_property("Sequence", PropertyValue::String("On".into()), false)?;
            self.props.set_allowed_values("Sequence", &["On", "Off"])?;
        }
        if !self.props.has_property("DAC Number") {
            self.props
                .define_property("DAC Number", PropertyValue::Integer(16), true)?;
        }
        if !self.props.has_property("Z Upper Limit") {
            self.props.define_property(
                "Z Upper Limit",
                PropertyValue::Float(self.upper_limit),
                true,
            )?;
        }
        if !self.props.has_property("Z Lower Limit") {
            self.props.define_property(
                "Z Lower Limit",
                PropertyValue::Float(self.lower_limit),
                true,
            )?;
        }
        Ok(())
    }

    fn position_to_count(&self, pos: f64) -> u32 {
        let span = self.upper_limit - self.lower_limit;
        let normalized = if span > 0.0 {
            ((pos - self.lower_limit) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let max_count = if self.is_ts16 { 65535.0 } else { 4095.0 };
        (normalized * max_count) as u32
    }

    fn write_to_port(&mut self, value: u32) -> MmResult<()> {
        let cmd = format!("FOCUS,{}\n", value);
        let _ = self.send_recv(&cmd)?;
        Ok(())
    }

    pub fn clear_stage_sequence(&mut self) -> MmResult<()> {
        self.sequence.clear();
        Ok(())
    }

    pub fn add_to_stage_sequence(&mut self, position: f64) -> MmResult<()> {
        if position < self.lower_limit || position > self.upper_limit {
            return Err(MmError::UnknownPosition);
        }
        self.sequence.push(position);
        Ok(())
    }

    pub fn send_stage_sequence(&mut self) -> MmResult<()> {
        if self.sequence.is_empty() {
            return Ok(());
        }

        let mut transitions = 1usize;
        for pair in self.sequence.windows(2) {
            if pair[0] != pair[1] {
                transitions += 1;
            }
        }

        let start = self.sequence[0];
        let end = *self.sequence.last().unwrap();
        let (step, direction) = if transitions == 1 {
            (0.0, 1)
        } else if end > start {
            ((end - start) / (transitions as f64 - 1.0), 1)
        } else {
            ((start - end) / (transitions as f64 - 1.0), 0)
        };

        let span = self.upper_limit - self.lower_limit;
        let step_count = if span > 0.0 {
            ((step / span).clamp(0.0, 1.0) * 65535.0) as u32
        } else {
            0
        };
        let start_count = self.position_to_count(start);

        self.send_recv("CLEAR_FOCUS\n")?;
        self.send_recv(&format!(
            "PROG_FOCUS,{},{},{},{},0\n",
            start_count, step_count, transitions, direction
        ))?;
        Ok(())
    }

    pub fn start_stage_sequence(&mut self) -> MmResult<()> {
        self.send_recv("ARM\n")?;
        Ok(())
    }

    pub fn stop_stage_sequence(&mut self) -> MmResult<()> {
        Ok(())
    }

    fn update_position_limits(&mut self) -> MmResult<()> {
        if let Some(entry) = self.props.entry_mut("Position") {
            entry.lower_limit = self.lower_limit;
            entry.upper_limit = self.upper_limit;
        }
        Ok(())
    }
}

impl Default for TriggerScopeFocus {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for TriggerScopeFocus {
    fn name(&self) -> &str {
        "TriggerScope-Focus"
    }

    fn description(&self) -> &str {
        "ARC TriggerScope Focus stage"
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
            "Position" if self.props.has_property("Position") => {
                Ok(PropertyValue::Float(self.position_um))
            }
            "Sequence" if self.props.has_property("Sequence") => Ok(PropertyValue::String(
                if self.sequence_on { "On" } else { "Off" }.into(),
            )),
            "Upper Limit" | "Z Upper Limit" => Ok(PropertyValue::Float(self.upper_limit)),
            "Lower Limit" | "Z Lower Limit" => Ok(PropertyValue::Float(self.lower_limit)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Position" if self.props.has_property("Position") => {
                let pos = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_position_um(pos)
            }
            "Sequence" if self.props.has_property("Sequence") => {
                let label = val.to_string();
                self.sequence_on = match label.as_str() {
                    "On" => true,
                    "Off" => false,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.props.set(name, PropertyValue::String(label))
            }
            "Upper Limit" if self.props.has_property("Upper Limit") => {
                self.upper_limit = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.update_position_limits()?;
                self.props.set(name, PropertyValue::Float(self.upper_limit))
            }
            "Lower Limit" if self.props.has_property("Lower Limit") => {
                self.lower_limit = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.update_position_limits()?;
                self.props.set(name, PropertyValue::Float(self.lower_limit))
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
        DeviceType::Stage
    }

    fn busy(&self) -> bool {
        false
    }

    fn bind_parent(&mut self, parent: ParentHandle, metadata: DeviceChildMetadata) -> MmResult<()> {
        match metadata {
            DeviceChildMetadata::None => {
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

impl Stage for TriggerScopeFocus {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        if pos < self.lower_limit || pos > self.upper_limit {
            return Err(MmError::UnknownPosition);
        }
        self.position_um = pos;
        if self.initialized {
            self.write_to_port(self.position_to_count(pos))?;
        }
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        Ok(self.position_um)
    }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        self.set_position_um(self.position_um + d)
    }

    fn home(&mut self) -> MmResult<()> {
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((self.lower_limit, self.upper_limit))
    }

    fn get_focus_direction(&self) -> FocusDirection {
        FocusDirection::Unknown
    }

    fn is_continuous_focus_drive(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn focus_scales_position_to_ts12_counts() {
        let t = MockTransport::new().expect("FOCUS,2047\n", "FOCUS,2047");
        let mut focus = TriggerScopeFocus::new().with_transport(Box::new(t));
        focus.initialize().unwrap();
        focus.set_position_um(500.0).unwrap();
        assert_eq!(focus.get_position_um().unwrap(), 500.0);
    }

    #[test]
    fn focus_scales_position_to_ts16_counts() {
        let t = MockTransport::new().expect("FOCUS,32767\n", "FOCUS,32767");
        let mut focus = TriggerScopeFocus::new().with_transport(Box::new(t));
        focus.set_ts16(true);
        focus.initialize().unwrap();
        focus.set_position_um(500.0).unwrap();
    }

    #[test]
    fn focus_sequence_uses_upstream_clear_prog_and_arm() {
        let t = MockTransport::new()
            .expect("CLEAR_FOCUS\n", "CLEAR_FOCUS")
            .expect("PROG_FOCUS,0,32767,3,1,0\n", "PROG_FOCUS")
            .expect("ARM\n", "ARM");
        let mut focus = TriggerScopeFocus::new().with_transport(Box::new(t));
        focus.set_ts16(true);
        focus.initialize().unwrap();
        focus.add_to_stage_sequence(0.0).unwrap();
        focus.add_to_stage_sequence(500.0).unwrap();
        focus.add_to_stage_sequence(1000.0).unwrap();
        focus.send_stage_sequence().unwrap();
        focus.start_stage_sequence().unwrap();
    }
}
