//! Arduino32Da — 12-bit DAC/PWM channel (SignalIO).

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, SignalIO};
use crate::types::{DeviceType, PropertyValue};

pub type DaWriter = std::sync::Arc<dyn Fn(u8, u16) -> MmResult<()> + Send + Sync>;

pub struct Arduino32Da {
    props: PropertyMap,
    initialized: bool,
    name: String,
    channel: u8,
    volts: f64,
    gate_open: bool,
    gated_volts: f64,
    min_volts: f64,
    max_volts: f64,
    writer: Option<DaWriter>,
}

impl Arduino32Da {
    pub fn new(channel: u8) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Volts", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("Power %", PropertyValue::Float(100.0), false)
            .unwrap();

        Self {
            props,
            initialized: false,
            name: format!("Arduino32-DAC/PWM-{}", channel),
            channel,
            volts: 0.0,
            gate_open: true,
            gated_volts: 0.0,
            min_volts: 0.0,
            max_volts: 100.0,
            writer: None,
        }
    }

    pub fn with_writer(mut self, writer: DaWriter) -> Self {
        self.writer = Some(writer);
        self
    }

    fn volts_to_counts(&self, volts: f64) -> u16 {
        let clamped = volts.clamp(self.min_volts, self.max_volts);
        let frac = (clamped - self.min_volts) / (self.max_volts - self.min_volts);
        (frac * 4095.0) as u16
    }

    fn write_volts(&self, volts: f64) -> MmResult<()> {
        let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
        writer(self.channel, self.volts_to_counts(volts))
    }
}

impl Device for Arduino32Da {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "Arduino32 DAC/PWM channel"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.writer.is_none() {
            return Err(MmError::CommHubMissing);
        }
        if let Ok(PropertyValue::Float(mv)) = self.props.get("Power %").cloned() {
            self.max_volts = mv;
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if name == "Volts" {
            return Ok(PropertyValue::Float(self.volts));
        }
        self.props.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Volts" => {
                let v = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized && self.gate_open {
                    self.write_volts(v)?;
                }
                self.volts = v;
                self.gated_volts = v;
                self.props
                    .entry_mut("Volts")
                    .map(|e| e.value = PropertyValue::Float(v));
                Ok(())
            }
            "Power %" => {
                let v = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.max_volts = v;
                self.props.set(name, PropertyValue::Float(v))
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
        DeviceType::SignalIO
    }
    fn busy(&self) -> bool {
        false
    }
}

impl SignalIO for Arduino32Da {
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
        if open {
            self.write_volts(self.gated_volts)?;
        } else {
            self.write_volts(0.0)?;
        }
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(self.gate_open)
    }

    fn set_signal(&mut self, volts: f64) -> MmResult<()> {
        if self.gate_open && self.initialized {
            self.write_volts(volts)?;
        }
        self.volts = volts;
        self.gated_volts = volts;
        Ok(())
    }

    fn get_signal(&self) -> MmResult<f64> {
        Err(MmError::UnsupportedCommand)
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((self.min_volts, self.max_volts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn make_da() -> (Arduino32Da, Arc<Mutex<Vec<(u8, u16)>>>) {
        let log: Arc<Mutex<Vec<(u8, u16)>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        let writer: DaWriter = Arc::new(move |ch, counts| {
            log2.lock().unwrap().push((ch, counts));
            Ok(())
        });
        (Arduino32Da::new(1).with_writer(writer), log)
    }

    #[test]
    fn initialize_does_not_write() {
        let (mut da, log) = make_da();
        da.initialize().unwrap();
        assert!(log.lock().unwrap().is_empty());
    }

    #[test]
    fn full_scale() {
        let (mut da, log) = make_da();
        da.initialize().unwrap();
        da.set_signal(100.0).unwrap();
        assert_eq!(log.lock().unwrap().last().unwrap(), &(1, 4095));
    }

    #[test]
    fn exposes_upstream_power_property_and_channel_name() {
        let da = Arduino32Da::new(3);
        assert_eq!(da.name(), "Arduino32-DAC/PWM-3");
        assert!(da.has_property("Power %"));
        assert!(!da.has_property("MaxVolts"));
        assert!(!da.has_property("Channel"));
        assert_eq!(
            da.get_property("Power %").unwrap(),
            PropertyValue::Float(100.0)
        );
    }

    #[test]
    fn conversion_truncates_like_upstream() {
        let (mut da, log) = make_da();
        da.initialize().unwrap();
        da.set_signal(50.0).unwrap();
        assert_eq!(log.lock().unwrap().last().unwrap(), &(1, 2047));
    }

    #[test]
    fn get_signal_is_unsupported_like_upstream() {
        let (mut da, _) = make_da();
        da.initialize().unwrap();
        da.set_signal(50.0).unwrap();
        assert_eq!(da.get_signal().unwrap_err(), MmError::UnsupportedCommand);
    }
}
