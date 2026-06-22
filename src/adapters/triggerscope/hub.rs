/// TriggerScope Hub.
///
/// ASCII serial protocol, `\n` terminated.
///   Identify: send `"*\n"`, recv firmware banner like `"ARC TRIGGERSCOPE 16 v1.2\n"`
///   Status:   send `"STAT?\n"`, recv status string
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Hub};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::sync::{Arc, Mutex};

const SOFTWARE_VERSION: &str = "v1.6.5, 8/16/16";

pub type SharedTriggerScopeTransport = Arc<Mutex<Box<dyn Transport>>>;

pub struct TriggerScopeHub {
    props: PropertyMap,
    transport: Option<SharedTriggerScopeTransport>,
    initialized: bool,
    firmware_version: String,
    is_ts16: bool,
    serial_tx: String,
    serial_rx: String,
    program_file: String,
    program_load: i64,
    step_mode: i64,
    arm_mode: i64,
    array_num: i64,
    clear_arrays: String,
}

impl TriggerScopeHub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            firmware_version: String::new(),
            is_ts16: false,
            serial_tx: String::new(),
            serial_rx: String::new(),
            program_file: "TriggerScope.csv".into(),
            program_load: 0,
            step_mode: 0,
            arm_mode: 0,
            array_num: 1,
            clear_arrays: "Off".into(),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(Arc::new(Mutex::new(t)));
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

    pub fn shared_transport(&self) -> Option<SharedTriggerScopeTransport> {
        self.transport.clone()
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
        let port = self
            .props
            .get("Port")
            .map(|value| value.to_string())
            .unwrap_or_default();
        if !self.props.has_property("Firmware Version") {
            self.props.define_property(
                "Firmware Version",
                PropertyValue::String(self.firmware_version.clone()),
                true,
            )?;
        }
        if !self.props.has_property("Software Version") {
            self.props.define_property(
                "Software Version",
                PropertyValue::String(SOFTWARE_VERSION.into()),
                true,
            )?;
        }
        if !self.props.has_property("DAC Bits") {
            self.props.define_property(
                "DAC Bits",
                PropertyValue::String(if self.is_ts16 { "16" } else { "12" }.into()),
                true,
            )?;
        }
        if !self.props.has_property("COM Port") {
            self.props
                .define_property("COM Port", PropertyValue::String(port), true)?;
        }
        if !self.props.has_property("Trigger Time Delta") {
            self.props.define_property(
                "Trigger Time Delta",
                PropertyValue::String(String::new()),
                true,
            )?;
        }
        if !self.props.has_property("Serial TX") {
            self.props
                .define_property("Serial TX", PropertyValue::String(String::new()), false)?;
        }
        if !self.props.has_property("Serial RX") {
            self.props
                .define_property("Serial RX", PropertyValue::String(String::new()), false)?;
        }
        if !self.props.has_property("Program File") {
            self.props.define_property(
                "Program File",
                PropertyValue::String("TriggerScope.csv".into()),
                false,
            )?;
        }
        if !self.props.has_property("Program Load") {
            self.props
                .define_property("Program Load", PropertyValue::Integer(0), false)?;
            self.props.set_property_limits("Program Load", 0.0, 1.0)?;
        }
        if !self.props.has_property("Step Mode") {
            self.props
                .define_property("Step Mode", PropertyValue::Integer(0), false)?;
        }
        if !self.props.has_property("Arm Mode") {
            self.props
                .define_property("Arm Mode", PropertyValue::Integer(0), false)?;
            self.props.set_property_limits("Arm Mode", 0.0, 1.0)?;
        }
        if !self.props.has_property("Array #") {
            self.props
                .define_property("Array #", PropertyValue::Integer(1), false)?;
            self.props.set_property_limits("Array #", 1.0, 6.0)?;
        }
        if !self.props.has_property("Clear Arrays") {
            self.props.define_property(
                "Clear Arrays",
                PropertyValue::String("Off".into()),
                false,
            )?;
            self.props.set_allowed_values(
                "Clear Arrays",
                &["Off", "Clear Active Array", "Clear All Arrays"],
            )?;
        }
        Ok(())
    }

    pub fn firmware_version(&self) -> &str {
        &self.firmware_version
    }
    pub fn is_ts16(&self) -> bool {
        self.is_ts16
    }
}

