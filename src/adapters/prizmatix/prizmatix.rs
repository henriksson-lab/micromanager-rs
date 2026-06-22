use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic, Hub};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::sync::{Arc, Mutex};

/// Maximum number of LED channels supported.
const MAX_LEDS: usize = 8;

pub type SharedPrizmatixTransport = Arc<Mutex<Box<dyn Transport>>>;

pub struct PrizmatixHub {
    props: PropertyMap,
    transport: Option<SharedPrizmatixTransport>,
    initialized: bool,
    num_leds: usize,
}

impl PrizmatixHub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("NumLEDs", PropertyValue::Integer(0), true)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            num_leds: 0,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(Arc::new(Mutex::new(t)));
        self
    }

    pub fn with_shared_transport(mut self, transport: SharedPrizmatixTransport) -> Self {
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
                    MmError::LocallyDefined("Prizmatix transport lock poisoned".into())
                })?;
                f(guard.as_mut())
            }
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        self.call_transport(|t| Ok(t.send_recv(command)?.trim().to_string()))
    }

    fn read_num_leds(&mut self) -> MmResult<usize> {
        let v0 = self.cmd("V:0\n")?;
        let num_leds = v0
            .rfind('_')
            .and_then(|pos| v0[pos + 1..].parse::<usize>().ok())
            .unwrap_or(0);
        if num_leds == 0 {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(num_leds.min(MAX_LEDS))
    }

    pub fn shared_transport(&self) -> Option<SharedPrizmatixTransport> {
        self.transport.clone()
    }

    pub fn create_controller_child(&self) -> MmResult<PrizmatixController> {
        let transport = self.shared_transport().ok_or(MmError::NotConnected)?;
        Ok(PrizmatixController::new().with_shared_transport(transport))
    }
}

impl Default for PrizmatixHub {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for PrizmatixHub {
    fn name(&self) -> &str {
        "prizmatix-Hub"
    }

    fn description(&self) -> &str {
        "Hub (required)"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.num_leds = self.read_num_leds()?;
        self.props
            .entry_mut("NumLEDs")
            .map(|e| e.value = PropertyValue::Integer(self.num_leds as i64));
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

impl Hub for PrizmatixHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        if !self.initialized {
            self.num_leds = self.read_num_leds()?;
        }
        if self.num_leds > 0 {
            Ok(vec!["Prizmatix Ctrl".into()])
        } else {
            Ok(Vec::new())
        }
    }
}

/// Prizmatix LED controller.
///
/// Upstream implements this child as a Generic device. Each LED channel has
/// an intensity property named after the channel and a `State <channel>` toggle.
pub struct PrizmatixController {
    props: PropertyMap,
    transport: Option<SharedPrizmatixTransport>,
    initialized: bool,
    num_leds: usize,
    led_names: Vec<String>,
    /// Intensities for each channel, scaled to the 12-bit value sent upstream.
    intensities: [u16; MAX_LEDS],
    /// On/off state for each channel.
    channel_on: [bool; MAX_LEDS],
}

impl PrizmatixController {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Firmware Name", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("NumLEDs", PropertyValue::Integer(0), true)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            num_leds: 0,
            led_names: Vec::new(),
            intensities: [0u16; MAX_LEDS],
            channel_on: [false; MAX_LEDS],
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(Arc::new(Mutex::new(t)));
        self
    }

    pub fn with_shared_transport(mut self, transport: SharedPrizmatixTransport) -> Self {
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
                    MmError::LocallyDefined("Prizmatix transport lock poisoned".into())
                })?;
                f(guard.as_mut())
            }
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let cmd = format!("{command}\n");
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }

    /// Parse firmware name from firmware type code.
    fn firmware_name(code: u8) -> &'static str {
        match code {
            1 => "UHPTLCC-USB",
            2 => "UHPTLCC-USB-STBL",
            3 => "FC-LED",
            4 => "Combi-LED",
            5 => "UHP-M-USB",
            6 | 7 => "UHP-F-USB",
            _ => "Unknown",
        }
    }

    fn intensity_prop_name(&self, ch: usize) -> String {
        self.led_names
            .get(ch)
            .cloned()
            .unwrap_or_else(|| format!("LED{}", ch))
    }

    fn state_prop_name(&self, ch: usize) -> String {
        format!("State {}", self.intensity_prop_name(ch))
    }

    fn send_combined_power(&mut self) -> MmResult<()> {
        let mut cmd = String::from("P:");
        for i in 0..self.num_leds {
            if self.channel_on[i] {
                cmd.push_str(&self.intensities[i].to_string());
            } else {
                cmd.push('0');
            }
            cmd.push(',');
        }
        self.cmd(&cmd)?;
        Ok(())
    }

    fn firmware_code(response: &str) -> u8 {
        response
            .rsplit(['_', ':'])
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }
}

