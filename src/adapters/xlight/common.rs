use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::sync::{Arc, Mutex};

pub type SharedXLightTransport = Arc<Mutex<Box<dyn Transport>>>;

#[derive(Clone, Copy)]
pub struct XLightSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub query: &'static str,
    pub command: &'static str,
    pub num_positions: u64,
    pub one_based: bool,
    pub initial_position: u64,
    pub labels: &'static [&'static str],
}

pub struct XLightStateCore {
    spec: XLightSpec,
    props: PropertyMap,
    transport: Option<SharedXLightTransport>,
    initialized: bool,
    position: u64,
    labels: Vec<String>,
    gate_open: bool,
    busy: bool,
}

impl XLightStateCore {
    pub fn new(spec: XLightSpec) -> Self {
        let labels = spec.labels.iter().map(|label| label.to_string()).collect();
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Name", PropertyValue::String(spec.name.into()), true)
            .unwrap();
        props
            .define_property(
                "Description",
                PropertyValue::String(spec.description.into()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "State",
                PropertyValue::Integer(spec.initial_position as i64),
                false,
            )
            .unwrap();
        props
            .define_property("Label", PropertyValue::String("Undefined".into()), false)
            .unwrap();

        let allowed: Vec<String> = (0..spec.num_positions).map(|i| i.to_string()).collect();
        let allowed_refs: Vec<&str> = allowed.iter().map(String::as_str).collect();
        props.set_allowed_values("State", &allowed_refs).unwrap();

        Self {
            spec,
            props,
            transport: None,
            initialized: false,
            position: spec.initial_position,
            labels,
            gate_open: true,
            busy: false,
        }
    }

    pub fn with_transport(mut self, transport: Box<dyn Transport>) -> Self {
        self.transport = Some(Arc::new(Mutex::new(transport)));
        self
    }

    pub fn with_shared_transport(mut self, transport: SharedXLightTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn execute_command_ex(&mut self, command: &str) -> MmResult<String> {
        let transport = self.transport.as_ref().ok_or(MmError::NotConnected)?;
        let full = format!("{}\r", command);
        let mut transport = transport
            .lock()
            .map_err(|_| MmError::LocallyDefined("XLight transport lock poisoned".into()))?;
        transport.purge()?;
        self.busy = true;
        let attempt = (|| {
            transport.send(&full)?;
            let response = transport.receive_line()?.trim().to_string();
            if response.starts_with(command) {
                Ok(response)
            } else {
                Err(MmError::SerialInvalidResponse)
            }
        })();
        self.busy = false;

        attempt
    }

    pub fn initialize_with_command(
        &mut self,
        query: &'static str,
        command: &'static str,
    ) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let response = self.execute_command_ex(query)?;
        let position = self.parse_position(&response, query)?;
        self.spec.query = query;
        self.spec.command = command;
        self.position = position.min(self.spec.num_positions.saturating_sub(1));
        if let Some(entry) = self.props.entry_mut("State") {
            entry.value = PropertyValue::Integer(self.position as i64);
        }
        self.initialized = true;
        Ok(())
    }

    fn parse_position(&self, response: &str, query: &str) -> MmResult<u64> {
        let raw = response
            .strip_prefix(query)
            .ok_or(MmError::SerialInvalidResponse)?
            .parse::<u64>()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        if self.spec.one_based {
            raw.checked_sub(1).ok_or(MmError::SerialInvalidResponse)
        } else {
            Ok(raw)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn shared_transport_routes_serial_commands_in_order() {
        let shared: SharedXLightTransport = Arc::new(Mutex::new(Box::new(
            MockTransport::new()
                .expect("rC\r", "rC1")
                .expect("C2\r", "C2")
                .expect("rB\r", "rB3"),
        )));
        let mut dichroic = XLightStateCore::new(XLightSpec {
            name: "d",
            description: "d",
            query: "rC",
            command: "C",
            num_positions: 5,
            one_based: true,
            initial_position: 0,
            labels: &["0", "1", "2", "3", "4"],
        })
        .with_shared_transport(Arc::clone(&shared));
        let mut emission = XLightStateCore::new(XLightSpec {
            name: "e",
            description: "e",
            query: "rB",
            command: "B",
            num_positions: 8,
            one_based: true,
            initial_position: 0,
            labels: &["0", "1", "2", "3", "4", "5", "6", "7"],
        })
        .with_shared_transport(shared);

        dichroic.initialize().unwrap();
        dichroic.set_position(1).unwrap();
        emission.initialize().unwrap();

        assert_eq!(dichroic.get_position().unwrap(), 1);
        assert_eq!(emission.get_position().unwrap(), 2);
    }

    #[test]
    fn command_timeout_does_not_retry() {
        let mut device = XLightStateCore::new(XLightSpec {
            name: "d",
            description: "d",
            query: "rC",
            command: "C",
            num_positions: 5,
            one_based: true,
            initial_position: 0,
            labels: &["0", "1", "2", "3", "4"],
        })
        .with_transport(Box::new(MockTransport::new()));

        assert_eq!(device.execute_command_ex("C2"), Err(MmError::SerialTimeout));
    }
}

impl Device for XLightStateCore {
    fn name(&self) -> &str {
        self.spec.name
    }

    fn description(&self) -> &str {
        self.spec.description
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.initialize_with_command(self.spec.query, self.spec.command)
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
        self.busy
    }
}

impl StateDevice for XLightStateCore {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.spec.num_positions {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized && pos != self.position {
            let wire = if self.spec.one_based { pos + 1 } else { pos };
            let command = format!("{}{}", self.spec.command, wire);
            self.execute_command_ex(&command)?;
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
        self.spec.num_positions
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
        if pos >= self.spec.num_positions {
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
