//! ESP32 Hub — manages the serial transport and shared state.

use parking_lot::Mutex;
use std::sync::Arc;

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Hub};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

const FIRMWARE_MIN: i32 = 1;

#[derive(Debug, Default)]
pub struct HubState {
    pub switch_state: u8,
    pub shutter_open: bool,
}

pub struct Esp32Hub {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    version: i32,
    pub shared: Arc<Mutex<HubState>>,
    inverted_logic: bool,
    pub has_z_stage: bool,
    pub has_xy_stage: bool,
    pub z_range_um: f64,
    pub x_range_um: f64,
    pub y_range_um: f64,
}

impl Esp32Hub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Logic", PropertyValue::String("Normal".into()), false)
            .unwrap();
        props
            .set_allowed_values("Logic", &["Inverted", "Normal"])
            .unwrap();
        props
            .define_property("Version", PropertyValue::Integer(0), true)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            version: 0,
            shared: Arc::new(Mutex::new(HubState::default())),
            inverted_logic: false,
            has_z_stage: false,
            has_xy_stage: false,
            z_range_um: 0.0,
            x_range_um: 0.0,
            y_range_um: 0.0,
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

    fn send_recv(&mut self, cmd: &str) -> MmResult<String> {
        self.call_transport(|t| Ok(t.send_recv(cmd)?.trim().to_string()))
    }

    fn query_controller_info(&mut self) -> MmResult<i32> {
        let resp = self.send_recv("V")?;
        if !resp.starts_with("MM-ESP32") {
            return Err(MmError::LocallyDefined("ESP32 board not found".into()));
        }
        let ver: i32 = resp
            .split(',')
            .nth(1)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);

        for (axis, field) in [(0u8, "x"), (1, "y"), (2, "z")] {
            let cmd = format!("U,{}", axis);
            if let Ok(ans) = self.send_recv(&cmd) {
                if let Some(range_str) = ans.split(',').nth(1) {
                    if let Ok(range) = range_str.trim().parse::<f64>() {
                        match field {
                            "x" => self.x_range_um = range,
                            "y" => self.y_range_um = range,
                            "z" => self.z_range_um = range,
                            _ => {}
                        }
                    }
                }
            }
        }
        self.has_xy_stage = self.x_range_um > 0.0 && self.y_range_um > 0.0;
        self.has_z_stage = self.z_range_um > 0.0;

        Ok(ver)
    }

    /// Send `S,<val>` to set the digital output.
    pub fn write_switch(&mut self, val: u8) -> MmResult<()> {
        let effective = if self.inverted_logic { !val } else { val };
        let cmd = format!("S,{}", effective);
        self.call_transport(|t| t.send(&cmd))?;
        self.shared.lock().switch_state = val;
        Ok(())
    }

    /// Send `O,<channel>,<value>` to set PWM output.
    pub fn write_pwm(&mut self, channel: u8, value: f64) -> MmResult<()> {
        let cmd = format!("O,{},{}", channel, value);
        self.call_transport(|t| t.send(&cmd))
    }

    pub fn is_inverted(&self) -> bool {
        self.inverted_logic
    }
    pub fn version(&self) -> i32 {
        self.version
    }
}

impl Default for Esp32Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Esp32Hub {
    fn name(&self) -> &str {
        "ESP32-Hub"
    }
    fn description(&self) -> &str {
        "ESP32 Hub (required)"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        let ver = self.query_controller_info()?;
        if ver < FIRMWARE_MIN {
            return Err(MmError::LocallyDefined(format!(
                "Firmware version {} not supported (minimum {})",
                ver, FIRMWARE_MIN
            )));
        }
        self.version = ver;
        self.props
            .entry_mut("Version")
            .map(|e| e.value = PropertyValue::Integer(ver as i64));

        if let Ok(PropertyValue::String(logic)) = self.props.get("Logic").cloned() {
            self.inverted_logic = logic == "Inverted";
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Logic" {
            self.inverted_logic = val.as_str() == "Inverted";
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
        false
    }
}

impl Hub for Esp32Hub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        if !self.initialized && self.query_controller_info().is_err() {
            return Ok(Vec::new());
        }

        Ok(vec![
            "ESP32-Switch".to_string(),
            "ESP32-Shutter".to_string(),
            "ESP32-PWM0".to_string(),
            "ESP32-PWM1".to_string(),
            "ESP32-PWM2".to_string(),
            "ESP32-PWM3".to_string(),
            "ESP32-PWM4".to_string(),
            "ZStage".to_string(),
            "XYStage".to_string(),
            "ESP32-Input".to_string(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;
    use crate::transport::Transport;
    use std::sync::{Arc, Mutex as StdMutex};

    struct RecordingTransport {
        sent: Arc<StdMutex<Vec<String>>>,
    }

    impl RecordingTransport {
        fn new(sent: Arc<StdMutex<Vec<String>>>) -> Self {
            Self { sent }
        }
    }

    impl Transport for RecordingTransport {
        fn send(&mut self, cmd: &str) -> MmResult<()> {
            self.sent.lock().unwrap().push(cmd.to_string());
            Ok(())
        }

        fn receive_line(&mut self) -> MmResult<String> {
            Err(MmError::SerialInvalidResponse)
        }

        fn purge(&mut self) -> MmResult<()> {
            Ok(())
        }
    }

    fn make_hub_with_stages() -> Esp32Hub {
        let t = MockTransport::new()
            .expect("V", "MM-ESP32,5")
            .expect("U,0", "U,200")
            .expect("U,1", "U,200")
            .expect("U,2", "U,100");
        Esp32Hub::new().with_transport(Box::new(t))
    }

    #[test]
    fn initialize_ok() {
        let mut hub = make_hub_with_stages();
        hub.initialize().unwrap();
        assert_eq!(hub.version(), 5);
        assert!(hub.has_xy_stage);
        assert!(hub.has_z_stage);
    }

    #[test]
    fn default_logic_matches_upstream_internal_state() {
        let hub = Esp32Hub::new();
        assert!(!hub.is_inverted());
        assert_eq!(
            hub.get_property("Logic").unwrap(),
            PropertyValue::String("Normal".into())
        );
    }

    #[test]
    fn explicit_inverted_logic_inverts_switch_value() {
        let sent = Arc::new(StdMutex::new(Vec::new()));
        let t = RecordingTransport::new(sent.clone());
        let mut hub = Esp32Hub::new().with_transport(Box::new(t));
        hub.set_property("Logic", PropertyValue::String("Inverted".into()))
            .unwrap();
        hub.write_switch(0x81).unwrap();
        assert_eq!(sent.lock().unwrap().as_slice(), &["S,126".to_string()]);
    }

    #[test]
    fn bad_firmware_rejected() {
        let t = MockTransport::new().any("WrongDevice,1");
        let mut hub = Esp32Hub::new().with_transport(Box::new(t));
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn installed_devices_match_upstream_order() {
        let mut hub = make_hub_with_stages();
        assert_eq!(
            hub.detect_installed_devices().unwrap(),
            vec![
                "ESP32-Switch",
                "ESP32-Shutter",
                "ESP32-PWM0",
                "ESP32-PWM1",
                "ESP32-PWM2",
                "ESP32-PWM3",
                "ESP32-PWM4",
                "ZStage",
                "XYStage",
                "ESP32-Input",
            ]
        );
    }

    #[test]
    fn installed_devices_are_empty_when_detection_fails() {
        let mut hub = Esp32Hub::new();
        assert_eq!(
            hub.detect_installed_devices().unwrap(),
            Vec::<String>::new()
        );
    }
}
