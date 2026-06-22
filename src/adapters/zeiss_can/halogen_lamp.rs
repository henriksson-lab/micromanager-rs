/// Zeiss CAN-bus halogen lamp shutter/intensity device.
///
/// Upstream `HalogenLamp` controls shutter state through `HPCT8,*`,
/// light-manager state through `HPCT12,*`, and intensity through `HPCV1,*`.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::types::{DeviceType, PropertyValue};
use std::cell::Cell;
use std::time::Instant;

use super::hub::{ZeissHub, DEVICE_NAME_HALOGEN_LAMP};

pub struct ZeissHalogenLamp {
    props: PropertyMap,
    hub: ZeissHub,
    initialized: bool,
    open: Cell<bool>,
    light_manager_on: Cell<bool>,
    intensity: Cell<i64>,
    changed_time: Cell<Instant>,
    delay_ms: f64,
}

impl ZeissHalogenLamp {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            props,
            hub: ZeissHub::new(),
            initialized: false,
            open: Cell::new(true),
            light_manager_on: Cell::new(false),
            intensity: Cell::new(0),
            changed_time: Cell::new(Instant::now()),
            delay_ms: 0.0,
        }
    }

    pub fn new_with_hub(hub: ZeissHub) -> Self {
        let mut s = Self::new();
        s.hub = hub;
        s
    }

    fn define_runtime_properties(&mut self) -> MmResult<()> {
        if !self.props.has_property("Name") {
            self.props
                .define_property("Name", PropertyValue::String(self.name().into()), true)?;
        }
        if !self.props.has_property("Description") {
            self.props.define_property(
                "Description",
                PropertyValue::String("HalogenLamp".into()),
                true,
            )?;
        }
        if !self.props.has_property("State") {
            self.props.define_property(
                "State",
                PropertyValue::Integer(if self.open.get() { 1 } else { 0 }),
                false,
            )?;
            self.props.set_allowed_values("State", &["0", "1"])?;
        }
        if !self.props.has_property("LightManager") {
            self.props
                .define_property("LightManager", PropertyValue::Integer(0), false)?;
            self.props.set_allowed_values("LightManager", &["0", "1"])?;
        }
        if !self.props.has_property("Intensity") {
            self.props
                .define_property("Intensity", PropertyValue::Integer(0), false)?;
            self.props.set_property_limits("Intensity", 0.0, 256.0)?;
        }
        if !self.props.has_property("Delay_ms") {
            self.props
                .define_property("Delay_ms", PropertyValue::Float(0.0), false)?;
            self.props.set_property_limits("Delay_ms", 0.0, f64::MAX)?;
        }
        Ok(())
    }

    fn read_open(&self) -> MmResult<bool> {
        let resp = self.hub.send("HPCt8")?;
        let body = resp
            .strip_prefix("PH")
            .ok_or(MmError::SerialInvalidResponse)?;
        match body.chars().next() {
            Some('0') => Ok(true),
            Some('1') => Ok(false),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn set_light_manager(&mut self, on: bool) -> MmResult<()> {
        self.hub.execute(if on { "HPCT12,2" } else { "HPCT12,1" })?;
        self.light_manager_on.set(on);
        Ok(())
    }

    fn read_light_manager(&self) -> MmResult<bool> {
        let resp = self.hub.send("HPCt12")?;
        let body = resp
            .strip_prefix("PH")
            .ok_or(MmError::SerialInvalidResponse)?;
        match body.chars().next() {
            Some('1') => Ok(false),
            Some('2') => Ok(true),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn set_intensity(&mut self, intensity: i64) -> MmResult<()> {
        if !(0..=256).contains(&intensity) {
            return Err(MmError::InvalidPropertyValue);
        }
        self.hub.execute(&format!("HPCV1,{}", intensity))?;
        self.intensity.set(intensity);
        Ok(())
    }

    fn read_intensity(&self) -> MmResult<i64> {
        let resp = self.hub.send("HPCv1")?;
        let body = resp
            .strip_prefix("PH")
            .ok_or(MmError::SerialInvalidResponse)?;
        let intensity: i64 = body
            .trim()
            .parse()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        if (0..=255).contains(&intensity) {
            Ok(intensity)
        } else {
            Err(MmError::SerialInvalidResponse)
        }
    }
}

impl Default for ZeissHalogenLamp {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ZeissHalogenLamp {
    fn name(&self) -> &str {
        DEVICE_NAME_HALOGEN_LAMP
    }

    fn description(&self) -> &str {
        "HalogenLamp"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.hub.is_connected() {
            return Err(MmError::NotConnected);
        }
        if self.initialized {
            return Ok(());
        }
        self.changed_time.set(Instant::now());
        self.open.set(self.read_open()?);
        self.set_light_manager(false)?;
        self.define_runtime_properties()?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => {
                let open = self.read_open()?;
                self.open.set(open);
                Ok(PropertyValue::Integer(if open { 1 } else { 0 }))
            }
            "LightManager" => {
                let on = self.read_light_manager()?;
                self.light_manager_on.set(on);
                Ok(PropertyValue::Integer(if on { 1 } else { 0 }))
            }
            "Intensity" => {
                let intensity = self.read_intensity()?;
                self.intensity.set(intensity);
                Ok(PropertyValue::Integer(intensity))
            }
            "Delay_ms" => Ok(PropertyValue::Float(self.delay_ms)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if state != 0 && state != 1 {
                    return Err(MmError::InvalidPropertyValue);
                }
                let open = state == 1;
                self.set_open(open)?;
                self.props
                    .set(name, PropertyValue::Integer(if open { 1 } else { 0 }))
            }
            "LightManager" => {
                let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if state != 0 && state != 1 {
                    return Err(MmError::InvalidPropertyValue);
                }
                let on = state == 1;
                self.set_light_manager(on)?;
                self.props
                    .set(name, PropertyValue::Integer(if on { 1 } else { 0 }))
            }
            "Intensity" => {
                let intensity = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_intensity(intensity)?;
                self.props.set(name, PropertyValue::Integer(intensity))
            }
            "Delay_ms" => {
                let delay = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if delay < 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.delay_ms = delay;
                self.props.set(name, PropertyValue::Float(delay))
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
        DeviceType::Shutter
    }

    fn busy(&self) -> bool {
        self.changed_time.get().elapsed().as_secs_f64() * 1000.0 < self.delay_ms
    }
}

impl Shutter for ZeissHalogenLamp {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        self.changed_time.set(Instant::now());
        self.hub.execute(if open { "HPCT8,0" } else { "HPCT8,1" })?;
        self.open.set(open);
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        let open = self.read_open()?;
        self.open.set(open);
        Ok(open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::NotSupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;
    use crate::transport::Transport;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    fn halogen_with(t: MockTransport) -> ZeissHalogenLamp {
        let hub = ZeissHub::new().with_transport(Box::new(t));
        ZeissHalogenLamp::new_with_hub(hub)
    }

    struct RecordingTransport {
        script: VecDeque<String>,
        commands: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingTransport {
        fn new(responses: &[&str], commands: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                script: responses.iter().map(|s| s.to_string()).collect(),
                commands,
            }
        }
    }

    impl Transport for RecordingTransport {
        fn send(&mut self, cmd: &str) -> MmResult<()> {
            self.commands.lock().unwrap().push(cmd.to_string());
            Ok(())
        }

        fn receive_line(&mut self) -> MmResult<String> {
            self.script.pop_front().ok_or(MmError::SerialTimeout)
        }

        fn purge(&mut self) -> MmResult<()> {
            Ok(())
        }
    }

    #[test]
    fn initialize_reads_state_and_switches_light_manager_off() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let hub = ZeissHub::new().with_transport(Box::new(RecordingTransport::new(
            &["PH0"],
            commands.clone(),
        )));
        let mut lamp = ZeissHalogenLamp::new_with_hub(hub);

        lamp.initialize().unwrap();

        assert_eq!(
            commands.lock().unwrap().as_slice(),
            &["HPCt8\r", "HPCT12,1\r"]
        );
        assert!(lamp.has_property("State"));
        assert!(lamp.has_property("LightManager"));
        assert!(lamp.has_property("Intensity"));
    }

    #[test]
    fn open_close_uses_upstream_reversed_wire_values() {
        let t = MockTransport::new()
            .expect("HPCt8\r", "PH1")
            .expect("HPCt8\r", "PH0");
        let mut lamp = halogen_with(t);

        assert!(!lamp.get_open().unwrap());
        lamp.set_open(true).unwrap();
        assert!(lamp.get_open().unwrap());
        lamp.set_open(false).unwrap();
    }

    #[test]
    fn light_manager_and_intensity_are_live_properties() {
        let t = MockTransport::new()
            .expect("HPCt8\r", "PH0")
            .expect("HPCt8\r", "PH0")
            .expect("HPCt12\r", "PH2")
            .expect("HPCv1\r", "PH123");
        let mut lamp = halogen_with(t);

        lamp.initialize().unwrap();
        assert!(lamp.get_open().unwrap());
        assert_eq!(
            lamp.get_property("LightManager").unwrap(),
            PropertyValue::Integer(1)
        );
        assert_eq!(
            lamp.get_property("Intensity").unwrap(),
            PropertyValue::Integer(123)
        );
        lamp.set_property("LightManager", PropertyValue::Integer(0))
            .unwrap();
        lamp.set_property("Intensity", PropertyValue::Integer(42))
            .unwrap();
    }

    #[test]
    fn malformed_halogen_reads_are_rejected() {
        let t = MockTransport::new()
            .expect("HPCt8\r", "PH3")
            .expect("HPCv1\r", "PH256");
        let lamp = halogen_with(t);

        assert!(lamp.get_open().is_err());
        assert!(lamp.get_property("Intensity").is_err());
    }

    #[test]
    fn invalid_binary_properties_do_not_send_commands() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let hub = ZeissHub::new().with_transport(Box::new(RecordingTransport::new(
            &["PH0"],
            commands.clone(),
        )));
        let mut lamp = ZeissHalogenLamp::new_with_hub(hub);
        lamp.initialize().unwrap();
        commands.lock().unwrap().clear();

        assert_eq!(
            lamp.set_property("State", PropertyValue::Integer(2)),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            lamp.set_property("LightManager", PropertyValue::Integer(2)),
            Err(MmError::InvalidPropertyValue)
        );
        assert!(commands.lock().unwrap().is_empty());
    }

    #[test]
    fn invalid_delay_does_not_update_cache() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let hub =
            ZeissHub::new().with_transport(Box::new(RecordingTransport::new(&["PH0"], commands)));
        let mut lamp = ZeissHalogenLamp::new_with_hub(hub);
        lamp.initialize().unwrap();

        assert_eq!(
            lamp.set_property("Delay_ms", PropertyValue::Float(-1.0)),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            lamp.get_property("Delay_ms").unwrap(),
            PropertyValue::Float(0.0)
        );
    }
}
