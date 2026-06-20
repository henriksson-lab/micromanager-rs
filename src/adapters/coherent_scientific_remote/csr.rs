use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};
use std::time::Instant;

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
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    /// Index (1-6) of the laser to control with the shutter interface.
    trigger_laser: usize,
    /// Cached state for the trigger laser.
    is_open: Cell<bool>,
    /// Number of lasers found during initialization.
    laser_count: usize,
    /// Connected laser numbers and their model labels, used by the upstream
    /// "Laser <model> - <property>" naming scheme.
    laser_models: Vec<(usize, String)>,
    delay_ms: f64,
    changed_time: Cell<Instant>,
}

impl CoherentScientificRemote {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Description", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("ShutterLaser", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .define_property("Shutter Laser", PropertyValue::String("None".into()), false)
            .unwrap();
        props
            .define_property("Delay_ms", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .set_property_limits("Delay_ms", 0.0, f64::MAX)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            trigger_laser: 1,
            is_open: Cell::new(false),
            laser_count: 0,
            laser_models: Vec::new(),
            delay_ms: 0.0,
            changed_time: Cell::new(Instant::now()),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(RefCell::new(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_ref() {
            Some(t) => f(t.borrow_mut().as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let cmd = command.to_string();
        self.call_transport(|t| {
            let resp = t.send_recv(&cmd)?;
            Ok(resp.trim().to_string())
        })
    }

    /// Query a token (appends `?`)
    fn query(&self, token: &str) -> MmResult<String> {
        self.cmd(&format!("{}?", token))
    }

    /// Set a token (sends `TOKEN VALUE`)
    fn set_laser_cmd(&self, token: &str, value: &str) -> MmResult<String> {
        self.cmd(&format!("{} {}", token, value))?;
        self.query(token)
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

    fn power_readback_token(laser_num: usize) -> String {
        Self::power_setpoint_token(laser_num)
    }

    fn model_token(laser_num: usize) -> String {
        Self::replace_laser_num("SYST{laserNum}:INF:MOD", laser_num)
    }

    fn serial_token(laser_num: usize) -> String {
        Self::replace_laser_num("SYST{laserNum}:INF:SNUM", laser_num)
    }

    fn usage_hours_token(laser_num: usize) -> String {
        Self::replace_laser_num("SYST{laserNum}:DIOD:HOUR", laser_num)
    }

    fn wavelength_token(laser_num: usize) -> String {
        Self::replace_laser_num("SYST{laserNum}:INF:WAV", laser_num)
    }

    fn modulation_readback_token(laser_num: usize) -> String {
        Self::replace_laser_num("SOUR{laserNum}:AM:SOUR", laser_num)
    }

    fn modulation_internal_token(laser_num: usize) -> String {
        Self::replace_laser_num("SOUR{laserNum}:AM:INT", laser_num)
    }

    fn modulation_external_token(laser_num: usize) -> String {
        Self::replace_laser_num("SOUR{laserNum}:AM:EXT", laser_num)
    }

    fn temperature_base_token(laser_num: usize) -> String {
        Self::replace_laser_num("SOUR{laserNum}:TEMP:BAS", laser_num)
    }

    fn handshaking_token(laser_num: usize) -> String {
        Self::replace_laser_num("SYST{laserNum}:COMM:HAND", laser_num)
    }

    fn prompt_token(laser_num: usize) -> String {
        Self::replace_laser_num("SYST{laserNum}:COMM:PROM", laser_num)
    }

    fn clear_error_token(laser_num: usize) -> String {
        Self::replace_laser_num("SYST{laserNum}:ERR:CLE", laser_num)
    }

    fn property_laser_num(name: &str, suffix: &str) -> Option<usize> {
        name.strip_prefix("Laser")
            .and_then(|s| s.strip_suffix(suffix))
            .and_then(|s| s.parse::<usize>().ok())
    }

    fn upstream_property_laser_num(&self, name: &str, suffix: &str) -> Option<usize> {
        let model_part = name
            .strip_prefix("Laser ")
            .and_then(|s| s.strip_suffix(suffix))?;
        self.laser_models
            .iter()
            .find_map(|(num, model)| (model == model_part).then_some(*num))
    }

    fn upstream_property_name(model: &str, suffix: &str) -> String {
        format!("Laser {} - {}", model, suffix)
    }

    fn trigger_label(num: usize, model: &str) -> String {
        format!("{} ({})", num, model)
    }

    fn trigger_label_for(&self, laser_num: usize) -> Option<String> {
        self.laser_models
            .iter()
            .find_map(|(num, model)| (*num == laser_num).then(|| Self::trigger_label(*num, model)))
    }

    fn parse_trigger_label(value: &str) -> Option<usize> {
        if value == "None" {
            return Some(0);
        }
        value
            .split_once(' ')
            .and_then(|(num, _)| num.parse::<usize>().ok())
    }

    fn parse_float_query(&self, token: &str) -> MmResult<f64> {
        self.query(token)?
            .parse::<f64>()
            .map_err(|_| MmError::SerialInvalidResponse)
    }

    fn query_mw(&self, token: &str) -> MmResult<f64> {
        Ok(self.parse_float_query(token)? * POWER_CONVERSION)
    }

    fn query_state_bool(&self, laser_num: usize) -> MmResult<bool> {
        let state = self.query(&Self::laser_state_token(laser_num))?;
        let state = state.to_lowercase();
        if state.starts_with("on") {
            Ok(true)
        } else if state.starts_with("off") {
            Ok(false)
        } else {
            Err(MmError::SerialInvalidResponse)
        }
    }

    fn query_power_percent(&self, laser_num: usize) -> MmResult<f64> {
        let mw = self
            .query(&Self::power_setpoint_token(laser_num))?
            .parse::<f64>()
            .map_err(|_| MmError::SerialInvalidResponse)?
            * POWER_CONVERSION;
        let max_mw = self
            .query(&Self::power_max_token(laser_num))?
            .parse::<f64>()
            .map_err(|_| MmError::SerialInvalidResponse)?
            * POWER_CONVERSION;
        let min_mw = self
            .query(&Self::power_min_token(laser_num))?
            .parse::<f64>()
            .map_err(|_| MmError::SerialInvalidResponse)?
            * POWER_CONVERSION;
        Ok(100.0 * (mw - min_mw) / (max_mw - min_mw))
    }

    fn define_upstream_laser_properties(&mut self, laser_num: usize, model: &str) {
        let state_prop = Self::upstream_property_name(model, "State");
        self.props
            .define_property(&state_prop, PropertyValue::String("Off".into()), false)
            .ok();
        self.props
            .set_allowed_values(&state_prop, &["Off", "On"])
            .ok();

        let modulation_prop = Self::upstream_property_name(model, "Modulation/Trigger");
        self.props
            .define_property(
                &modulation_prop,
                PropertyValue::String("CW (constant power)".into()),
                false,
            )
            .ok();
        self.props
            .set_allowed_values(
                &modulation_prop,
                &[
                    "CW (constant power)",
                    "CW (constant current)",
                    "External/Digital",
                    "External/Analog",
                    "External/Mixed",
                ],
            )
            .ok();

        let power_setpoint = Self::upstream_property_name(model, "PowerSetpoint (%)");
        self.props
            .define_property(&power_setpoint, PropertyValue::Float(0.0), false)
            .ok();
        self.props
            .set_property_limits(&power_setpoint, 0.0, 100.0)
            .ok();

        for (suffix, default) in [
            ("PowerReadback (mW)", PropertyValue::Float(0.0)),
            ("Head Usage (h)", PropertyValue::Float(0.0)),
            ("Port", PropertyValue::String(laser_num.to_string())),
            ("Minimum Laser Power (mW)", PropertyValue::Float(0.0)),
            ("Maximum Laser Power (mW)", PropertyValue::Float(0.0)),
            ("Wavelength (nm)", PropertyValue::Float(0.0)),
            ("Temperature Baseplate (C)", PropertyValue::Float(0.0)),
            ("Head ID", PropertyValue::String(String::new())),
        ] {
            self.props
                .define_property(Self::upstream_property_name(model, suffix), default, true)
                .ok();
        }
    }

    fn query_modulation(&self, laser_num: usize) -> MmResult<String> {
        let ans = self
            .query(&Self::modulation_readback_token(laser_num))?
            .to_lowercase();
        Ok(if ans.starts_with("cwp") {
            "CW (constant power)"
        } else if ans.starts_with("cwc") {
            "CW (constant current)"
        } else if ans.starts_with("digital") {
            "External/Digital"
        } else if ans.starts_with("analog") {
            "External/Analog"
        } else if ans.starts_with("mixed") {
            "External/Mixed"
        } else {
            self.set_laser_cmd(&Self::modulation_internal_token(laser_num), "CWP")?;
            "CW (constant power)"
        }
        .into())
    }

    fn set_modulation(&self, laser_num: usize, value: &str) -> MmResult<()> {
        match value {
            "CW (constant power)" => {
                self.set_laser_cmd(&Self::modulation_internal_token(laser_num), "CWP")?;
            }
            "CW (constant current)" => {
                self.set_laser_cmd(&Self::modulation_internal_token(laser_num), "CWC")?;
            }
            "External/Digital" => {
                self.set_laser_cmd(&Self::modulation_external_token(laser_num), "DIG")?;
            }
            "External/Analog" => {
                self.set_laser_cmd(&Self::modulation_external_token(laser_num), "ANAL")?;
            }
            "External/Mixed" => {
                self.set_laser_cmd(&Self::modulation_external_token(laser_num), "MIX")?;
            }
            _ => return Err(MmError::InvalidPropertyValue),
        }
        Ok(())
    }

    fn set_power_percent(&self, laser_num: usize, pct: f64) -> MmResult<()> {
        let max_w = self.parse_float_query(&Self::power_max_token(laser_num))?;
        let min_w = self.parse_float_query(&Self::power_min_token(laser_num))?;
        let max_mw = max_w * POWER_CONVERSION;
        let min_mw = min_w * POWER_CONVERSION;
        let mw = min_mw + pct / 100.0 * (max_mw - min_mw);
        self.set_laser_cmd(
            &Self::power_setpoint_token(laser_num),
            &format!("{:.6}", mw / POWER_CONVERSION),
        )?;
        Ok(())
    }

    fn get_upstream_laser_property(&self, name: &str) -> Option<MmResult<PropertyValue>> {
        if let Some(laser_num) = self.upstream_property_laser_num(name, " - State") {
            return Some(Ok(PropertyValue::String(
                if match self.query_state_bool(laser_num) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                } {
                    "On"
                } else {
                    "Off"
                }
                .into(),
            )));
        }
        if let Some(laser_num) = self.upstream_property_laser_num(name, " - PowerSetpoint (%)") {
            return Some(
                self.query_power_percent(laser_num)
                    .map(PropertyValue::Float),
            );
        }
        if let Some(laser_num) = self.upstream_property_laser_num(name, " - PowerReadback (mW)") {
            return Some(Ok(PropertyValue::Float(
                match self.query_mw(&Self::power_readback_token(laser_num)) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                },
            )));
        }
        if let Some(laser_num) = self.upstream_property_laser_num(name, " - Modulation/Trigger") {
            return Some(self.query_modulation(laser_num).map(PropertyValue::String));
        }
        if let Some(laser_num) = self.upstream_property_laser_num(name, " - Head Usage (h)") {
            return Some(Ok(PropertyValue::Float(
                match self.parse_float_query(&Self::usage_hours_token(laser_num)) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                },
            )));
        }
        if let Some(laser_num) = self.upstream_property_laser_num(name, " - Port") {
            return Some(Ok(PropertyValue::String(laser_num.to_string())));
        }
        if let Some(laser_num) =
            self.upstream_property_laser_num(name, " - Minimum Laser Power (mW)")
        {
            return Some(Ok(PropertyValue::Float(
                match self.query_mw(&Self::power_min_token(laser_num)) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                },
            )));
        }
        if let Some(laser_num) =
            self.upstream_property_laser_num(name, " - Maximum Laser Power (mW)")
        {
            return Some(Ok(PropertyValue::Float(
                match self.query_mw(&Self::power_max_token(laser_num)) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                },
            )));
        }
        if let Some(laser_num) = self.upstream_property_laser_num(name, " - Wavelength (nm)") {
            return Some(Ok(PropertyValue::Float(
                match self.parse_float_query(&Self::wavelength_token(laser_num)) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                },
            )));
        }
        if let Some(laser_num) =
            self.upstream_property_laser_num(name, " - Temperature Baseplate (C)")
        {
            return Some(Ok(PropertyValue::Float(
                match self.parse_float_query(&Self::temperature_base_token(laser_num)) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                },
            )));
        }
        if let Some(laser_num) = self.upstream_property_laser_num(name, " - Head ID") {
            return Some(Ok(PropertyValue::String(
                match self.query(&Self::serial_token(laser_num)) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                },
            )));
        }
        None
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
        self.props
            .entry_mut("Description")
            .map(|e| e.value = PropertyValue::String(idn));

        // Probe lasers 1-6
        let mut found = 0;
        let mut trigger_values = vec!["None".to_string()];
        self.laser_models.clear();
        for laser_num in 1usize..=6 {
            let model_q = Self::model_token(laser_num) + "?";
            match self.cmd(&model_q) {
                Ok(model) if !model.starts_with("ERR") && !model.is_empty() => {
                    found += 1;
                    let model = model.trim().to_string();
                    self.laser_models.push((laser_num, model.clone()));

                    let _ = self.set_laser_cmd(&Self::handshaking_token(laser_num), "On");
                    let _ = self.set_laser_cmd(&Self::prompt_token(laser_num), "Off");
                    let _ = self.query(&Self::clear_error_token(laser_num));

                    // Define per-laser properties
                    let state_prop = format!("Laser{}_State", laser_num);
                    let power_sp_prop = format!("Laser{}_PowerSetpoint_pct", laser_num);
                    let power_rb_prop = format!("Laser{}_PowerReadback_mW", laser_num);
                    self.define_upstream_laser_properties(laser_num, &model);

                    self.props
                        .define_property(&state_prop, PropertyValue::String("Off".into()), false)
                        .ok();
                    self.props
                        .define_property(&power_sp_prop, PropertyValue::Float(0.0), false)
                        .ok();
                    self.props
                        .set_property_limits(&power_sp_prop, 0.0, 100.0)
                        .ok();
                    self.props
                        .define_property(&power_rb_prop, PropertyValue::Float(0.0), true)
                        .ok();

                    let model_prop = format!("Laser{}_Model", laser_num);
                    self.props
                        .define_property(&model_prop, PropertyValue::String(model.clone()), true)
                        .ok();

                    // Read initial state
                    let state_tok = Self::laser_state_token(laser_num);
                    if let Ok(state) = self.query(&state_tok) {
                        let s = if state.to_lowercase().starts_with("on") {
                            "On"
                        } else {
                            "Off"
                        };
                        self.props
                            .entry_mut(&state_prop)
                            .map(|e| e.value = PropertyValue::String(s.into()));
                    }

                    // If this is the first laser, set it as the trigger
                    if found == 1 {
                        self.trigger_laser = laser_num;
                    }
                    trigger_values.push(Self::trigger_label(laser_num, &model));
                }
                _ => {}
            }
        }

        if found == 0 {
            return Err(MmError::DeviceNotFound("No Coherent lasers found".into()));
        }

        self.laser_count = found;
        let trigger_label = self.trigger_label_for(self.trigger_laser);
        if let Some(entry) = self.props.entry_mut("Shutter Laser") {
            entry.allowed_values = trigger_values;
            if let Some(label) = trigger_label {
                entry.value = PropertyValue::String(label);
            }
        }

        // Initial state was already read during the per-laser probe loop above.
        // Sync is_open from the trigger laser's property.
        let state_prop = format!("Laser{}_State", self.trigger_laser);
        if let Ok(PropertyValue::String(s)) = self.props.get(&state_prop).cloned() {
            self.is_open.set(s.to_lowercase().starts_with("on"));
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            self.is_open.set(false);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if self.initialized {
            if let Some(laser_num) = Self::property_laser_num(name, "_State") {
                return Ok(PropertyValue::String(
                    if self.query_state_bool(laser_num)? {
                        "On"
                    } else {
                        "Off"
                    }
                    .into(),
                ));
            }
            if let Some(laser_num) = Self::property_laser_num(name, "_PowerReadback_mW") {
                let mw = self
                    .query(&Self::power_setpoint_token(laser_num))?
                    .parse::<f64>()
                    .map_err(|_| MmError::SerialInvalidResponse)?
                    * POWER_CONVERSION;
                return Ok(PropertyValue::Float(mw));
            }
            if let Some(laser_num) = Self::property_laser_num(name, "_PowerSetpoint_pct") {
                return Ok(PropertyValue::Float(self.query_power_percent(laser_num)?));
            }
            if let Some(value) = self.get_upstream_laser_property(name) {
                return value;
            }
        }
        if name == "Delay_ms" {
            return Ok(PropertyValue::Float(self.delay_ms));
        }
        if name == "Shutter Laser" && self.initialized {
            return Ok(PropertyValue::String(
                self.trigger_label_for(self.trigger_laser)
                    .unwrap_or_else(|| "None".into()),
            ));
        }
        if name == "ShutterLaser" {
            return Ok(PropertyValue::Integer(self.trigger_laser as i64));
        }
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Port" && self.initialized {
            return Err(MmError::InvalidPropertyValue);
        }

        // Handle per-laser state property
        if name.ends_with("_State") && name.starts_with("Laser") {
            if let Some(num_str) = name
                .strip_prefix("Laser")
                .and_then(|s| s.strip_suffix("_State"))
            {
                if let Ok(laser_num) = num_str.parse::<usize>() {
                    let s = match &val {
                        PropertyValue::String(s) => s.clone(),
                        _ => return Err(MmError::InvalidPropertyValue),
                    };
                    if self.initialized {
                        let state_tok = Self::laser_state_token(laser_num);
                        self.set_laser_cmd(&state_tok, &s)?;
                        if laser_num == self.trigger_laser {
                            self.is_open.set(s == "On");
                            self.changed_time.set(Instant::now());
                        }
                    }
                    return self.props.set(name, PropertyValue::String(s));
                }
            }
        }

        // Handle per-laser power setpoint
        if name.ends_with("_PowerSetpoint_pct") && name.starts_with("Laser") {
            if let Some(num_str) = name
                .strip_prefix("Laser")
                .and_then(|s| s.strip_suffix("_PowerSetpoint_pct"))
            {
                if let Ok(laser_num) = num_str.parse::<usize>() {
                    let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                    if self.initialized {
                        self.set_power_percent(laser_num, pct)?;
                    }
                    return self.props.set(name, PropertyValue::Float(pct));
                }
            }
        }

        if let Some(laser_num) = self.upstream_property_laser_num(name, " - State") {
            let s = match &val {
                PropertyValue::String(s) => s.clone(),
                _ => return Err(MmError::InvalidPropertyValue),
            };
            if self.initialized {
                self.set_laser_cmd(&Self::laser_state_token(laser_num), &s)?;
                if laser_num == self.trigger_laser {
                    self.is_open.set(s == "On");
                    self.changed_time.set(Instant::now());
                }
            }
            return self.props.set(name, PropertyValue::String(s));
        }

        if let Some(laser_num) = self.upstream_property_laser_num(name, " - PowerSetpoint (%)") {
            let pct = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            if self.initialized {
                self.set_power_percent(laser_num, pct)?;
            }
            return self.props.set(name, PropertyValue::Float(pct));
        }

        if let Some(laser_num) = self.upstream_property_laser_num(name, " - Modulation/Trigger") {
            let s = match &val {
                PropertyValue::String(s) => s.clone(),
                _ => return Err(MmError::InvalidPropertyValue),
            };
            if self.initialized {
                self.set_modulation(laser_num, &s)?;
            }
            return self.props.set(name, PropertyValue::String(s));
        }

        if name == "Shutter Laser" {
            let s = match &val {
                PropertyValue::String(s) => s.clone(),
                _ => return Err(MmError::InvalidPropertyValue),
            };
            let laser_num = Self::parse_trigger_label(&s).ok_or(MmError::InvalidPropertyValue)?;
            if self.initialized
                && laser_num > 0
                && !self.laser_models.iter().any(|(num, _)| *num == laser_num)
            {
                return Err(MmError::InvalidPropertyValue);
            }
            self.trigger_laser = laser_num;
            self.is_open.set(false);
            self.props.set(name, PropertyValue::String(s))?;
            let _ = self
                .props
                .set("ShutterLaser", PropertyValue::Integer(laser_num as i64));
            return Ok(());
        }

        if name == "ShutterLaser" {
            let laser_num = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as usize;
            if self.initialized
                && laser_num > 0
                && !self.laser_models.iter().any(|(num, _)| *num == laser_num)
            {
                return Err(MmError::InvalidPropertyValue);
            }
            self.trigger_laser = laser_num;
            self.props
                .set(name, PropertyValue::Integer(laser_num as i64))?;
            if let Some(label) = self.trigger_label_for(laser_num) {
                let _ = self
                    .props
                    .set("Shutter Laser", PropertyValue::String(label));
            }
            return Ok(());
        }

        if name == "Delay_ms" {
            let delay = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
            self.props.set(name, PropertyValue::Float(delay))?;
            self.delay_ms = delay;
            return Ok(());
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
        self.changed_time.get().elapsed().as_secs_f64() * 1000.0 < self.delay_ms
    }
}

