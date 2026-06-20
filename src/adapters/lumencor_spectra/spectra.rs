/// Lumencor Spectra/Aura/Sola/SpectraX LED light engine (legacy serial binary protocol).
///
/// Protocol: pure-write binary, no responses.
///
/// Init sequence (sent once on initialize):
///   `[0x57, 0x02, 0xFF, 0x50]`  — enable TTL mode
///   `[0x57, 0x03, 0xAB, 0x50]`  — set DAC control
///
/// Enable mask (active-low, bit = 0 means channel ON):
///   `[0x4F, mask, 0x50]`
///   All-off mask: 0x7F (bits 0-6 set = all channels disabled)
///   All-on  mask: 0x10 (Spectra/SpectraX shuttered bits enabled, YG filter high)
///
/// Channel bit mapping (active-low):
///   bit 0 = RED, bit 1 = GREEN, bit 2 = CYAN, bit 3 = VIOLET,
///   bit 4 = YG_FILTER, bit 5 = BLUE, bit 6 = TEAL
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

const INIT_CMD_1: [u8; 4] = [0x57, 0x02, 0xFF, 0x50];
const INIT_CMD_2: [u8; 4] = [0x57, 0x03, 0xAB, 0x50];
const ALL_OFF_MASK: u8 = 0x7F;
const ALL_ON_MASK: u8 = 0x10;

pub struct LumencorSpectra {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    open: bool,
}

impl LumencorSpectra {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("State", &["0", "1"]).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
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

    fn send_mask(&mut self, mask: u8) -> MmResult<()> {
        self.call_transport(|t| t.send_bytes(&[0x4F, mask, 0x50]))
    }
}

impl Default for LumencorSpectra {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for LumencorSpectra {
    fn name(&self) -> &str {
        "Spectra"
    }
    fn description(&self) -> &str {
        "Lumencor Spectra Light Engine"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.call_transport(|t| t.send_bytes(&INIT_CMD_1))?;
        self.call_transport(|t| t.send_bytes(&INIT_CMD_2))?;
        self.send_mask(ALL_OFF_MASK)?;
        self.open = false;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::Integer(if self.open { 1 } else { 0 })),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                match state {
                    0 => self.set_open(false),
                    1 => self.set_open(true),
                    _ => Err(MmError::InvalidPropertyValue),
                }
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

impl Shutter for LumencorSpectra {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let mask = if open { ALL_ON_MASK } else { ALL_OFF_MASK };
        self.send_mask(mask)?;
        self.open = open;
        self.props
            .set("State", PropertyValue::Integer(if open { 1 } else { 0 }))?;
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

    #[test]
    fn initialize() {
        let mut s = LumencorSpectra::new().with_transport(Box::new(MockTransport::new()));
        s.initialize().unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn open_close() {
        let mut s = LumencorSpectra::new().with_transport(Box::new(MockTransport::new()));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn fire() {
        let mut s = LumencorSpectra::new().with_transport(Box::new(MockTransport::new()));
        s.initialize().unwrap();
        assert_eq!(s.fire(10.0), Err(MmError::UnsupportedCommand));
    }

    #[test]
    fn no_transport_error() {
        assert!(LumencorSpectra::new().initialize().is_err());
    }
}
