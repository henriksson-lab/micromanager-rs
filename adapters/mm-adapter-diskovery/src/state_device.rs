/// Generic Diskovery state device implementation.
///
/// All preset-based devices share this structure.
/// Protocol:
///   Query:   `Q:<param>\r`       → `A:<param>,<value>\r`
///   Set:     `A:<param>,<n>\r`   → `A:<param>,<n>\r`  (echo confirms)
///
/// Positions are 1-based on the wire for presets; motor is 0/1.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct DiskoveryStateDevice {
    dev_name: &'static str,
    description: &'static str,
    query_param: &'static str,  // e.g. "PRESET_SD"
    set_param: &'static str,    // e.g. "PRESET_SD"
    one_based: bool,            // true for presets; false for motor (0/1)
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position: u64,
    num_positions: u64,
    labels: Vec<String>,
    gate_open: bool,
}

impl DiskoveryStateDevice {
    fn new(
        dev_name: &'static str,
        description: &'static str,
        param: &'static str,
        one_based: bool,
        num_positions: u64,
        label_prefix: &'static str,
    ) -> Self {
        let labels = (0..num_positions)
            .map(|i| format!("{}{}", label_prefix, i + 1))
            .collect();
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            dev_name, description,
            query_param: param,
            set_param: param,
            one_based,
            props, transport: None, initialized: false,
            position: 0, num_positions, labels, gate_open: true,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where F: FnOnce(&mut dyn Transport) -> MmResult<R> {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }

    /// Parse `A:<param>,<value>` response, returning value as i64.
    fn parse_response(resp: &str, param: &str) -> Option<i64> {
        let expected = format!("A:{},", param);
        if resp.starts_with(&expected) {
            resp[expected.len()..].parse::<i64>().ok()
        } else {
            None
        }
    }
}

impl Device for DiskoveryStateDevice {
    fn name(&self) -> &str { self.dev_name }
    fn description(&self) -> &str { self.description }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let q = format!("Q:{}", self.query_param);
        let resp = self.cmd(&q)?;
        if let Some(wire_val) = Self::parse_response(&resp, self.query_param) {
            self.position = if self.one_based {
                (wire_val as u64).saturating_sub(1)
            } else {
                wire_val as u64
            };
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::Integer(self.position as i64)),
            "Label" => Ok(PropertyValue::String(
                self.labels.get(self.position as usize).cloned().unwrap_or_default()
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u64;
                self.set_position(pos)
            }
            "Label" => {
                let label = val.as_str().to_string();
                self.set_position_by_label(&label)
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::State }
    fn busy(&self) -> bool { false }
}

impl StateDevice for DiskoveryStateDevice {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions { return Err(MmError::UnknownPosition); }
        if self.initialized {
            let wire = if self.one_based { pos + 1 } else { pos };
            let cmd = format!("A:{},{}", self.set_param, wire);
            let resp = self.cmd(&cmd)?;
            // Confirm echo
            if Self::parse_response(&resp, self.set_param).is_none() {
                return Err(MmError::LocallyDefined(
                    format!("Diskovery set command echo mismatch: {}", resp)
                ));
            }
        }
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> { Ok(self.position) }
    fn get_number_of_positions(&self) -> u64 { self.num_positions }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels.get(pos as usize).cloned().ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self.labels.iter().position(|l| l == label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64;
        self.set_position(pos)
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        if pos >= self.num_positions { return Err(MmError::UnknownPosition); }
        self.labels[pos as usize] = label.to_string();
        Ok(())
    }

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> { self.gate_open = open; Ok(()) }
    fn get_gate_open(&self) -> MmResult<bool> { Ok(self.gate_open) }
}

// ---- Public newtype wrappers ----

macro_rules! diskovery_device {
    ($struct_name:ident, $dev_name:literal, $desc:literal, $param:literal, $one_based:expr, $num_pos:expr, $label_pfx:literal) => {
        pub struct $struct_name(DiskoveryStateDevice);

        impl $struct_name {
            pub fn new() -> Self {
                Self(DiskoveryStateDevice::new(
                    $dev_name, $desc, $param, $one_based, $num_pos, $label_pfx,
                ))
            }
            pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
                self.0 = self.0.with_transport(t); self
            }
        }

        impl Default for $struct_name { fn default() -> Self { Self::new() } }

        impl Device for $struct_name {
            fn name(&self) -> &str { self.0.name() }
            fn description(&self) -> &str { self.0.description() }
            fn initialize(&mut self) -> MmResult<()> { self.0.initialize() }
            fn shutdown(&mut self) -> MmResult<()> { self.0.shutdown() }
            fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.0.get_property(name) }
            fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> { self.0.set_property(name, val) }
            fn property_names(&self) -> Vec<String> { self.0.property_names() }
            fn has_property(&self, name: &str) -> bool { self.0.has_property(name) }
            fn is_property_read_only(&self, name: &str) -> bool { self.0.is_property_read_only(name) }
            fn device_type(&self) -> DeviceType { self.0.device_type() }
            fn busy(&self) -> bool { self.0.busy() }
        }