impl Default for PrizmatixController {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for PrizmatixController {
    fn name(&self) -> &str {
        "Prizmatix Ctrl"
    }

    fn description(&self) -> &str {
        "Prizmatix Control"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Get number of LEDs from V:0 response "V:0_<nLEDs>"
        let v0 = self.cmd("V:0")?;
        let num_leds = v0
            .rfind('_')
            .and_then(|pos| v0[pos + 1..].parse::<usize>().ok())
            .unwrap_or(0);
        if num_leds == 0 {
            return Err(MmError::SerialInvalidResponse);
        }
        self.num_leds = num_leds.min(MAX_LEDS);
        self.props
            .entry_mut("NumLEDs")
            .map(|e| e.value = PropertyValue::Integer(self.num_leds as i64));

        // Get firmware name from V:1 response.
        if let Ok(v1) = self.cmd("V:1") {
            let code = Self::firmware_code(&v1);
            let name = Self::firmware_name(code);
            self.props
                .entry_mut("Firmware Name")
                .map(|e| e.value = PropertyValue::String(name.into()));
            if code == 2 {
                self.props
                    .define_property("STBL", PropertyValue::Integer(0), false)
                    .ok();
                self.props.set_allowed_values("STBL", &["0", "1"]).ok();
            }
        }

        // Get LED channel names from S:0 response (comma-separated after first char)
        let led_names: Vec<String> = if let Ok(s0) = self.cmd("S:0") {
            // Format: first char is count or prefix, then comma-separated names
            s0.chars()
                .skip(1)
                .collect::<String>()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            Vec::new()
        };

        // Define per-LED properties
        for i in 0..self.num_leds {
            let led_name = led_names
                .get(i)
                .cloned()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("LED{}", i));

            let intensity_prop = led_name.clone();
            let state_prop = format!("State {}", led_name);

            self.props
                .define_property(&intensity_prop, PropertyValue::Integer(0), false)
                .ok();
            self.props
                .set_property_limits(&intensity_prop, 0.0, 100.0)
                .ok();

            self.props
                .define_property(&state_prop, PropertyValue::Integer(0), false)
                .ok();
            self.props.set_allowed_values(&state_prop, &["0", "1"]).ok();

            self.led_names.push(led_name);
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.cmd("P:0");
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "STBL" {
            let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            if v != 0 && v != 1 {
                return Err(MmError::InvalidPropertyValue);
            }
            if self.initialized {
                self.cmd(&format!("K:1,8,{}", v))?;
            }
            return self.props.set(name, PropertyValue::Integer(v));
        }

        if let Some(ch) = (0..self.num_leds).find(|&i| name == self.intensity_prop_name(i)) {
            let percent = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            if !(0.0..=100.0).contains(&percent) {
                return Err(MmError::InvalidPropertyValue);
            }
            let scaled = (percent * 4095.0 / 100.0).floor() as u16;
            self.intensities[ch] = scaled;
            if self.initialized {
                self.send_combined_power()?;
            }
            return self.props.set(name, PropertyValue::Integer(percent as i64));
        }

        if let Some(ch) = (0..self.num_leds).find(|&i| name == self.state_prop_name(i)) {
            let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            if v != 0 && v != 1 {
                return Err(MmError::InvalidPropertyValue);
            }
            self.channel_on[ch] = v != 0;
            if self.initialized {
                self.send_combined_power()?;
            }
            return self.props.set(name, PropertyValue::Integer(v));
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
        DeviceType::Generic
    }

    fn busy(&self) -> bool {
        false
    }
}

impl Generic for PrizmatixController {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("V:0\n", "V:0_3")
            .expect("V:1\n", "V:1_4")
            .expect("S:0\n", "0Red,Green,Blue")
    }

