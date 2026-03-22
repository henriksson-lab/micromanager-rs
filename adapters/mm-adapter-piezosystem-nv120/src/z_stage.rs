/// Piezosystem Jena NV120/1CL single-channel piezo Z stage.
///
/// Protocol (ASCII, `\r` terminated):
///   `\r`                  → identify device; response ends with `>\r`
///   `dspclmin,2\r`        → get min position in µm; response: `dspclmin,<ch>,<val>`
///   `dspclmax,2\r`        → get max position in µm; response: `dspclmax,<ch>,<val>`
///   `dspvmin\r`           → get min voltage; response: `dspvmin,<val>`
///   `dspvmax\r`           → get max voltage; response: `dspvmax,<val>`
///   `cloop\r`             → get loop mode; response: `cloop <val>` (0=open,1=closed)
///   `cloop,<0|1>\r`       → set loop mode
///   `rk\r`                → read position (µm in closed loop, voltage in open)
///   `set,<val>\r`         → set position (closed loop, µm) - no response
///   `stat\r`              → get status; response: `stat,<val>`
///   `setk,<0|1>\r`        → set remote control (1=on, 0=off)
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

pub struct PsjNV120Stage {
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

impl PsjNV120Stage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            position_um: 0.0,
            loop_closed: false,
            min_v: -20.0,
            max_v: 130.0,
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

    /// Parse `<key>,<ch>,<value>` or `<key>,<value>` → last numeric token.
    fn parse_csv_last(resp: &str) -> MmResult<f64> {
        resp.trim()
            .split(',')
            .last()
            .and_then(|s| s.trim().split_whitespace().next())
            .and_then(|s| s.parse::<f64>().ok())
            .ok_or_else(|| MmError::LocallyDefined(format!("Cannot parse: {}", resp)))
    }

    fn voltage_to_um(&self, v: f64) -> f64 {
        (self.max_um - self.min_um) * (v - self.min_v) / (self.max_v - self.min_v) + self.min_um
    }

    fn um_to_voltage(&self, pos: f64) -> f64 {
        (self.max_v - self.min_v) * (pos - self.min_um) / (self.max_um - self.min_um) + self.min_v
    }

    fn get_limits_from_device(&mut self) -> MmResult<()> {
        let r = self.cmd("dspclmin,2")?;
        self.min_um = Self::parse_csv_last(&r)?;
        let r = self.cmd("dspclmax,2")?;
        self.max_um = Self::parse_csv_last(&r)?;
        let r = self.cmd("dspvmin")?;
        self.min_v = Self::parse_csv_last(&r)?;
        let r = self.cmd("dspvmax")?;
        self.max_v = Self::parse_csv_last(&r)?;
        Ok(())
    }
}

impl Default for PsjNV120Stage {
    fn default() -> Self { Self::new() }
}

impl Device for PsjNV120Stage {
    fn name(&self) -> &str { "PSJ-NV120-Stage" }
    fn description(&self) -> &str { "Piezosystem Jena NV120/1CL piezo Z stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Identify
        let ver = self.cmd("")?;
        if ver.is_empty() {
            return Err(MmError::LocallyDefined("No response from device".into()));
        }

        // Get limits
        self.get_limits_from_device()?;

        // Enable remote
        let _ = self.send_only("setk,1");

        // Get loop mode
        let loop_resp = self.cmd("cloop")?;
        // response is "cloop <val>" or "cloop,<val>"
        let loop_val: i32 = loop_resp
            .split(|c: char| c == ',' || c == ' ')
            .filter_map(|s| s.trim().parse::<i32>().ok())
            .last()
            .unwrap_or(0);
        self.loop_closed = loop_val == 1;

        // Read current position
        let rk_resp = self.cmd("rk")?;
        if let Ok(raw) = Self::parse_csv_last(&rk_resp) {
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

impl Stage for PsjNV120Stage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let set_val = if self.loop_closed {
            pos
        } else {
            self.um_to_voltage(pos)
        };
        let cmd = if self.loop_closed {
            format!("set,{:.3}", set_val)
        } else {
            format!("set,{:.3}", set_val)
        };
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

    fn make_transport() -> MockTransport {
        // "setk,1" is send_only — no script entry.
        MockTransport::new()
            .expect("",           "NV120CLE V1.0>")
            .expect("dspclmin,2", "dspclmin,2,0.0")
            .expect("dspclmax,2", "dspclmax,2,100.0")
            .expect("dspvmin",    "dspvmin,-20")
            .expect("dspvmax",    "dspvmax,130")
            // setk,1 is send_only
            .expect("cloop",      "cloop 0")   // open loop
            .expect("rk",         "rk,65.0")  // voltage
    }

    #[test]
    fn initialize() {
        let mut stage = PsjNV120Stage::new().with_transport(Box::new(make_transport()));
        stage.initialize().unwrap();
        // voltage 65.0 in open loop → um = (100-0)*(65-(-20))/(130-(-20)) + 0 = 100*85/150 ≈ 56.67
        let pos = stage.get_position_um().unwrap();
        assert!(pos > 50.0 && pos < 60.0, "Expected ~56.67 µm, got {}", pos);
    }

    #[test]
    fn limits_read_from_device() {
        let mut stage = PsjNV120Stage::new().with_transport(Box::new(make_transport()));
        stage.initialize().unwrap();
        let (min, max) = stage.get_limits().unwrap();
        assert_eq!(min, 0.0);
        assert_eq!(max, 100.0);
    }

    #[test]
    fn move_absolute() {
        let t = make_transport().any("");
        let mut stage = PsjNV120Stage::new().with_transport(Box::new(t));
        stage.initialize().unwrap();
        stage.set_position_um(50.0).unwrap();
        assert!((stage.get_position_um().unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn no_transport_error() {
        assert!(PsjNV120Stage::new().initialize().is_err());
    }
}
