/// 89 North LDI (Laser Diode Illuminator).
///
/// Protocol (TX/RX `\n`):
///   `CONFIG?\n`               → "CONFIG:<nm1>,<nm2>,..."  available wavelengths
///   `F_MODE?\n`               → "F_MODE=RUN|IDLE"          functional mode
///   `RUN\n` / `IDLE\n`        → OK                         set mode
///   `SET:<nm>?\n`             → "SET:<nm>=<float>"         query intensity
///   `SET:<nm>=<0.0-100.0>\n`  → OK                         set intensity (%)
///   `SHUTTER:<nm>?\n`         → "SHUTTER:<nm>=OPEN|CLOSED"
///   `SHUTTER:<nm>=OPEN\n` /   → OK
///   `SHUTTER:<nm>=CLOSED\n`
///   `FAULT?\n`                → "ok" or "FAULT:<desc>"
///   `CLEAR\n`                 → OK                         clear faults
///
/// Shutter set_open sends a combined command:
///   `SHUTTER:<nm1>=OPEN,<nm2>=OPEN,...\n`   (for all wavelengths)
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Shutter};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

pub struct LdiController {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    wavelengths: Vec<u32>,
    intensities: Vec<f64>,   // per wavelength, 0.0–100.0
    shutter_open: Vec<bool>, // per wavelength auto-shutter state
    open: bool,
}

impl LdiController {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("FunctionalMode", PropertyValue::String("RUN".into()), false).unwrap();
        props.set_allowed_values("FunctionalMode", &["RUN", "IDLE"]).unwrap();
        props.define_property("Fault", PropertyValue::String(String::new()), true).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            wavelengths: Vec::new(),
            intensities: Vec::new(),
            shutter_open: Vec::new(),
            open: false,
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
        let full = format!("{}\n", command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            let trimmed = r.trim().to_string();
            if trimmed.starts_with("ERR") {
                Err(MmError::LocallyDefined(format!("LDI error: {}", trimmed)))
            } else {
                Ok(trimmed)
            }
        })
    }

    fn wavelength_index(&self, nm: u32) -> Option<usize> {
        self.wavelengths.iter().position(|&w| w == nm)
    }
}

impl Default for LdiController { fn default() -> Self { Self::new() } }

impl Device for LdiController {
    fn name(&self) -> &str { "LdiController" }
    fn description(&self) -> &str { "89 North Laser Diode Illuminator" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Discover available wavelengths
        let cfg = self.cmd("CONFIG?")?;
        let wl_str = cfg.strip_prefix("CONFIG:").unwrap_or("");
        self.wavelengths = wl_str.split(',')
            .filter_map(|s| s.trim().parse::<u32>().ok())
            .collect();
        self.intensities = vec![100.0; self.wavelengths.len()];
        self.shutter_open = vec![true; self.wavelengths.len()];
        // Ensure running mode
        let mode = self.cmd("F_MODE?")?;
        if mode.contains("IDLE") {
            self.cmd("RUN")?;
        }
        // Define per-wavelength properties
        for &nm in &self.wavelengths {
            let int_key = format!("Intensity_{}nm", nm);
            let _ = self.props.define_property(&int_key, PropertyValue::Float(100.0), false);
            let sh_key = format!("AutoShutter_{}nm", nm);
            let _ = self.props.define_property(&sh_key, PropertyValue::String("1".into()), false);
            let _ = self.props.set_allowed_values(&sh_key, &["0", "1"]);
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.set_open(false);
            let _ = self.cmd("IDLE");
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        // Handle per-wavelength intensity
        for (i, &nm) in self.wavelengths.iter().enumerate() {
            let int_key = format!("Intensity_{}nm", nm);
            if name == int_key {
                let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.cmd(&format!("SET:{}={:.1}", nm, pct))?;
                }
                self.intensities[i] = pct;
                self.props.entry_mut(name).map(|e| e.value = PropertyValue::Float(pct));
                return Ok(());
            }
            let sh_key = format!("AutoShutter_{}nm", nm);
            if name == sh_key {
                self.shutter_open[i] = val.as_str() == "1";
                self.props.entry_mut(name).map(|e| e.value = val);
                return Ok(());
            }
        }
        match name {
            "FunctionalMode" => {
                let mode = val.as_str().to_string();
                if self.initialized {
                    self.cmd(&mode)?;
                }
                self.props.entry_mut(name).map(|e| e.value = PropertyValue::String(mode));
                Ok(())
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

impl Shutter for LdiController {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        // Build combined SHUTTER command for all wavelengths
        let state = if open { "OPEN" } else { "CLOSED" };
        let parts: Vec<String> = self.wavelengths.iter()
            .map(|&nm| format!("{}={}", nm, state))
            .collect();
        if !parts.is_empty() {
            let cmd = format!("SHUTTER:{}", parts.join(","));
            self.cmd(&cmd)?;
        }
        self.open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> { Ok(self.open) }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect("CONFIG?\n", "CONFIG:405,488,561,640")
            .expect("F_MODE?\n", "F_MODE=RUN")
    }

    #[test]
    fn initialize() {
        let mut dev = LdiController::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        assert_eq!(dev.wavelengths, vec![405, 488, 561, 640]);
    }

    #[test]
    fn open_close() {
        let t = make_init_transport()
            .expect("SHUTTER:405=OPEN,488=OPEN,561=OPEN,640=OPEN\n", "ok")
            .expect("SHUTTER:405=CLOSED,488=CLOSED,561=CLOSED,640=CLOSED\n", "ok");
        let mut dev = LdiController::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn set_intensity() {
        let t = make_init_transport()
            .expect("SET:488=75.0\n", "ok");
        let mut dev = LdiController::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Intensity_488nm", PropertyValue::Float(75.0)).unwrap();
        assert!((dev.intensities[1] - 75.0).abs() < 0.01);
    }

    #[test]
    fn idle_mode_on_init_runs() {
        let t = MockTransport::new()
            .expect("CONFIG?\n", "CONFIG:405")
            .expect("F_MODE?\n", "F_MODE=IDLE")
            .expect("RUN\n", "ok");
        let mut dev = LdiController::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
    }

    #[test]
    fn no_transport_error() { assert!(LdiController::new().initialize().is_err()); }
}