        impl StateDevice for $struct_name {
            fn set_position(&mut self, pos: u64) -> MmResult<()> { self.0.set_position(pos) }
            fn get_position(&self) -> MmResult<u64> { self.0.get_position() }
            fn get_number_of_positions(&self) -> u64 { self.0.get_number_of_positions() }
            fn get_position_label(&self, pos: u64) -> MmResult<String> { self.0.get_position_label(pos) }
            fn set_position_by_label(&mut self, label: &str) -> MmResult<()> { self.0.set_position_by_label(label) }
            fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> { self.0.set_position_label(pos, label) }
            fn set_gate_open(&mut self, open: bool) -> MmResult<()> { self.0.set_gate_open(open) }
            fn get_gate_open(&self) -> MmResult<bool> { self.0.get_gate_open() }
        }
    };
}

diskovery_device!(DiskoverySD,      "Diskovery-SD",      "Diskovery spinning disk position", "PRESET_SD",       true,  4, "Disk-");
diskovery_device!(DiskoveryWF,      "Diskovery-WF",      "Diskovery wide-field size",         "PRESET_WF",       true,  4, "WF-");
diskovery_device!(DiskoveryFilterW, "Diskovery-FilterW", "Diskovery filter wheel W",          "PRESET_FILTER_W", true,  4, "FilterW-");
diskovery_device!(DiskoveryFilterT, "Diskovery-FilterT", "Diskovery filter turret T",         "PRESET_FILTER_T", true,  4, "FilterT-");
diskovery_device!(DiskoveryIris,    "Diskovery-Iris",    "Diskovery objective selector/iris", "PRESET_IRIS",     true,  4, "Iris-");
diskovery_device!(DiskoveryMotor,   "Diskovery-Motor",   "Diskovery spinning disk motor",     "MOTOR_RUNNING_SD",false, 2, "Motor-");

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    #[test]
    fn sd_initialize() {
        // Query returns preset 2 (1-based) → MM position 1
        let t = MockTransport::new().expect("Q:PRESET_SD\r", "A:PRESET_SD,2");
        let mut d = DiskoverySD::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 1);
    }

    #[test]
    fn sd_set_position() {
        let t = MockTransport::new()
            .expect("Q:PRESET_SD\r", "A:PRESET_SD,1")
            .expect("A:PRESET_SD,3\r", "A:PRESET_SD,3"); // MM 2 → wire 3
        let mut d = DiskoverySD::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position(2).unwrap();
        assert_eq!(d.get_position().unwrap(), 2);
    }

    #[test]
    fn motor_0based() {
        let t = MockTransport::new()
            .expect("Q:MOTOR_RUNNING_SD\r", "A:MOTOR_RUNNING_SD,0")
            .expect("A:MOTOR_RUNNING_SD,1\r", "A:MOTOR_RUNNING_SD,1");
        let mut d = DiskoveryMotor::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 0);
        d.set_position(1).unwrap();
        assert_eq!(d.get_position().unwrap(), 1);
    }

    #[test]
    fn out_of_range_rejected() {
        let t = MockTransport::new().expect("Q:PRESET_SD\r", "A:PRESET_SD,1");
        let mut d = DiskoverySD::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        assert!(d.set_position(4).is_err()); // only 0..3 valid
    }

    #[test]
    fn no_transport_error() {
        assert!(DiskoverySD::new().initialize().is_err());
    }

    #[test]
    fn filterw_label() {
        let t = MockTransport::new()
            .expect("Q:PRESET_FILTER_W\r", "A:PRESET_FILTER_W,1")
            .expect("A:PRESET_FILTER_W,2\r", "A:PRESET_FILTER_W,2");
        let mut d = DiskoveryFilterW::new().with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position_label(1, "DAPI").unwrap();
        d.set_position_by_label("DAPI").unwrap();
        assert_eq!(d.get_position().unwrap(), 1);
    }
}
