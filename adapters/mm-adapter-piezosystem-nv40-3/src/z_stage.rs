/// Piezosystem Jena NV40/3 three-channel piezo controller, single-axis Stage.
///
/// The NV40/3 has 3 channels (1-3 externally, 0-2 internally).
/// Each channel is controlled independently. This adapter wraps one channel
/// as a Stage device.
///
/// Protocol (ASCII, `\r` terminated):
///   `\r`                  → identify; response ends with `>`
///   `cloop,<ch>\r`        → get loop mode for channel; response: `cloop,<ch>,<0|1>`
///   `cloop,<ch>,<0|1>\r`  → set loop mode (no response)
///   `rd,<ch>\r`           → read channel value; response: `rd,<ch>,<val>`
///   `wr,<ch>,<val>\r`     → write set-point (no response)
///   `setk,<ch>,<0|1>\r`   → remote control on/off for channel
///
/// Channel numbering: internal 0-2, external 1-3.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

pub struct PsjNV40_3Stage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    channel: u8,   // internal 0-2
    position_um: f64,
    loop_closed: bool,
    min_v: f64,
    max_v: f64,
    min_um: f64,
    max_um: f64,
}

impl PsjNV40_3Stage {
    /// Create a stage for `channel` (0-based internal channel, i.e. 0, 1, or 2).
    pub fn new(channel: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Channel", PropertyValue::Integer(channel as i64), false).unwrap();
        props.define_property("MinVoltage", PropertyValue::Float(0.0), false).unwrap();
        props.define_property("MaxVoltage", PropertyValue::Float(100.0), false).unwrap();
        props.define_property("MinPosition_um", PropertyValue::Float(0.0), false).unwrap();
        props.define_property("MaxPosition_um", PropertyValue::Float(100.0), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            channel,
            position_um: 0.0,
            loop_closed: false,
            min_v: 0.0,
            max_v: 100.0,
            min_um: 0.0,
            max_um: 100.0,
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

    fn parse_csv_last(resp: &str) -> MmResult<f64> {
        resp.trim()
            .split(|c: char| c == ',' || c == ' ')
            .filter(|s| !s.is_empty())
            .last()
            .and_then(|s| s.parse::<f64>().ok())
            .ok_or_else(|| MmError::LocallyDefined(format!("Cannot parse: {}", resp)))
    }

    fn voltage_to_um(&self, v: f64) -> f64 {
        (self.max_um - self.min_um) * (v - self.min_v) / (self.max_v - self.min_v) + self.min_um
    }

    fn um_to_voltage(&self, pos: f64) -> f64 {
        (self.max_v - self.min_v) * (pos - self.min_um) / (self.max_um - self.min_um) + self.min_v
    }
}

impl Default for PsjNV40_3Stage {
    fn default() -> Self { Self::new(0) }
}

impl Device for PsjNV40_3Stage {
    fn name(&self) -> &str { "PSJ-NV40-3-Stage" }
    fn description(&self) -> &str { "Piezosystem Jena NV40/3 piezo Z stage (single channel)" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        let ver = self.cmd("")?;
        if ver.is_empty() {
            return Err(MmError::LocallyDefined("No response from device".into()));
        }

        let ch = self.channel;

        // Enable remote for this channel
        let _ = self.send_only(&format!("setk,{},1", ch));

        // Get loop mode
        let loop_resp = self.cmd(&format!("cloop,{}", ch))?;
        let loop_val = Self::parse_csv_last(&loop_resp).unwrap_or(0.0) as i32;
        self.loop_closed = loop_val == 1;

        // Read current value
        let rd_resp = self.cmd(&format!("rd,{}", ch))?;
        if let Ok(raw) = Self::parse_csv_last(&rd_resp) {
            self.position_um = if self.loop_closed {
                raw
            } else {
                self.voltage_to_um(raw)
            };
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

impl Stage for PsjNV40_3Stage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let ch = self.channel;
        let set_val = if self.loop_closed { pos } else { self.um_to_voltage(pos) };
        self.send_only(&format!("wr,{},{:.3}", ch, set_val))?;
        self.position_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.position_um) }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        let new_pos = self.position_um + d;
        self.set_position_um(new_pos)
    }

    fn home(&mut self) -> MmResult<()> {
        self.set_position_um(self.min_um)
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

    fn make_transport_ch0() -> MockTransport {
        // "setk,0,1" is send_only — no script entry needed.
        MockTransport::new()
            .expect("",        "NV40/3 V1.0>")
            // setk,0,1 is send_only
            .expect("cloop,0", "cloop,0,0")   // open loop
            .expect("rd,0",    "rd,0,25.0")   // 25V
    }

    #[test]
    fn initialize_ch0() {
        let mut stage = PsjNV40_3Stage::new(0).with_transport(Box::new(make_transport_ch0()));
        stage.initialize().unwrap();
        // 25V in [0,100]V → [0,100]µm = 25µm
        let pos = stage.get_position_um().unwrap();
        assert!((pos - 25.0).abs() < 1e-6, "Expected 25µm, got {}", pos);
    }

    #[test]
    fn move_absolute() {
        // "wr,0,..." is send_only — no script entry.
        let t = make_transport_ch0();
        let mut stage = PsjNV40_3Stage::new(0).with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_position_um(60.0).unwrap();
        assert!((stage.get_position_um().unwrap() - 60.0).abs() < 1e-9);
    }

    #[test]
    fn move_relative() {
        // "wr,0,..." is send_only — no script entry.
        let t = make_transport_ch0();
        let mut stage = PsjNV40_3Stage::new(0).with_transport(Box::new(t));
        stage.initialize().unwrap();
        let init_pos = stage.get_position_um().unwrap();
        stage.set_relative_position_um(15.0).unwrap();
        assert!((stage.get_position_um().unwrap() - (init_pos + 15.0)).abs() < 1e-9);
    }

    #[test]
    fn channel_2_init() {
        // "setk,2,1" is send_only — no script entry needed.
        let t = MockTransport::new()
            .expect("",        "NV40/3 V1.0>")
            // setk,2,1 is send_only
            .expect("cloop,2", "cloop,2,0")
            .expect("rd,2",    "rd,2,75.0");
        let mut stage = PsjNV40_3Stage::new(2).with_transport(Box::new(t));
        stage.initialize().unwrap();
        assert!((stage.get_position_um().unwrap() - 75.0).abs() < 1e-6);
    }

    #[test]
    fn no_transport_error() {
        assert!(PsjNV40_3Stage::new(0).initialize().is_err());
    }
}
