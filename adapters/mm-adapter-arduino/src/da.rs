/// ArduinoDa — 12-bit DAC channel (SignalIO) on the Arduino.
///
/// Voltage range 0–5 V by default; maps linearly to 0–4095 DAC counts.
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, SignalIO};
use mm_device::types::{DeviceType, PropertyValue};

pub type DaWriter = std::sync::Arc<dyn Fn(u8, u16) -> MmResult<()> + Send + Sync>;

pub struct ArduinoDa {
    props: PropertyMap,
    initialized: bool,
    channel: u8,
    volts: f64,
    gate_open: bool,
    gated_volts: f64,
    min_volts: f64,
    max_volts: f64,
    writer: Option<DaWriter>,
}

impl ArduinoDa {
    pub fn new(channel: u8) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Volts", PropertyValue::Float(0.0), false).unwrap();
        props.define_property("MaxVolts", PropertyValue::Float(5.0), false).unwrap();
        props.define_property("Channel", PropertyValue::Integer(channel as i64), true).unwrap();

        Self {
            props,
            initialized: false,
            channel,
            volts: 0.0,
            gate_open: true,
            gated_volts: 0.0,
            min_volts: 0.0,
            max_volts: 5.0,
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
        (frac * 4095.0).round() as u16
    }

    fn write_volts(&self, volts: f64) -> MmResult<()> {
        let writer = self.writer.as_ref().ok_or(MmError::NotConnected)?;
        let counts = self.volts_to_counts(volts);
        writer(self.channel, counts)
    }
}

impl Device for ArduinoDa {
    fn name(&self) -> &str { "Arduino-DA" }
    fn description(&self) -> &str { "Arduino DAC channel" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.writer.is_none() {
            return Err(MmError::CommHubMissing);
        }
        // Update max_volts from property in case it was set pre-init
        if let Ok(PropertyValue::Float(mv)) = self.props.get("MaxVolts").cloned() {
            self.max_volts = mv;
        }
        self.write_volts(0.0)?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.write_volts(0.0);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Volts" => Ok(PropertyValue::Float(self.volts)),
            _ => self.props.get(name).cloned(),
        }
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
                self.props.entry_mut("Volts")
                    .map(|e| e.value = PropertyValue::Float(v));
                Ok(())
            }
            "MaxVolts" => {
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

    fn device_type(&self) -> DeviceType { DeviceType::SignalIO }
    fn busy(&self) -> bool { false }
}

impl SignalIO for ArduinoDa {
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
        Ok(self.volts)
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((self.min_volts, self.max_volts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn make_da() -> (ArduinoDa, Arc<Mutex<Vec<(u8, u16)>>>) {
        let log: Arc<Mutex<Vec<(u8, u16)>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        let writer: DaWriter = Arc::new(move |ch, counts| {
            log2.lock().unwrap().push((ch, counts));
            Ok(())
        });
        (ArduinoDa::new(1).with_writer(writer), log)
    }

    #[test]
    fn initialize_writes_zero() {
        let (mut da, log) = make_da();
        da.initialize().unwrap();
        let writes = log.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0], (1, 0));
    }

    #[test]
    fn set_signal_converts_to_counts() {
        let (mut da, log) = make_da();
        da.initialize().unwrap();
        da.set_signal(5.0).unwrap(); // max = 4095
        let writes = log.lock().unwrap();
        assert_eq!(writes.last().unwrap(), &(1, 4095));
    }

    #[test]
    fn gate_closed_writes_zero() {
        let (mut da, log) = make_da();
        da.initialize().unwrap();
        da.set_signal(2.5).unwrap();
        da.set_gate_open(false).unwrap();
        let writes = log.lock().unwrap();
        // Last write should be 0 (gate closed)
        assert_eq!(writes.last().unwrap().1, 0);
    }

    #[test]
    fn limits() {
        let (da, _) = make_da();
        assert_eq!(da.get_limits().unwrap(), (0.0, 5.0));
    }
}
