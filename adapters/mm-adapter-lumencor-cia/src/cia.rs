/// Lumencor CIA (Camera Interface Adapter) shutter adapter.
///
/// Serial protocol: ASCII commands terminated with `\n`.
/// Commands:
///   "#R\n"  — Run/Start sequence (play)
///   "#S\n"  — Stop sequence
///   "#T\n"  — Step sequence
///   "#@\n"  — Rewind sequence
///   "#I\n"  — Clear/Init sequence
///   "#H\n"  — Set colour/level header, followed by 7 intensity bytes then "\n"
///   "#D\n"  — Download event data header, followed by event bytes then "\n"
///   "#E<n>\n" — Set light engine type (1=Spectra, 2=SpectraX, 3=Aura, 4=Sola)
///
/// Device responses are single-character echoes of the command prefix.
/// e.g. "#R\n" → responds "#R"
///
/// The CIA shutter open/close state is held in software only (no physical
/// shutter command) – the C++ source confirms `SetShutterPosition` just
/// stores the flag.
///
/// Colour levels correspond to channels [Violet, Blue, Cyan, Teal, Green, Yellow, Red].
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

/// Which Lumencor light engine is connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightEngine {
    Spectra = 1,
    SpectraX = 2,
    Aura = 3,
    Sola = 4,
}

/// Channel colour indices (0-based, matching CIAColorLevels array).
pub const CH_VIOLET: usize = 0;
pub const CH_BLUE: usize = 1;
pub const CH_CYAN: usize = 2;
pub const CH_TEAL: usize = 3;
pub const CH_GREEN: usize = 4;
pub const CH_YELLOW: usize = 5;
pub const CH_RED: usize = 6;

pub struct CiaShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    is_open: bool,
    light_engine: LightEngine,
    /// Channel intensity levels [0..=255] for each colour.
    color_levels: [u8; 7],
}

impl CiaShutter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "LightEngine",
                PropertyValue::String("Spectra".into()),
                false,
            )
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            is_open: false,
            light_engine: LightEngine::Spectra,
            color_levels: [0u8; 7],
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

    /// Send a text command (terminated with `\n`) and read back the response.
    fn send_cmd(&mut self, cmd: &str) -> MmResult<String> {
        let full = format!("{}\n", cmd);
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }

    /// Download colour levels to the device.
    /// Protocol: send "#H\n", then 7 raw intensity bytes, then "\n".
    pub fn download_levels(&mut self) -> MmResult<()> {
        // Send header
        let resp = self.send_cmd("#H")?;
        if !resp.contains("#H") {
            return Err(MmError::SerialInvalidResponse);
        }
        // Send 7 raw intensity bytes
        let levels = self.color_levels;
        self.call_transport(|t| t.send_bytes(&levels))?;
        // Send terminating newline
        self.call_transport(|t| t.send("\n"))?;
        Ok(())
    }

    /// Set light engine type.
    pub fn set_light_engine(&mut self, engine: LightEngine) -> MmResult<()> {
        self.light_engine = engine;
        let cmd = format!("#E{}", engine as u8);
        let resp = self.send_cmd(&cmd)?;
        if !resp.contains("#E") {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(())
    }

    /// Set a channel's intensity (0–255) and download to device.
    pub fn set_channel_level(&mut self, channel: usize, level: u8) -> MmResult<()> {
        if channel >= 7 {
            return Err(MmError::InvalidInputParam);
        }
        self.color_levels[channel] = level;
        self.download_levels()
    }
}

impl Default for CiaShutter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CiaShutter {
    fn name(&self) -> &str {
        "CIA"
    }
    fn description(&self) -> &str {
        "Lumencor Camera Interface Adapter"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Stop any running sequence
        self.send_cmd("#S")?;
        // Rewind
        self.send_cmd("#@")?;
        self.is_open = false;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send_cmd("#S");
            self.is_open = false;
            self.initialized = false;
        }
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
        DeviceType::Shutter
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Shutter for CiaShutter {
    /// CIA shutter state is software-only; open = "sequence running".
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        if open {
            self.send_cmd("#R")?;
        } else {
            self.send_cmd("#S")?;
        }
        self.is_open = open;
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        Ok(self.is_open)
    }

    fn fire(&mut self, _dt: f64) -> MmResult<()> {
        self.set_open(true)?;
        self.set_open(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_initialized() -> CiaShutter {
        // init: send "#S\n" → "#S", then "#@\n" → "#@"
        let t = MockTransport::new()
            .any("#S")
            .any("#@");
        let mut s = CiaShutter::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s
    }

    #[test]
    fn initialize_succeeds() {
        let s = make_initialized();
        assert!(s.initialized);
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn set_open_true_sends_run() {
        let mut s = make_initialized();
        s.transport = Some(Box::new(MockTransport::new().expect("#R\n", "#R")));
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
    }

    #[test]
    fn set_open_false_sends_stop() {
        let mut s = make_initialized();
        s.transport = Some(Box::new(MockTransport::new().expect("#S\n", "#S")));
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn fire_runs_then_stops() {
        let mut s = make_initialized();
        s.transport = Some(Box::new(
            MockTransport::new()
                .expect("#R\n", "#R")
                .expect("#S\n", "#S"),
        ));
        s.fire(5.0).unwrap();
        assert!(!s.get_open().unwrap());
    }

    #[test]
    fn no_transport_error() {
        assert!(CiaShutter::new().initialize().is_err());
    }

    #[test]
    fn set_channel_level() {
        let mut s = make_initialized();
        // download_levels: send "#H\n" → "#H", send bytes, send "\n"
        s.transport = Some(Box::new(MockTransport::new().any("#H")));
        s.set_channel_level(CH_BLUE, 128).unwrap();
        assert_eq!(s.color_levels[CH_BLUE], 128);
    }
}