impl Shutter for CoherentScientificRemote {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        let state_tok = Self::laser_state_token(self.trigger_laser);
        let val = if open { "On" } else { "Off" };
        self.set_laser_cmd(&state_tok, val)?;
        self.is_open.set(open);
        self.changed_time.set(Instant::now());
        let state_prop = format!("Laser{}_State", self.trigger_laser);
        self.props
            .entry_mut(&state_prop)
            .map(|e| e.value = PropertyValue::String(val.into()));
        Ok(())
    }

    fn get_open(&self) -> MmResult<bool> {
        if self.initialized {
            self.query_state_bool(self.trigger_laser)
        } else {
            Ok(self.is_open.get())
        }
    }

    fn fire(&mut self, delta_t: f64) -> MmResult<()> {
        self.set_open(true)?;
        std::thread::sleep(std::time::Duration::from_millis(
            delta_t.max(0.0).round() as u64
        ));
        self.set_open(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_transport() -> MockTransport {
        MockTransport::new()
            .expect("*IDN?", "Coherent Scientific Remote v1.0")
            // Laser 1 present
            .expect("SYST1:INF:MOD?", "OBIS-488-50")
            .expect("SYST1:COMM:HAND On", "On")
            .expect("SYST1:COMM:HAND?", "On")
            .expect("SYST1:COMM:PROM Off", "Off")
            .expect("SYST1:COMM:PROM?", "Off")
            .expect("SYST1:ERR:CLE?", "0")
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
        let t = make_transport().expect("SOUR1:AM:STATE?", "Off");
        let mut dev = CoherentScientificRemote::new().with_transport(Box::new(t));
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
            .expect("SOUR1:AM:STATE?", "On")
            .expect("SOUR1:AM:STATE?", "On")
            .expect("SOUR1:AM:STATE Off", "Off")
            .expect("SOUR1:AM:STATE?", "Off")
            .expect("SOUR1:AM:STATE?", "Off");
        let mut dev = CoherentScientificRemote::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.get_open().unwrap());
        dev.set_open(false).unwrap();
        assert!(!dev.get_open().unwrap());
    }

    #[test]
    fn power_setpoint_sets_then_queries_and_live_get_refreshes() {
        let t = make_transport()
            .expect("SOUR1:POW:LIM:HIGH?", "0.100")
            .expect("SOUR1:POW:LIM:LOW?", "0.000")
            .expect("SOUR1:POW:LEV:IMM:AMPL 0.050000", "0.050")
            .expect("SOUR1:POW:LEV:IMM:AMPL?", "0.050")
            .expect("SOUR1:POW:LEV:IMM:AMPL?", "0.052")
            .expect("SOUR1:POW:LEV:IMM:AMPL?", "0.050")
            .expect("SOUR1:POW:LIM:HIGH?", "0.100")
            .expect("SOUR1:POW:LIM:LOW?", "0.000");
        let mut dev = CoherentScientificRemote::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Laser1_PowerSetpoint_pct", PropertyValue::Float(50.0))
            .unwrap();
        assert_eq!(
            dev.get_property("Laser1_PowerReadback_mW").unwrap(),
            PropertyValue::Float(52.0)
        );
        assert_eq!(
            dev.get_property("Laser1_PowerSetpoint_pct").unwrap(),
            PropertyValue::Float(50.0)
        );
    }

    #[test]
    fn upstream_named_properties_are_registered_and_live() {
        let t = make_transport()
            .expect("SOUR1:AM:SOUR?", "DIGITAL")
            .expect("SOUR1:POW:LEV:IMM:AMPL?", "0.020")
            .expect("SYST1:DIOD:HOUR?", "12.5")
            .expect("SOUR1:POW:LIM:LOW?", "0.001")
            .expect("SOUR1:POW:LIM:HIGH?", "0.050")
            .expect("SYST1:INF:WAV?", "488")
            .expect("SOUR1:TEMP:BAS?", "31.2")
            .expect("SYST1:INF:SNUM?", "SN123");
        let mut dev = CoherentScientificRemote::new().with_transport(Box::new(t));
        dev.initialize().unwrap();

        assert_eq!(
            dev.get_property("Shutter Laser").unwrap(),
            PropertyValue::String("1 (OBIS-488-50)".into())
        );
        assert!(dev.has_property("Laser OBIS-488-50 - PowerSetpoint (%)"));
        assert_eq!(
            dev.get_property("Laser OBIS-488-50 - Modulation/Trigger")
                .unwrap(),
            PropertyValue::String("External/Digital".into())
        );
        assert_eq!(
            dev.get_property("Laser OBIS-488-50 - PowerReadback (mW)")
                .unwrap(),
            PropertyValue::Float(20.0)
        );
        assert_eq!(
            dev.get_property("Laser OBIS-488-50 - Head Usage (h)")
                .unwrap(),
            PropertyValue::Float(12.5)
        );
        assert_eq!(
            dev.get_property("Laser OBIS-488-50 - Minimum Laser Power (mW)")
                .unwrap(),
            PropertyValue::Float(1.0)
        );
        assert_eq!(
            dev.get_property("Laser OBIS-488-50 - Maximum Laser Power (mW)")
                .unwrap(),
            PropertyValue::Float(50.0)
        );
        assert_eq!(
            dev.get_property("Laser OBIS-488-50 - Wavelength (nm)")
                .unwrap(),
            PropertyValue::Float(488.0)
        );
        assert_eq!(
            dev.get_property("Laser OBIS-488-50 - Temperature Baseplate (C)")
                .unwrap(),
            PropertyValue::Float(31.2)
        );
        assert_eq!(
            dev.get_property("Laser OBIS-488-50 - Head ID").unwrap(),
            PropertyValue::String("SN123".into())
        );
    }

    #[test]
    fn upstream_modulation_and_power_setters_use_cpp_tokens() {
        let t = make_transport()
            .expect("SOUR1:AM:EXT DIG", "DIG")
            .expect("SOUR1:AM:EXT?", "DIG")
            .expect("SOUR1:POW:LIM:HIGH?", "0.100")
            .expect("SOUR1:POW:LIM:LOW?", "0.000")
            .expect("SOUR1:POW:LEV:IMM:AMPL 0.025000", "0.025")
            .expect("SOUR1:POW:LEV:IMM:AMPL?", "0.025");
        let mut dev = CoherentScientificRemote::new().with_transport(Box::new(t));
        dev.initialize().unwrap();

        dev.set_property(
            "Laser OBIS-488-50 - Modulation/Trigger",
            PropertyValue::String("External/Digital".into()),
        )
        .unwrap();
        dev.set_property(
            "Laser OBIS-488-50 - PowerSetpoint (%)",
            PropertyValue::Float(25.0),
        )
        .unwrap();
    }

    #[test]
    fn busy_tracks_delay_after_state_change() {
        let t = make_transport()
            .expect("SOUR1:AM:STATE On", "On")
            .expect("SOUR1:AM:STATE?", "On");
        let mut dev = CoherentScientificRemote::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.set_property("Delay_ms", PropertyValue::Float(1000.0))
            .unwrap();
        dev.set_open(true).unwrap();
        assert!(dev.busy());
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

    #[test]
    fn initialized_port_change_is_rejected() {
        let t = make_transport();
        let mut dev = CoherentScientificRemote::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        assert_eq!(
            dev.set_property("Port", PropertyValue::String("COM2".into()))
                .unwrap_err(),
            MmError::InvalidPropertyValue
        );
    }

    #[test]
    fn fire_closes_after_pulse() {
        let t = make_transport()
            .expect("SOUR1:AM:STATE On", "On")
            .expect("SOUR1:AM:STATE?", "On")
            .expect("SOUR1:AM:STATE Off", "Off")
            .expect("SOUR1:AM:STATE?", "Off")
            .expect("SOUR1:AM:STATE?", "Off");
        let mut dev = CoherentScientificRemote::new().with_transport(Box::new(t));
        dev.initialize().unwrap();
        dev.fire(0.0).unwrap();
        assert!(!dev.get_open().unwrap());
    }
}
