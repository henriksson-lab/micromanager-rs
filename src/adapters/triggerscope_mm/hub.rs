/// TriggerScope MM Hub.
///
/// ASCII serial protocol, `\n` terminated, answers end with `\r\n`.
///   Identify: send `"*\n"`, recv banner like `"ARC TRIGGERSCOPE 16 vX.Y\r\n"`
///   Status:   send `"STAT?\n"`, recv status string
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Hub};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::sync::{Arc, Mutex};

const SOFTWARE_VERSION: &str = "v1.0-MM, 8/24/2020";

pub type SharedTriggerScopeMMTransport = Arc<Mutex<Box<dyn Transport>>>;

pub struct TriggerScopeMMHub {
    props: PropertyMap,
    transport: Option<SharedTriggerScopeMMTransport>,
    initialized: bool,
    firmware_version: String,
    is_ts16: bool,
    use_action_leds: bool,
    serial_send: String,
    serial_receive: String,
}

impl TriggerScopeMMHub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Serial Send", PropertyValue::String(String::new()), false)
            .unwrap();
        props
            .define_property("Serial Receive", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("UseActionLEDs", PropertyValue::String("On".into()), false)
            .unwrap();
        props
            .set_allowed_values("UseActionLEDs", &["On", "Off"])
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            firmware_version: String::new(),
            is_ts16: false,
            use_action_leds: true,
            serial_send: String::new(),
            serial_receive: String::new(),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(Arc::new(Mutex::new(t)));
        self
    }

    pub fn with_shared_transport(mut self, transport: SharedTriggerScopeMMTransport) -> Self {
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
                    MmError::LocallyDefined("TriggerScope MM transport lock poisoned".into())
                })?;
                f(guard.as_mut())
            }
            None => Err(MmError::NotConnected),
        }
    }

    fn send_recv(&mut self, cmd: &str) -> MmResult<String> {
        self.call_transport(|t| {
            t.purge()?;
            Ok(t.send_recv(cmd)?.trim().to_string())
        })
    }

    pub fn is_ts16(&self) -> bool {
        self.is_ts16
    }
    pub fn firmware_version(&self) -> &str {
        &self.firmware_version
    }

    pub fn shared_transport(&self) -> Option<SharedTriggerScopeMMTransport> {
        self.transport.clone()
    }

    pub fn create_ttl_child(&self, pin_group: u8) -> MmResult<super::ttl::TriggerScopeMMTTL> {
        let transport = self.shared_transport().ok_or(MmError::NotConnected)?;
        Ok(super::ttl::TriggerScopeMMTTL::new(pin_group).with_shared_transport(transport))
    }

    pub fn create_dac_child(&self, channel: u8) -> MmResult<super::dac::TriggerScopeMMDAC> {
        let transport = self.shared_transport().ok_or(MmError::NotConnected)?;
        let mut child =
            super::dac::TriggerScopeMMDAC::new(channel).with_shared_transport(transport);
        child.set_ts16(self.is_ts16);
        Ok(child)
    }

    /// Send a command and receive one-line response (used by sub-devices).
    pub fn send_and_receive(&mut self, cmd: &str) -> MmResult<String> {
        self.send_recv(&format!("{}\n", cmd))
    }
}

impl Default for TriggerScopeMMHub {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for TriggerScopeMMHub {
    fn name(&self) -> &str {
        "TriggerScopeMM-Hub"
    }
    fn description(&self) -> &str {
        "ARC TriggerScope MM hub"
    }

