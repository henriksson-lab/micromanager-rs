use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

/// Maximum number of LED channels supported.
const MAX_LEDS: usize = 8;

/// Prizmatix LED controller.
///
/// Implements the `Shutter` trait: open = enable all active channels, closed = disable all.
/// Each channel has an intensity (0-100%) and on/off state property.
pub struct PrizmatixController {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    num_leds: usize,
    /// Intensities for each channel (0-100).
    intensities: [u8; MAX_LEDS],
    /// On/off state for each channel.
    channel_on: [bool; MAX_LEDS],
}

impl PrizmatixController {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("FirmwareName", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("NumLEDs", PropertyValue::Integer(0), true).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            num_leds: 0,
            intensities: [0u8; MAX_LEDS],
            channel_on: [false; MAX_LEDS],
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
        let cmd = command.to_string();
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

    fn intensity_prop_name(ch: usize) -> String {
        format!("LED{}_Intensity", ch + 1)
    }

    fn state_prop_name(ch: usize) -> String {
        format!("LED{}_State", ch + 1)
    }

    /// Send set-power command for a channel.
    fn send_intensity(&mut self, ch: usize, val: u8) -> MmResult<()> {
        // Protocol: P:<channel_1based>,<value>
        let cmd = format!("P:{},{}", ch + 1, val);
        self.cmd(&cmd)?;
        Ok(())
    }

    /// Send on/off command for a channel.
    fn send_on_off(&mut self, ch: usize, on: bool) -> MmResult<()> {
        // Protocol: O:<channel_1based>,<0/1>
        let cmd = format!("O:{},{}", ch + 1, if on { 1 } else { 0 });
        self.cmd(&cmd)?;
        Ok(())
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
        "Prizmatix LED Controller"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Get number of LEDs from V:0 response "V:0_<nLEDs>"
        let v0 = self.cmd("V:0")?;
        let num_leds = v0.find('_')
            .and_then(|pos| v0[pos + 1..].parse::<usize>().ok())
            .unwrap_or(0);
        if num_leds == 0 {
            return Err(MmError::SerialInvalidResponse);
        }
        self.num_leds = num_leds.min(MAX_LEDS);
        self.props.entry_mut("NumLEDs")
            .map(|e| e.value = PropertyValue::Integer(self.num_leds as i64));

        // Get firmware name from V:1 response "V:1_<code>"
        if let Ok(v1) = self.cmd("V:1") {
            let code: u8 = v1.find('_')
                .and_then(|pos| v1[pos + 1..].parse().ok())
                .unwrap_or(0);
            let name = Self::firmware_name(code);
            self.props.entry_mut("FirmwareName")
                .map(|e| e.value = PropertyValue::String(name.into()));
        }

        // Get LED channel names from S:0 response (comma-separated after first char)
        let led_names: Vec<String> = if let Ok(s0) = self.cmd("S:0") {
            // Format: first char is count or prefix, then comma-separated names
            s0.trim_start_matches(|c: char| !c.is_alphabetic())
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            Vec::new()
        };

        // Define per-LED properties
        for i in 0..self.num_leds {
            let led_name = led_names.get(i).cloned()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("LED{}", i + 1));

            let intensity_prop = Self::intensity_prop_name(i);
            let state_prop = Self::state_prop_name(i);

            self.props.define_property(
                &intensity_prop,
                PropertyValue::Integer(0),
                false,
            ).ok();
            self.props.set_property_limits(&intensity_prop, 0.0, 100.0).ok();

            self.props.define_property(
                &state_prop,
                PropertyValue::Integer(0),
                false,
            ).ok();

            let label_prop = format!("LED{}_Name", i + 1);
            self.props.define_property(
                &label_prop,
                PropertyValue::String(led_name),
                true,
            ).ok();
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            // Turn off all channels
            for i in 0..self.num_leds {
                let _ = self.send_on_off(i, false);
            }
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        // Handle LED intensity: LED<N>_Intensity
        if let Some(rest) = name.strip_prefix("LED").and_then(|s| {
            let end = s.find('_')?;
            let num: usize = s[..end].parse().ok()?;
            let suffix = &s[end..];
            if suffix == "_Intensity" { Some(num) } else { None }
        }) {
            let ch = rest - 1;
            if ch < self.num_leds {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u8;
                if self.initialized {
                    self.send_intensity(ch, v)?;
                }
                self.intensities[ch] = v;
                return self.props.set(name, PropertyValue::Integer(v as i64));
            }
        }

        // Handle LED on/off: LED<N>_State
        if let Some(rest) = name.strip_prefix("LED").and_then(|s| {
            let end = s.find('_')?;
            let num: usize = s[..end].parse().ok()?;
            let suffix = &s[end..];
            if suffix == "_State" { Some(num) } else { None }
        }) {
            let ch = rest - 1;
            if ch < self.num_leds {
                let v = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                let on = v != 0;
                if self.initialized {
                    self.send_on_off(ch, on)?;
                }
                self.channel_on[ch] = on;
                return self.props.set(name, PropertyValue::Integer(v));
            }
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
        DeviceType::Shutter
    }

    fn busy(&self) -> bool {
        false
    }
}

impl Shutter for PrizmatixController {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        for i in 0..self.num_leds {
            self.send_on_off(i, open)?;
            self.channel_on[i] = open;
        }
        self.is_open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.is_open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        self.set_open(true)?;
        self.set_open(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("V:0", "V:0_3")
            .expect("V:1", "V:1_4")
            .expect("S:0", "0Red,Green,Blue")
    }

    #[test]
    fn initialize_finds_leds() {
        let mut dev = PrizmatixController::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert_eq!(dev.num_leds, 3);
        assert_eq!(
            dev.get_property("FirmwareName").unwrap(),
            PropertyValue::String("Combi-LED".into())
        );
    }

    #[test]
    fn open_close_shutter() {
        let t = make_transport()
            // set_open(true) — 3 LEDs
            .expect("O:1,1", "OK")
            .expect("O:2,1", "OK")
            .expect("O:3,1", "OK")
            // set_open(false) — 3 LEDs
            .expect("O:1,0", "OK")
            .expect("O:2,0", "OK")
            .expect("O:3,0", "OK");
        let mut dev = PrizmatixController::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_intensity() {
        let t = make_transport().expect("P:1,75", "OK");
        let mut dev = PrizmatixController::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("LED1_Intensity", PropertyValue::Integer(75)).unwrap();
        assert_eq!(dev.intensities[0], 75);
    }

    #[test]
    fn no_transport_error() {
        let mut dev = PrizmatixController::new();
        assert!(dev.initialize().is_err());
    }
}
