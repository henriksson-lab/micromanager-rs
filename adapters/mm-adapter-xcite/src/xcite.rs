/// X-Cite 120PC Exacte xenon arc lamp illuminator.
///
/// 2-character ASCII command protocol (no terminator on most responses).
///   `tt\r`        → connect
///   `aa\r`        → clear alarm
///   `vv\r`        → software version string
///   `uu\r`        → unit status bitmask string
///   `ii\r`        → current intensity level (char '0'–'4')
///   `mm\r`        → open shutter
///   `zz\r`        → close shutter
///   `bb\r`        → turn lamp on
///   `ss\r`        → turn lamp off
///   `[b'i', N]`   → set intensity to level N (0=0%, 1=12%, 2=25%, 3=50%, 4=100%)
///   `ll\r`        → lock front panel
///   `nn\r`        → unlock front panel
///
/// Unit status bitmask (from `uu`):
///   bit 0: alarm active
///   bit 1: lamp on
///   bit 2: shutter open
///   bit 4: lamp ready
///   bit 5: panel locked
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Shutter};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

const INTENSITIES: [&str; 5] = ["0", "12", "25", "50", "100"];

pub struct XCite120PC {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    shutter_open: bool,
    lamp_on: bool,
    intensity_level: u8,
}

impl XCite120PC {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Intensity_pct", PropertyValue::String("100".into()), false).unwrap();
        props.set_allowed_values("Intensity_pct", &INTENSITIES).unwrap();
        props.define_property("LampState", PropertyValue::String("Off".into()), false).unwrap();
        props.set_allowed_values("LampState", &["On", "Off"]).unwrap();
        props.define_property("PanelLock", PropertyValue::String("Off".into()), false).unwrap();
        props.set_allowed_values("PanelLock", &["On", "Off"]).unwrap();
        props.define_property("Version", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("LampHours", PropertyValue::String(String::new()), true).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            shutter_open: false,
            lamp_on: false,
            intensity_level: 4,
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

    fn parse_status(&mut self, status_str: &str) -> (bool, bool, bool, bool) {
        // Status is a bitmask returned as an integer string
        let bits: u32 = status_str.trim().parse().unwrap_or(0);
        let alarm      = (bits & 0x01) != 0;
        let lamp_on    = (bits & 0x02) != 0;
        let shutter    = (bits & 0x04) != 0;
        let locked     = (bits & 0x20) != 0;
        (alarm, lamp_on, shutter, locked)
    }
}

impl Default for XCite120PC {
    fn default() -> Self { Self::new() }
}

impl Device for XCite120PC {
    fn name(&self) -> &str { "XCite120PC" }
    fn description(&self) -> &str { "X-Cite 120PC Exacte xenon arc lamp" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        let _ = self.cmd("tt"); // connect
        let _ = self.cmd("aa"); // clear alarm

        let ver = self.cmd("vv")?;
        self.props.entry_mut("Version").map(|e| e.value = PropertyValue::String(ver));

        let status = self.cmd("uu")?;
        let (_alarm, lamp, shutter, locked) = self.parse_status(&status);
        self.lamp_on = lamp;
        self.shutter_open = shutter;
        self.props.entry_mut("LampState")
            .map(|e| e.value = PropertyValue::String(if lamp { "On".into() } else { "Off".into() }));
        self.props.entry_mut("PanelLock")
            .map(|e| e.value = PropertyValue::String(if locked { "On".into() } else { "Off".into() }));

        let level_str = self.cmd("ii")?;
        self.intensity_level = level_str.trim().parse::<u8>().unwrap_or(4).min(4);
        let pct = INTENSITIES[self.intensity_level as usize];
        self.props.entry_mut("Intensity_pct")
            .map(|e| e.value = PropertyValue::String(pct.into()));

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.cmd("zz"); // close shutter
            self.shutter_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Intensity_pct" => {
                let pct_str = val.as_str().to_string();
                let level = INTENSITIES.iter().position(|&s| s == pct_str)
                    .ok_or(MmError::InvalidPropertyValue)? as u8;
                if self.initialized {
                    self.call_transport(|t| t.send_bytes(&[b'i', level]))?;
                    let _ = self.call_transport(|t| t.receive_line());
                }
                self.intensity_level = level;
                self.props.set(name, PropertyValue::String(pct_str))
            }
            "LampState" => {
                let state = val.as_str().to_string();
                if self.initialized {
                    let cmd = if state == "On" { "bb" } else { "ss" };
                    self.cmd(cmd)?;
                }
                self.lamp_on = state == "On";
                self.props.set(name, PropertyValue::String(state))
            }
            "PanelLock" => {
                let lock = val.as_str().to_string();
                if self.initialized {
                    let cmd = if lock == "On" { "ll" } else { "nn" };
                    self.cmd(cmd)?;
                }
                self.props.set(name, PropertyValue::String(lock))
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Shutter }
    fn busy(&self) -> bool { false }
}

impl Shutter for XCite120PC {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let cmd = if open { "mm" } else { "zz" };
        self.cmd(cmd)?;
        self.shutter_open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.shutter_open) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> { self.set_open(true) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .any("OK")          // tt
            .any("OK")          // aa
            .any("v1.23")       // vv
            .any("0")           // uu: all bits clear → shutter closed, lamp off
            .any("4")           // ii: level 4 = 100%
    }

    #[test]
    fn initialize() {
        let mut dev = XCite120PC::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(dev.intensity_level, 4);
    }

    #[test]
    fn open_close_shutter() {
        let t = make_transport().any("OK").any("OK");
        let mut dev = XCite120PC::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_intensity() {
        let t = make_transport()
            .expect_binary(&[b'i', 2]) // level 2 = 25%
            .any("OK");                // discard response
        let mut dev = XCite120PC::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Intensity_pct", PropertyValue::String("25".into())).unwrap();
        assert_eq!(dev.intensity_level, 2);
    }

    #[test]
    fn lamp_on_off() {
        let t = make_transport().any("OK").any("OK");
        let mut dev = XCite120PC::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("LampState", PropertyValue::String("On".into())).unwrap();
        assert!(dev.lamp_on);
        dev.set_property("LampState", PropertyValue::String("Off".into())).unwrap();
        assert!(!dev.lamp_on);
    }

    #[test]
    fn no_transport_error() {
        assert!(XCite120PC::new().initialize().is_err());
    }
}
