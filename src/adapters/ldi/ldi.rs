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
///   `SHUTTER:<nm1>=1,<nm2>=1,...\n`   (for all wavelengths)
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct LdiController {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    wavelengths: Vec<u32>,
    intensities: Vec<f64>, // per wavelength, 0.0-100.0
    auto_shutter_wavelengths: [Option<u32>; 4],
    open: bool,
}

impl LdiController {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            wavelengths: Vec::new(),
            intensities: Vec::new(),
            auto_shutter_wavelengths: [None; 4],
            open: false,
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

    #[allow(dead_code)]
    fn wavelength_index(&self, nm: u32) -> Option<usize> {
        self.wavelengths.iter().position(|&w| w == nm)
    }

    fn validate_property_value(&self, name: &str, val: &PropertyValue) -> MmResult<()> {
        let entry = self
            .props
            .entry(name)
            .ok_or_else(|| MmError::UnknownLabel(name.to_string()))?;
        if entry.read_only {
            return Ok(());
        }
        if !entry.allowed_values.is_empty() {
            let val_str = val.to_string();
            if !entry.allowed_values.iter().any(|v| v == &val_str) {
                return Err(MmError::InvalidPropertyValue);
            }
        }
        if entry.has_limits {
            let numeric = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            if numeric < entry.lower_limit || numeric > entry.upper_limit {
                return Err(MmError::InvalidPropertyValue);
            }
        }
        Ok(())
    }

    fn set_cached_property(&mut self, name: &str, val: PropertyValue) {
        if let Some(entry) = self.props.entry_mut(name) {
            entry.value = val;
        }
    }

    fn define_property_if_missing(
        &mut self,
        name: &str,
        value: PropertyValue,
        read_only: bool,
    ) -> MmResult<()> {
        if !self.props.has_property(name) {
            self.props.define_property(name, value, read_only)?;
        }
        Ok(())
    }

    fn define_runtime_properties(&mut self) -> MmResult<()> {
        self.define_property_if_missing(
            "Functional Mode",
            PropertyValue::String("IDLE".into()),
            false,
        )?;
        self.props
            .set_allowed_values("Functional Mode", &["IDLE", "RUN"])?;

        self.define_property_if_missing(
            "FunctionalMode",
            PropertyValue::String("IDLE".into()),
            false,
        )?;
        self.props
            .set_allowed_values("FunctionalMode", &["IDLE", "RUN"])?;

        self.define_property_if_missing(
            "Intensity Control",
            PropertyValue::String("PC".into()),
            false,
        )?;
        self.props
            .set_allowed_values("Intensity Control", &["PC", "EXT"])?;

        self.define_property_if_missing(
            "Shutter Control",
            PropertyValue::String("PC".into()),
            false,
        )?;
        self.props
            .set_allowed_values("Shutter Control", &["PC", "EXT"])?;

        self.define_property_if_missing("Despeckler", PropertyValue::String("ON".into()), false)?;
        self.props
            .set_allowed_values("Despeckler", &["ON", "OFF"])?;

        self.define_property_if_missing(
            "Sleep Timer (Minutes)",
            PropertyValue::Integer(30),
            false,
        )?;
        self.props
            .set_property_limits("Sleep Timer (Minutes)", 0.0, 99.0)?;

        self.define_property_if_missing("Fault", PropertyValue::String("NONE".into()), false)?;
        self.props.set_allowed_values("Fault", &["CLEAR"])?;
        Ok(())
    }

    fn set_string_command_property(
        &mut self,
        name: &str,
        val: PropertyValue,
        command: &str,
    ) -> MmResult<()> {
        self.validate_property_value(name, &val)?;
        let value = val.as_str().to_string();
        if self.initialized {
            self.cmd(&format!("{}={}", command, value))?;
        }
        self.set_cached_property(name, PropertyValue::String(value));
        Ok(())
    }
}

