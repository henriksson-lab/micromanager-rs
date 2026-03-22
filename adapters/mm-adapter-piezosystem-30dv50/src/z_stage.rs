/// Piezosystem Jena 30DV50 single-channel piezo Z stage.
///
/// Protocol (ASCII, `\r` terminated, CR+LF for receive):
///   `\r`              → get version / identify (empty command); response ends with `>`
///   `stat\r`          → status query; response: `stat,<value>`
///   `cloop\r`         → get loop mode; response: `cloop,<0|1>`
///   `cloop,0\r`       → set open loop
///   `cloop,1\r`       → set close loop
///   `rd\r`            → read current value (voltage in open loop, µm in closed)
///   `wr,<val>\r`      → write set-point value (no response)
///   `set,<val>\r`     → set position (closed loop); no response
///   `rohm\r`          → query resistance
///   (limits from pre-init properties: min_V_, max_V_, min_um_, max_um_)
///
/// The C++ adapter (Piezosystem_30DV50.cpp) uses `rd`/`wr` commands,
/// and converts between voltage and position using linear interpolation.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

pub struct Psj30DV50Stage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position_um: f64,
    loop_closed: bool,
    min_v: f64,
    max_v: f64,
    min_um: f64,
    max_um: f64,
}

impl Psj30DV50Stage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("MinVoltage", PropertyValue::Float(-20.0), false).unwrap();
        props.define_property("MaxVoltage", PropertyValue::Float(130.0), false).unwrap();
        props.define_property("MinPosition_um", PropertyValue::Float(0.0), false).unwrap();
        props.define_property("MaxPosition_um", PropertyValue::Float(80.0), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            position_um: 0.0,
            loop_closed: false,
            min_v: -20.0,
            max_v: 130.0,
            min_um: 0.0,
            max_um: 80.0,
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
        self.call_transport(|t| Ok(t.send_recv(&cmd)?.trim().to_string()))
    }

    fn send_only(&mut self, command: &str) -> MmResult<()> {
        let cmd = command.to_string();
        self.call_transport(|t| { t.send(&cmd)?; Ok(()) })
    }

    /// Parse `<key>,<value>` response → value as f64.
    fn parse_value(resp: &str) -> MmResult<f64> {
        let resp = resp.trim();
        let parts: Vec<&str> = resp.splitn(2, ',').collect();
        if parts.len() < 2 {
            return Err(MmError::LocallyDefined(format!("Cannot parse: {}", resp)));
        }
        parts[1].trim().parse::<f64>()
            .map_err(|_| MmError::LocallyDefined(format!("Non-numeric value: {}", parts[1])))
    }

    fn voltage_to_um(&self, v: f64) -> f64 {
        (self.max_um - self.min_um) * (v - self.min_v) / (self.max_v - self.min_v) + self.min_um
    }
}

impl Default for Psj30DV50Stage {
    fn default() -> Self { Self::new() }
}

impl Device for Psj30DV50Stage {
    fn name(&self) -> &str { "PSJ-30DV50-Stage" }
    fn description(&self) -> &str { "Piezosystem Jena 30DV50 piezo Z stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Identify device
        let ver = self.cmd("")?;
        if ver.is_empty() {
            return Err(MmError::LocallyDefined("No response from device".into()));
        }

        // Get loop mode
        let loop_resp = self.cmd("cloop")?;
        if let Ok(v) = Self::parse_value(&loop_resp) {
            self.loop_closed = v as i32 == 1;
        }

        // Read current position
        let rd_resp = self.cmd("rd")?;
        if let Ok(raw) = Self::parse_value(&rd_resp) {
            if self.loop_closed {
                self.position_um = raw;
            } else {
                self.position_um = self.voltage_to_um(raw);
            }
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
        self.props.set(name, val)
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Stage }
    fn busy(&self) -> bool { false }
}

impl Stage for Psj30DV50Stage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let set_value = if self.loop_closed {
            pos
        } else {
            // convert µm to voltage
            (self.max_v - self.min_v) * (pos - self.min_um) / (self.max_um - self.min_um) + self.min_v
        };
        let cmd = format!("wr,{:.3}", set_value);
        self.send_only(&cmd)?;
        self.position_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.position_um) }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let new_pos = self.position_um + d;
        self.set_position_um(new_pos)
    }

    fn home(&mut self) -> MmResult<()> {
        self.set_position_um(0.0)
    }

    fn stop(&mut self) -> MmResult<()> { Ok(()) }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((self.min_um, self.max_um)) }
    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("",       "30DV50 V1.0>")
            .expect("cloop",  "cloop,0")      // open loop
            .expect("rd",     "rd,65.0")      // raw voltage
    }

    #[test]
    fn initialize() {
        let mut stage = Psj30DV50Stage::new().with_transport(Box::new(make_transport()));
        stage.initialize().unwrap();
        // voltage 65.0 → position: (80-0)*(65-(-20))/(130-(-20)) + 0 = 80*85/150 ≈ 45.33
        let pos = stage.get_position_um().unwrap();
        assert!(pos > 40.0 && pos < 50.0, "Expected ~45 µm, got {}", pos);
    }

    #[test]
    fn move_absolute_open_loop() {
        let t = make_transport()
            .any(""); // wr command has no response
        let mut stage = Psj30DV50Stage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_position_um(40.0).unwrap();
        assert!((stage.get_position_um().unwrap() - 40.0).abs() < 1e-9);
    }

    #[test]
    fn move_relative() {
        let t = make_transport().any("");
        let mut stage = Psj30DV50Stage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        let init_pos = stage.get_position_um().unwrap();
        stage.set_relative_position_um(5.0).unwrap();
        assert!((stage.get_position_um().unwrap() - (init_pos + 5.0)).abs() < 1e-9);
    }

    #[test]
    fn limits() {
        let mut stage = Psj30DV50Stage::new().with_transport(Box::new(make_transport()));
        stage.initialize().unwrap();
        let (min, max) = stage.get_limits().unwrap();
        assert_eq!(min, 0.0);
        assert_eq!(max, 80.0);
    }

    #[test]
    fn no_transport_error() {
        assert!(Psj30DV50Stage::new().initialize().is_err());
    }
}
