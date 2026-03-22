use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Shutter};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

/// Power conversion factor: device speaks Watts, we expose mW.
const POWER_CONVERSION: f64 = 1000.0;

/// Coherent Scientific Remote laser controller.
///
/// Implements `Shutter` for the currently selected laser channel (`trigger_laser`).
/// On initialize, probes lasers 1-6 and adds per-laser properties for each found.
///
/// Protocol: SCPI-like with `?` appended for queries, space-separated for sets.
pub struct CoherentScientificRemote {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    /// Index (1-6) of the laser to control with the shutter interface.
    trigger_laser: usize,
    /// Cached state for the trigger laser.
    is_open: bool,
    /// Number of lasers found during initialization.
    laser_count: usize,
}

impl CoherentScientificRemote {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("Description", PropertyValue::String(String::new()), true).unwrap();
        props.define_property("ShutterLaser", PropertyValue::Integer(1), false).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            trigger_laser: 1,
            is_open: false,
            laser_count: 0,
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

    /// Query a token (appends `?`)
    fn query(&mut self, token: &str) -> MmResult<String> {
        self.cmd(&format!("{}?", token))
    }

    /// Set a token (sends `TOKEN VALUE`)
    fn set_laser_cmd(&mut self, token: &str, value: &str) -> MmResult<String> {
        self.cmd(&format!("{} {}", token, value))
    }

    /// Replace `{laserNum}` in token string with the laser number.
    fn replace_laser_num(token: &str, laser_num: usize) -> String {
        token.replace("{laserNum}", &laser_num.to_string())
    }

    fn laser_state_token(laser_num: usize) -> String {
        Self::replace_laser_num("SOUR{laserNum}:AM:STATE", laser_num)
    }

    fn power_setpoint_token(laser_num: usize) -> String {
        Self::replace_laser_num("SOUR{laserNum}:POW:LEV:IMM:AMPL", laser_num)
    }

    fn power_max_token(laser_num: usize) -> String {
        Self::replace_laser_num("SOUR{laserNum}:POW:LIM:HIGH", laser_num)
    }

    fn power_min_token(laser_num: usize) -> String {
        Self::replace_laser_num("SOUR{laserNum}:POW:LIM:LOW", laser_num)
    }

    fn model_token(laser_num: usize) -> String {
        Self::replace_laser_num("SYST{laserNum}:INF:MOD", laser_num)
    }
}

impl Default for CoherentScientificRemote {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CoherentScientificRemote {
    fn name(&self) -> &str {
        "Coherent-Scientific Remote"
    }

    fn description(&self) -> &str {
        "CoherentScientificRemote Laser"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }

        // Check that controller is present
        let idn = self.query("*IDN")?;
        if !idn.to_lowercase().contains("coherent") {
            return Err(MmError::DeviceNotFound("Coherent Scientific Remote".into()));
        }
        self.props.entry_mut("Description")
            .map(|e| e.value = PropertyValue::String(idn));

        // Probe lasers 1-6
        let mut found = 0;
        for laser_num in 1usize..=6 {
            let model_q = Self::model_token(laser_num) + "?";
            match self.cmd(&model_q) {
                Ok(model) if !model.starts_with("ERR") && !model.is_empty() => {
                    found += 1;
                    let model = model.trim().to_string();

                    // Define per-laser properties
                    let state_prop = format!("Laser{}_State", laser_num);
                    let power_sp_prop = format!("Laser{}_PowerSetpoint_pct", laser_num);
                    let power_rb_prop = format!("Laser{}_PowerReadback_mW", laser_num);

                    self.props.define_property(
                        &state_prop,
                        PropertyValue::String("Off".into()),
                        false,
                    ).ok();
                    self.props.define_property(
                        &power_sp_prop,
                        PropertyValue::Float(0.0),
                        false,
                    ).ok();
                    self.props.set_property_limits(&power_sp_prop, 0.0, 100.0).ok();
                    self.props.define_property(
                        &power_rb_prop,
                        PropertyValue::Float(0.0),
                        true,
                    ).ok();

                    let model_prop = format!("Laser{}_Model", laser_num);
                    self.props.define_property(
                        &model_prop,
                        PropertyValue::String(model),
                        true,
                    ).ok();

                    // Read initial state
                    let state_tok = Self::laser_state_token(laser_num);
                    if let Ok(state) = self.query(&state_tok) {
                        let s = if state.to_lowercase().starts_with("on") {
                            "On"
                        } else {
                            "Off"
                        };
                        self.props.entry_mut(&state_prop)
                            .map(|e| e.value = PropertyValue::String(s.into()));
                    }

                    // If this is the first laser, set it as the trigger
                    if found == 1 {
                        self.trigger_laser = laser_num;
                    }
                }
                _ => {}
            }
        }