impl Default for LdiController {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for LdiController {
    fn name(&self) -> &str {
        "89 North Laser Diode Illuminator"
    }
    fn description(&self) -> &str {
        "Multi-line, Solid-State Laser Illuminator"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Discover available wavelengths
        let cfg = self.cmd("CONFIG?")?;
        let wl_str = cfg.strip_prefix("CONFIG:").unwrap_or("");
        self.wavelengths = wl_str
            .split(',')
            .filter_map(|s| s.trim().parse::<u32>().ok())
            .filter(|&w| w < 9990)
            .collect();
        self.intensities = vec![0.0; self.wavelengths.len()];
        self.define_runtime_properties()?;
        // Define per-wavelength properties
        for &nm in &self.wavelengths {
            let int_key = format!("{} Intensity", nm);
            let _ = self
                .props
                .define_property(&int_key, PropertyValue::Float(0.0), false);
            let _ = self.props.set_property_limits(&int_key, 0.0, 100.0);
            let compat_int_key = format!("Intensity_{}nm", nm);
            let _ = self
                .props
                .define_property(&compat_int_key, PropertyValue::Float(0.0), false);
            let _ = self.props.set_property_limits(&compat_int_key, 0.0, 100.0);
            let sh_key = format!("{} Shutter", nm);
            let _ =
                self.props
                    .define_property(&sh_key, PropertyValue::String("CLOSED".into()), false);
            let _ = self.props.set_allowed_values(&sh_key, &["CLOSED", "OPEN"]);
            let ttl_key = format!("{} TTL Inverted", nm);
            let _ =
                self.props
                    .define_property(&ttl_key, PropertyValue::String("OFF".into()), false);
            let _ = self.props.set_allowed_values(&ttl_key, &["OFF", "ON"]);
        }
        let allowed: Vec<String> = std::iter::once("None".to_string())
            .chain(self.wavelengths.iter().map(u32::to_string))
            .collect();
        let allowed_refs: Vec<&str> = allowed.iter().map(String::as_str).collect();
        for i in 1..=4 {
            let key = format!("Auto Shutter Wavelength {}", i);
            let _ = self
                .props
                .define_property(&key, PropertyValue::String("None".into()), false);
            let _ = self.props.set_allowed_values(&key, &allowed_refs);
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
        // Handle per-wavelength intensity
        for i in 0..self.wavelengths.len() {
            let nm = self.wavelengths[i];
            let int_key = format!("Intensity_{}nm", nm);
            let upstream_int_key = format!("{} Intensity", nm);
            if name == int_key || name == upstream_int_key {
                self.validate_property_value(name, &val)?;
                let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.cmd(&format!("SET:{}={:.1}", nm, pct))?;
                }
                self.intensities[i] = pct;
                self.set_cached_property(name, PropertyValue::Float(pct));
                if name == int_key {
                    self.set_cached_property(&upstream_int_key, PropertyValue::Float(pct));
                } else {
                    self.set_cached_property(&int_key, PropertyValue::Float(pct));
                }
                return Ok(());
            }
            let sh_key = format!("{} Shutter", nm);
            if name == sh_key {
                self.validate_property_value(name, &val)?;
                let state = val.as_str().to_string();
                if self.initialized {
                    self.cmd(&format!("SHUTTER:{}={}", nm, state))?;
                }
                self.set_cached_property(name, PropertyValue::String(state));
                return Ok(());
            }
            let ttl_key = format!("{} TTL Inverted", nm);
            if name == ttl_key {
                return self.set_string_command_property(name, val, &format!("TTL_INVERT:{}", nm));
            }
        }
        if let Some(slot) = name.strip_prefix("Auto Shutter Wavelength ") {
            let idx = slot
                .parse::<usize>()
                .ok()
                .and_then(|n| n.checked_sub(1))
                .filter(|&n| n < 4)
                .ok_or_else(|| MmError::UnknownLabel(name.to_string()))?;
            self.validate_property_value(name, &val)?;
            let value = val.as_str().to_string();
            self.auto_shutter_wavelengths[idx] = if value == "None" {
                None
            } else {
                Some(
                    value
                        .parse::<u32>()
                        .map_err(|_| MmError::InvalidPropertyValue)?,
                )
            };
            self.set_cached_property(name, PropertyValue::String(value));
            return Ok(());
        }
        match name {
            "Port" => {
                if self.initialized {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.props.set(name, val)
            }
            "Functional Mode" | "FunctionalMode" => {
                self.validate_property_value(name, &val)?;
                let mode = val.as_str().to_string();
                if self.initialized {
                    self.cmd(&mode)?;
                }
                self.set_cached_property("Functional Mode", PropertyValue::String(mode.clone()));
                self.set_cached_property("FunctionalMode", PropertyValue::String(mode));
                return Ok(());
            }
            "Intensity Control" => self.set_string_command_property(name, val, "INT_MODE"),
            "Shutter Control" => self.set_string_command_property(name, val, "SH_MODE"),
            "Despeckler" => self.set_string_command_property(name, val, "SPECKLE"),
            "Sleep Timer (Minutes)" => {
                self.validate_property_value(name, &val)?;
                let minutes = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.cmd(&format!("SLEEP={}", minutes))?;
                }
                self.set_cached_property(name, PropertyValue::Integer(minutes));
                Ok(())
            }
            "Fault" => {
                self.validate_property_value(name, &val)?;
                if self.initialized {
                    self.cmd("CLEAR")?;
                }
                self.set_cached_property(name, PropertyValue::String("CLEAR".into()));
                Ok(())
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
        false
    }
}

impl Shutter for LdiController {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let state = if open { "1" } else { "0" };
        let parts: Vec<String> = self
            .auto_shutter_wavelengths
            .iter()
            .filter_map(|&nm| nm.map(|w| format!("{}={}", w, state)))
            .collect();
        self.open = true;
        if !parts.is_empty() {
            let cmd = format!("SHUTTER:{}", parts.join(","));
            self.cmd(&cmd)?;
            self.open = open;
        }
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.open)
    }