    fn initialize(&mut self) -> MmResult<()> {
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
        if !banner.ends_with("MM") {
            return Err(MmError::SerialInvalidResponse);
        }
        self.is_ts16 = banner.contains("ARC TRIGGERSCOPE 16") || banner.contains("ARC_LED 16");
        self.firmware_version = banner;
        if self.send_recv("SSL1\n").is_ok() {
            self.use_action_leds = true;
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
            "FirmwareVersion" => Ok(PropertyValue::String(self.firmware_version.clone())),
            "Firmware Version" => Ok(PropertyValue::String(self.firmware_version.clone())),
            "Software Version" => Ok(PropertyValue::String(SOFTWARE_VERSION.into())),
            "DACBits" => Ok(PropertyValue::Integer(if self.is_ts16 { 16 } else { 12 })),
            "DAC Bits" => Ok(PropertyValue::String(
                if self.is_ts16 { "16" } else { "12" }.into(),
            )),
            "Serial Send" => Ok(PropertyValue::String(self.serial_send.clone())),
            "Serial Receive" => Ok(PropertyValue::String(self.serial_receive.clone())),
            "UseActionLEDs" => Ok(PropertyValue::String(
                if self.use_action_leds { "On" } else { "Off" }.into(),
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Ok(()),
            "Serial Send" => {
                let cmd = val.to_string();
                self.serial_receive = self.send_recv(&format!("{}\n", cmd))?;
                self.serial_send = cmd.clone();
                self.props.set(name, PropertyValue::String(cmd))
            }
            "UseActionLEDs" => {
                let requested = val.to_string();
                let cmd = match requested.as_str() {
                    "On" => "SSL1\n",
                    "Off" => "SSL0\n",
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.send_recv(cmd)?;
                self.use_action_leds = requested == "On";
                self.props.set(name, PropertyValue::String(requested))
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

impl Hub for TriggerScopeMMHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        let mut devs = Vec::new();
        devs.push("TS_TTL1-8".to_string());
        devs.push("TS_TTL9-16".to_string());
        for i in 1..=16u8 {
            devs.push(format!("TS_DAC{:02}", i));
        }
        Ok(devs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{SignalIO, StateDevice};
    use crate::transport::MockTransport;

    #[test]
    fn hub_initialize_ts16() {
        let t = MockTransport::new().expect("*\n", "ARC TRIGGERSCOPE 16 v1.0-MM");
        let mut hub = TriggerScopeMMHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert!(hub.is_ts16());
    }

    #[test]
    fn hub_initialize_ts12() {
        let t = MockTransport::new().expect("*\n", "ARC TRIGGERSCOPE v1.0-MM");
        let mut hub = TriggerScopeMMHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert!(!hub.is_ts16());
    }

    #[test]
    fn no_transport_error() {
        let mut hub = TriggerScopeMMHub::new();
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn rejects_non_mm_firmware() {
        let t = MockTransport::new().expect("*\n", "ARC TRIGGERSCOPE 16 v1.0");
        let mut hub = TriggerScopeMMHub::new().with_transport(Box::new(t));
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn installed_devices_match_module_data_names() {
        let mut hub = TriggerScopeMMHub::new();
        let devices = hub.detect_installed_devices().unwrap();
        assert_eq!(devices[0], "TS_TTL1-8");
        assert_eq!(devices[1], "TS_TTL9-16");
        assert_eq!(devices[2], "TS_DAC01");
        assert_eq!(devices.last().unwrap(), "TS_DAC16");
        assert_eq!(devices.len(), 18);
    }

    #[test]
    fn hub_created_children_share_transport() {
        let t = MockTransport::new()
            .expect("*\n", "ARC TRIGGERSCOPE 16 v1.0-MM")
            .expect("SSL1\n", "SSL1")
            .expect("PDN0\n", "PDN0-50")
            .expect("SDO0-1\n", "!SDO0-1")
            .expect("SAR1-1\n", "!SAR1-1")
            .expect("PAN1\n", "PAN1-50")
            .expect("SAO1-65535\n", "!SAO1-65535");
        let mut hub = TriggerScopeMMHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();

        let mut ttl = hub.create_ttl_child(0).unwrap();
        ttl.initialize().unwrap();
        ttl.set_position(1).unwrap();

        let mut dac = hub.create_dac_child(1).unwrap();
        dac.initialize().unwrap();
        dac.set_signal(5.0).unwrap();
    }

    #[test]
    fn initialized_port_write_is_reverted() {
        let t = MockTransport::new()
            .expect("*\n", "ARC TRIGGERSCOPE 16 v1.0-MM")
            .expect("SSL1\n", "SSL1");
        let mut hub = TriggerScopeMMHub::new().with_transport(Box::new(t));
        hub.set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        hub.initialize().unwrap();

        hub.set_property("Port", PropertyValue::String("COM2".into()))
            .unwrap();

        assert_eq!(
            hub.get_property("Port").unwrap(),
            PropertyValue::String("COM1".into())
        );
    }
}