        if found == 0 {
            return Err(MmError::DeviceNotFound("No Coherent lasers found".into()));
        }

        self.laser_count = found;

        // Initial state was already read during the per-laser probe loop above.
        // Sync is_open from the trigger laser's property.
        let state_prop = format!("Laser{}_State", self.trigger_laser);
        if let Ok(PropertyValue::String(s)) = self.props.get(&state_prop).cloned() {
            self.is_open = s.to_lowercase().starts_with("on");
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let state_tok = Self::laser_state_token(self.trigger_laser);
            let _ = self.set_laser_cmd(&state_tok, "Off");
            self.is_open = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        // Handle per-laser state property
        if name.ends_with("_State") && name.starts_with("Laser") {
            if let Some(num_str) = name.strip_prefix("Laser").and_then(|s| s.strip_suffix("_State")) {
                if let Ok(laser_num) = num_str.parse::<usize>() {
                    let s = match &val {
                        PropertyValue::String(s) => s.clone(),
                        _ => return Err(MmError::InvalidPropertyValue),
                    };
                    if self.initialized {
                        let state_tok = Self::laser_state_token(laser_num);
                        self.set_laser_cmd(&state_tok, &s)?;
                        if laser_num == self.trigger_laser {
                            self.is_open = s == "On";
                        }
                    }
                    return self.props.set(name, PropertyValue::String(s));
                }
            }
        }

        // Handle per-laser power setpoint
        if name.ends_with("_PowerSetpoint_pct") && name.starts_with("Laser") {
            if let Some(num_str) = name.strip_prefix("Laser").and_then(|s| s.strip_suffix("_PowerSetpoint_pct")) {
                if let Ok(laser_num) = num_str.parse::<usize>() {
                    let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                    if self.initialized {
                        // Query power limits
                        let max_tok = Self::power_max_token(laser_num);
                        let min_tok = Self::power_min_token(laser_num);
                        let max_w: f64 = self.query(&max_tok).ok()
                            .and_then(|s| s.parse().ok()).unwrap_or(1.0);
                        let min_w: f64 = self.query(&min_tok).ok()
                            .and_then(|s| s.parse().ok()).unwrap_or(0.0);
                        let max_mw = max_w * POWER_CONVERSION;
                        let min_mw = min_w * POWER_CONVERSION;
                        let mw = min_mw + pct / 100.0 * (max_mw - min_mw);
                        let w = mw / POWER_CONVERSION;
                        let sp_tok = Self::power_setpoint_token(laser_num);
                        self.set_laser_cmd(&sp_tok, &format!("{:.6}", w))?;
                    }
                    return self.props.set(name, PropertyValue::Float(pct));
                }
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

impl Shutter for CoherentScientificRemote {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let state_tok = Self::laser_state_token(self.trigger_laser);
        let val = if open { "On" } else { "Off" };
        self.set_laser_cmd(&state_tok, val)?;
        self.is_open = open;
        let state_prop = format!("Laser{}_State", self.trigger_laser);
        self.props.entry_mut(&state_prop)
            .map(|e| e.value = PropertyValue::String(val.into()));
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
            .expect("*IDN?", "Coherent Scientific Remote v1.0")
            // Laser 1 present
            .expect("SYST1:INF:MOD?", "OBIS-488-50")
            .expect("SOUR1:AM:STATE?", "Off")
            // Lasers 2-6 not present
            .expect("SYST2:INF:MOD?", "ERR")
            .expect("SYST3:INF:MOD?", "ERR")
            .expect("SYST4:INF:MOD?", "ERR")
            .expect("SYST5:INF:MOD?", "ERR")
            .expect("SYST6:INF:MOD?", "ERR")
    }

    #[test]
    fn initialize_finds_laser() {
        let mut dev = CoherentScientificRemote::new().with_transport(Box::new(make_transport()));
        dev.initialize().unwrap();
        assert!(!dev.get_open().unwrap());
        assert_eq!(dev.laser_count, 1);
        assert_eq!(dev.trigger_laser, 1);
        assert_eq!(
            dev.get_property("Laser1_Model").unwrap(),
            PropertyValue::String("OBIS-488-50".into())
        );
    }

    #[test]
    fn open_close_laser() {
        let t = make_transport()
            .expect("SOUR1:AM:STATE On", "On")
            .expect("SOUR1:AM:STATE Off", "Off");
        let mut dev = CoherentScientificRemote::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn no_coherent_response_fails() {
        let t = MockTransport::new().expect("*IDN?", "SomeOtherDevice");
        let mut dev = CoherentScientificRemote::new().with_transport(Box::new(t));
        assert!(dev.initialize().is_err());
    }

    #[test]
    fn no_transport_error() {
        let mut dev = CoherentScientificRemote::new();
        assert!(dev.initialize().is_err());
    }
}