    #[test]
    fn hub_detects_prizmatix_controller_child() {
        let t = MockTransport::new().expect("V:0\n", "V:0_3");
        let mut hub = PrizmatixHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert_eq!(
            hub.get_property("NumLEDs").unwrap(),
            PropertyValue::Integer(3)
        );
        assert_eq!(
            hub.detect_installed_devices().unwrap(),
            vec!["Prizmatix Ctrl".to_string()]
        );
    }

    #[test]
    fn hub_uses_last_underscore_for_led_count_like_upstream() {
        let t = MockTransport::new().expect("V:0\n", "UHP_F_USB_3");
        let mut hub = PrizmatixHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert_eq!(
            hub.get_property("NumLEDs").unwrap(),
            PropertyValue::Integer(3)
        );
    }

    #[test]
    fn hub_port_remains_settable_after_initialize_like_upstream() {
        let t = MockTransport::new().expect("V:0\n", "V:0_3");
        let mut hub = PrizmatixHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();

        hub.set_property("Port", PropertyValue::String("COM2".into()))
            .unwrap();

        assert_eq!(
            hub.get_property("Port").unwrap(),
            PropertyValue::String("COM2".into())
        );
    }

    #[test]
    fn hub_created_controller_uses_shared_transport() {
        let t = MockTransport::new()
            .expect("V:0\n", "V:0_3")
            .expect("V:0\n", "V:0_3")
            .expect("V:1\n", "V:1_4")
            .expect("S:0\n", "0Red,Green,Blue")
            .expect("P:0,0,0,\n", "OK")
            .expect("P:2047,0,0,\n", "OK");
        let mut hub = PrizmatixHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();

        let mut child = hub.create_controller_child().unwrap();
        child.initialize().unwrap();
        child
            .set_property("Red", PropertyValue::Integer(50))
            .unwrap();
        child
            .set_property("State Red", PropertyValue::Integer(1))
            .unwrap();

        assert_eq!(child.intensities[0], 2047);
    }

    #[test]
    fn initialize_finds_leds() {
        let mut dev = PrizmatixController::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert_eq!(dev.num_leds, 3);
        assert_eq!(
            dev.get_property("Firmware Name").unwrap(),
            PropertyValue::String("Combi-LED".into())
        );
        assert!(dev.has_property("Red"));
        assert!(dev.has_property("State Red"));
    }

    #[test]
    fn controller_uses_last_underscore_for_led_count_like_upstream() {
        let t = MockTransport::new()
            .expect("V:0\n", "UHP_F_USB_2")
            .expect("V:1\n", "V:1_4")
            .expect("S:0\n", "0Red,Green");
        let mut dev = PrizmatixController::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert_eq!(dev.num_leds, 2);
        assert!(dev.has_property("Green"));
    }

    #[test]
    fn state_and_intensity_send_combined_power_command() {
        let t = make_transport()
            .expect("P:0,0,0,\n", "OK")
            .expect("P:3071,0,0,\n", "OK");
        let mut dev = PrizmatixController::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Red", PropertyValue::Integer(75)).unwrap();
        dev.set_property("State Red", PropertyValue::Integer(1))
            .unwrap();
        assert_eq!(dev.intensities[0], 3071);
    }

    #[test]
    fn state_properties_are_binary_like_upstream() {
        let mut dev = PrizmatixController::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert_eq!(
            dev.set_property("State Red", PropertyValue::Integer(2))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn shutdown_sends_upstream_all_off_command() {
        let t = make_transport().expect("P:0\n", "OK");
        let mut dev = PrizmatixController::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.shutdown().unwrap();
    }

    #[test]
    fn no_transport_error() {
        let mut dev = PrizmatixController::new();
        assert!(dev.initialize().is_err());
    }

    #[test]
    fn controller_description_matches_upstream() {
        let dev = PrizmatixController::new();
        assert_eq!(dev.description(), "Prizmatix Control");
    }
}