impl Default for TriggerScopeHub {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for TriggerScopeHub {
    fn name(&self) -> &str {
        "TriggerScope-Hub"
    }
    fn description(&self) -> &str {
        "ARC TriggerScope hub"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.initialized {
            return Ok(());
        }
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let mut banner = String::new();
        for _ in 0..10 {
            banner = self.send_recv("*\n")?;
            if banner.contains("ERROR_UNKNOWN_COMMAND") {
                banner = self.send_recv("*\n")?;
            }
            if banner.contains("ARC TRIGGERSCOPE") || banner.contains("ARC_LED") {
                break;
            }
        }
        if !banner.contains("ARC TRIGGERSCOPE") && !banner.contains("ARC_LED") {
            return Err(MmError::SerialInvalidResponse);
        }
        self.is_ts16 = banner.contains("ARC TRIGGERSCOPE 16") || banner.contains("ARC_LED 16");
        self.firmware_version = banner.clone();
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
            "FirmwareVersion" => Ok(PropertyValue::String(self.firmware_version.clone())),
            "Firmware Version" if self.props.has_property("Firmware Version") => {
                Ok(PropertyValue::String(self.firmware_version.clone()))
            }
            "Software Version" if self.props.has_property("Software Version") => {
                Ok(PropertyValue::String(SOFTWARE_VERSION.into()))
            }
            "DACBits" => Ok(PropertyValue::Integer(if self.is_ts16 { 16 } else { 12 })),
            "DAC Bits" if self.props.has_property("DAC Bits") => Ok(PropertyValue::String(
                if self.is_ts16 { "16" } else { "12" }.into(),
            )),
            "Serial TX" if self.props.has_property("Serial TX") => {
                Ok(PropertyValue::String(self.serial_tx.clone()))
            }
            "Serial RX" if self.props.has_property("Serial RX") => {
                Ok(PropertyValue::String(self.serial_rx.clone()))
            }
            "Program File" if self.props.has_property("Program File") => {
                Ok(PropertyValue::String(self.program_file.clone()))
            }
            "Program Load" if self.props.has_property("Program Load") => {
                Ok(PropertyValue::Integer(self.program_load))
            }
            "Step Mode" if self.props.has_property("Step Mode") => {
                Ok(PropertyValue::Integer(self.step_mode))
            }
            "Arm Mode" if self.props.has_property("Arm Mode") => {
                Ok(PropertyValue::Integer(self.arm_mode))
            }
            "Array #" if self.props.has_property("Array #") => {
                Ok(PropertyValue::Integer(self.array_num))
            }
            "Clear Arrays" if self.props.has_property("Clear Arrays") => {
                Ok(PropertyValue::String(self.clear_arrays.clone()))
            }
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Serial TX" if self.props.has_property("Serial TX") => {
                let cmd = val.to_string();
                self.serial_rx = self.send_recv(&format!("{}\n", cmd))?;
                self.serial_tx = cmd.clone();
                self.props.set(name, PropertyValue::String(cmd))
            }
            "Program Load" if self.props.has_property("Program Load") => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.program_load = v;
                self.props.set(name, PropertyValue::Integer(v))
            }
            "Step Mode" if self.props.has_property("Step Mode") => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.step_mode = v;
                self.props.set(name, PropertyValue::Integer(v))
            }
            "Arm Mode" if self.props.has_property("Arm Mode") => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.arm_mode = v;
                self.props.set(name, PropertyValue::Integer(v))
            }
            "Array #" if self.props.has_property("Array #") => {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.array_num = v;
                self.props.set(name, PropertyValue::Integer(v))
            }
            "Program File" if self.props.has_property("Program File") => {
                self.program_file = val.to_string();
                self.props
                    .set(name, PropertyValue::String(self.program_file.clone()))
            }
            "Clear Arrays" if self.props.has_property("Clear Arrays") => {
                let clear = val.to_string();
                match clear.as_str() {
                    "Clear Active Array" => {
                        self.send_recv(&format!("CLEAR_ARRAY,{}\n", self.array_num))?;
                    }
                    "Clear All Arrays" => {
                        self.send_recv("CLEAR_ALL\n")?;
                    }
                    "Off" => {}
                    _ => return Err(MmError::InvalidPropertyValue),
                }
                self.clear_arrays = clear.clone();
                self.props.set(name, PropertyValue::String(clear))
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

impl Hub for TriggerScopeHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        let mut devs = Vec::new();
        devs.push("TriggerScope-CAM1".to_string());
        devs.push("TriggerScope-CAM2".to_string());
        for i in 1..=16 {
            devs.push(format!("TriggerScope-DAC{:02}", i));
        }
        for i in 1..=16 {
            devs.push(format!("TriggerScope-TTL{:02}", i));
        }
        devs.push("TriggerScope-Focus".to_string());
        Ok(devs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::triggerscope::{TriggerScopeDAC, TriggerScopeTTL};
    use crate::traits::{SignalIO, StateDevice};
    use crate::transport::MockTransport;

    #[test]
    fn initialize_ts16() {
        let t = MockTransport::new().expect("*\n", "ARC TRIGGERSCOPE 16 v1.65");
        let mut hub = TriggerScopeHub::new().with_transport(Box::new(t));
        assert_eq!(hub.property_names(), vec!["Port".to_string()]);
        hub.initialize().unwrap();
        assert!(hub.is_ts16());
        assert!(hub.firmware_version().contains("v1.65"));
        assert!(hub.has_property("Serial TX"));
        assert!(hub.has_property("Trigger Time Delta"));
    }

    #[test]
    fn initialize_ts12() {
        let t = MockTransport::new().expect("*\n", "ARC TRIGGERSCOPE v1.50");
        let mut hub = TriggerScopeHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert!(!hub.is_ts16());
    }

    #[test]
    fn invalid_banner_rejected() {
        let t = MockTransport::new().expect("*\n", "UNKNOWN DEVICE v1.0");
        let mut hub = TriggerScopeHub::new().with_transport(Box::new(t));
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn no_transport_error() {
        let mut hub = TriggerScopeHub::new();
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn installed_devices_match_upstream_registry_order() {
        let mut hub = TriggerScopeHub::new();
        let devices = hub.detect_installed_devices().unwrap();
        assert_eq!(devices.first().unwrap(), "TriggerScope-CAM1");
        assert_eq!(devices[2], "TriggerScope-DAC01");
        assert_eq!(devices[18], "TriggerScope-TTL01");
        assert_eq!(devices.last().unwrap(), "TriggerScope-Focus");
        assert_eq!(devices.len(), 35);
    }

    #[test]
    fn hub_and_children_can_share_one_serial_owner() {
        let shared: SharedTriggerScopeTransport = Arc::new(Mutex::new(Box::new(
            MockTransport::new()
                .expect("*\n", "ARC TRIGGERSCOPE 16 v1.65")
                .expect("TTL3,1\n", "TTL3 OK")
                .expect("DAC2,32767\n", "DAC2 OK"),
        )));
        let mut hub = TriggerScopeHub::new().with_shared_transport(Arc::clone(&shared));
        let mut ttl = TriggerScopeTTL::new(3).with_shared_transport(Arc::clone(&shared));
        let mut dac = TriggerScopeDAC::new(2).with_shared_transport(shared);

        hub.initialize().unwrap();
        ttl.initialize().unwrap();
        dac.initialize().unwrap();
        dac.set_ts16(hub.is_ts16());

        ttl.set_position(1).unwrap();
        dac.set_signal(5.0).unwrap();

        assert!(hub.is_ts16());
        assert_eq!(ttl.get_position().unwrap(), 1);
        assert!((dac.get_signal().unwrap() - 5.0).abs() < 0.01);
    }
}