    fn fire(&mut self, _delta_t: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        MockTransport::new().expect("CONFIG?\n", "CONFIG:405,488,561,640")
    }

    #[test]
    fn initialize() {
        let mut dev = LdiController::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        assert_eq!(dev.wavelengths, vec![405, 488, 561, 640]);
        assert_eq!(dev.name(), "89 North Laser Diode Illuminator");
        assert_eq!(
            dev.description(),
            "Multi-line, Solid-State Laser Illuminator"
        );
    }

    #[test]
    fn open_close() {
        let t = make_init_transport()
            .expect("SHUTTER:405=1,561=1\n", "ok")
            .expect("SHUTTER:405=0,561=0\n", "ok");
        let mut dev = LdiController::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property(
            "Auto Shutter Wavelength 1",
            PropertyValue::String("405".into()),
        )
        .unwrap();
        dev.set_property(
            "Auto Shutter Wavelength 2",
            PropertyValue::String("561".into()),
        )
        .unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn shutdown_is_upstream_noop() {
        let mut dev = LdiController::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        dev.shutdown().unwrap();
        assert!(!dev.initialized);
    }

    #[test]
    fn runtime_properties_are_created_during_initialize() {
        let mut dev = LdiController::new();
        assert!(dev.has_property("Port"));
        assert!(!dev.has_property("Functional Mode"));
        assert!(!dev.has_property("Intensity Control"));

        dev = dev.with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();

        assert!(dev.has_property("Functional Mode"));
        assert!(dev.has_property("Intensity Control"));
        assert!(dev.has_property("405 Intensity"));
    }

    #[test]
    fn set_open_with_no_auto_shutter_slots_sends_no_command() {
        let mut dev = LdiController::new().with_transport(Box::new(make_init_transport()));
        dev.initialize().unwrap();
        dev.set_open(false).unwrap();
        assert!(dev.get_open().unwrap());
    }

    #[test]
    fn set_intensity() {
        let t = make_init_transport().expect("SET:488=75.0\n", "ok");
        let mut dev = LdiController::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Intensity_488nm", PropertyValue::Float(75.0))
            .unwrap();
        assert!((dev.intensities[1] - 75.0).abs() < 0.01);
    }

    #[test]
    fn upstream_intensity_property_updates_compat_alias() {
        let t = make_init_transport().expect("SET:488=25.5\n", "ok");
        let mut dev = LdiController::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("488 Intensity", PropertyValue::Float(25.5))
            .unwrap();
        assert_eq!(
            dev.get_property("Intensity_488nm").unwrap(),
            PropertyValue::Float(25.5)
        );
    }

    #[test]
    fn shutter_and_ttl_properties_use_upstream_commands() {
        let t = make_init_transport()
            .expect("SHUTTER:488=OPEN\n", "ok")
            .expect("TTL_INVERT:488=ON\n", "ok");
        let mut dev = LdiController::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("488 Shutter", PropertyValue::String("OPEN".into()))
            .unwrap();
        dev.set_property("488 TTL Inverted", PropertyValue::String("ON".into()))
            .unwrap();
    }

    #[test]
    fn initialize_does_not_query_or_set_functional_mode() {
        let t = MockTransport::new().expect("CONFIG?\n", "CONFIG:405");
        let mut dev = LdiController::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert_eq!(
            dev.get_property("Functional Mode").unwrap(),
            PropertyValue::String("IDLE".into())
        );
    }

    #[test]
    fn fire_is_unsupported() {
        let mut dev = LdiController::new();
        assert_eq!(dev.fire(5.0), Err(MmError::UnsupportedCommand));
    }

    #[test]
    fn no_transport_error() {
        assert!(LdiController::new().initialize().is_err());
    }

    #[test]
    fn initialized_port_change_is_rejected() {
        let mut dev = LdiController::new().with_transport(Box::new(make_init_transport()));
        dev.set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        dev.initialize().unwrap();
        assert_eq!(
            dev.set_property("Port", PropertyValue::String("COM2".into())),
            Err(MmError::InvalidPropertyValue)
        );
        assert_eq!(
            dev.get_property("Port").unwrap(),
            PropertyValue::String("COM1".into())
        );
    }
}
