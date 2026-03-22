//! TeensyPulseGenerator — Generic device for the Teensy pulse-generator firmware.
//!
//! Binary protocol over serial:
//! Write command: 5 bytes [cmd, p0, p1, p2, p3]  (p0-p3 = uint32_le param)
//! Enquire:       2 bytes [0xFF, cmd]
//! Response:      5 bytes [cmd, v0, v1, v2, v3]  (v0-v3 = uint32_le value)

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Generic};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const CMD_VERSION: u8 = 0x00;
const CMD_START:   u8 = 0x01;
const CMD_STOP:    u8 = 0x02;
const CMD_INTERVAL: u8 = 0x03;
const CMD_PULSE_DUR: u8 = 0x04;
const CMD_TRIGGER:   u8 = 0x05;
const CMD_NR_PULSES: u8 = 0x06;
const ENQUIRE: u8 = 0xFF;

pub struct TeensyPulseGenerator {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    version: u32,
    interval_ms: f64,
    pulse_dur_ms: f64,
    trigger_mode: bool,
    run_until_stopped: bool,
    nr_pulses: u32,
    running: bool,
}

impl TeensyPulseGenerator {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("FirmwareVersion", PropertyValue::Integer(0), true).unwrap();
        props.define_property("IntervalMs", PropertyValue::Float(100.0), false).unwrap();
        props.define_property("PulseDurationMs", PropertyValue::Float(10.0), false).unwrap();
        props.define_property("TriggerMode", PropertyValue::String("Off".into()), false).unwrap();
        props.set_allowed_values("TriggerMode", &["Off", "On"]).unwrap();
        props.define_property("RunUntilStopped", PropertyValue::String("On".into()), false).unwrap();
        props.set_allowed_values("RunUntilStopped", &["Off", "On"]).unwrap();
        props.define_property("NumberOfPulses", PropertyValue::Integer(1), false).unwrap();
        props.define_property("State", PropertyValue::String("Stop".into()), false).unwrap();
        props.set_allowed_values("State", &["Stop", "Start"]).unwrap();
        props.define_property("Status", PropertyValue::String("Idle".into()), true).unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            version: 0,
            interval_ms: 100.0,
            pulse_dur_ms: 10.0,
            trigger_mode: false,
            run_until_stopped: true,
            nr_pulses: 1,
            running: false,
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

    /// Send a 5-byte command [cmd, param_le_u32].
    fn send_command(&mut self, cmd: u8, param: u32) -> MmResult<()> {
        let bytes = [
            cmd,
            (param & 0xFF) as u8,
            ((param >> 8) & 0xFF) as u8,
            ((param >> 16) & 0xFF) as u8,
            ((param >> 24) & 0xFF) as u8,
        ];
        self.call_transport(|t| t.send_bytes(&bytes))
    }

    /// Send a 2-byte enquire [0xFF, cmd].
    fn enquire(&mut self, cmd: u8) -> MmResult<()> {
        let bytes = [ENQUIRE, cmd];
        self.call_transport(|t| t.send_bytes(&bytes))
    }

    /// Read a 5-byte response and decode the uint32 value.
    fn get_response(&mut self, expected_cmd: u8) -> MmResult<u32> {
        let raw = self.call_transport(|t| t.receive_bytes(5))?;
        if raw.len() < 5 || raw[0] != expected_cmd {
            return Err(MmError::SerialInvalidResponse);
        }
        let val = u32::from_le_bytes([raw[1], raw[2], raw[3], raw[4]]);
        Ok(val)
    }

    fn get_param(&mut self, cmd: u8) -> MmResult<u32> {
        self.enquire(cmd)?;
        self.get_response(cmd)
    }

    fn set_param(&mut self, cmd: u8, val: u32) -> MmResult<u32> {
        self.send_command(cmd, val)?;
        self.get_response(cmd)
    }
}

impl Default for TeensyPulseGenerator {
    fn default() -> Self { Self::new() }
}

impl Device for TeensyPulseGenerator {
    fn name(&self) -> &str { "TeensyPulseGenerator" }
    fn description(&self) -> &str { "Teensy-based pulse generator" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }

        // Get firmware version
        self.send_command(CMD_VERSION, 0)?;
        self.version = self.get_response(0)?;
        self.props.entry_mut("FirmwareVersion")
            .map(|e| e.value = PropertyValue::Integer(self.version as i64));

        // Read current settings from firmware
        let interval_us = self.get_param(CMD_INTERVAL)?;
        self.interval_ms = interval_us as f64 / 1000.0;

        let pulse_us = self.get_param(CMD_PULSE_DUR)?;
        self.pulse_dur_ms = pulse_us as f64 / 1000.0;

        let trigger = self.get_param(CMD_TRIGGER)?;
        self.trigger_mode = trigger != 0;

        let nr = self.get_param(CMD_NR_PULSES)?;
        self.nr_pulses = nr;
        self.run_until_stopped = nr == 0;

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send_command(CMD_STOP, 0);
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "IntervalMs"       => Ok(PropertyValue::Float(self.interval_ms)),
            "PulseDurationMs"  => Ok(PropertyValue::Float(self.pulse_dur_ms)),
            "TriggerMode"      => Ok(PropertyValue::String(if self.trigger_mode { "On" } else { "Off" }.into())),
            "RunUntilStopped"  => Ok(PropertyValue::String(if self.run_until_stopped { "On" } else { "Off" }.into())),
            "NumberOfPulses"   => Ok(PropertyValue::Integer(self.nr_pulses as i64)),
            "State"            => Ok(PropertyValue::String(if self.running { "Start" } else { "Stop" }.into())),
            "Status"           => Ok(PropertyValue::String(if self.running { "Active" } else { "Idle" }.into())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "IntervalMs" if self.initialized => {
                let ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                let us = (ms * 1000.0) as u32;
                let resp = self.set_param(CMD_INTERVAL, us)?;
                if resp != us { return Err(MmError::SerialInvalidResponse); }
                self.interval_ms = ms;
                Ok(())
            }
            "PulseDurationMs" if self.initialized => {
                let ms = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                let us = (ms * 1000.0) as u32;
                let resp = self.set_param(CMD_PULSE_DUR, us)?;
                if resp != us { return Err(MmError::SerialInvalidResponse); }
                self.pulse_dur_ms = ms;
                Ok(())
            }
            "TriggerMode" if self.initialized => {
                let on = val.as_str() == "On";
                let param = if on { 1u32 } else { 0 };
                let resp = self.set_param(CMD_TRIGGER, param)?;
                if resp != param { return Err(MmError::SerialInvalidResponse); }
                self.trigger_mode = on;
                Ok(())
            }
            "RunUntilStopped" if self.initialized => {
                let on = val.as_str() == "On";
                let param = if on { 0u32 } else { self.nr_pulses };
                let resp = self.set_param(CMD_NR_PULSES, param)?;
                if resp != param { return Err(MmError::SerialInvalidResponse); }
                self.run_until_stopped = on;
                Ok(())
            }
            "NumberOfPulses" if self.initialized => {
                let n = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u32;
                if !self.run_until_stopped {
                    let resp = self.set_param(CMD_NR_PULSES, n)?;
                    if resp != n { return Err(MmError::SerialInvalidResponse); }
                }
                self.nr_pulses = n;
                Ok(())
            }
            "State" if self.initialized => {
                if val.as_str() == "Start" && !self.running {
                    let resp = self.set_param(CMD_START, 0)?;
                    if resp != 1 { return Err(MmError::SerialInvalidResponse); }
                    self.running = true;
                } else if val.as_str() == "Stop" && self.running {
                    self.send_command(CMD_STOP, 0)?;
                    let _ = self.get_response(CMD_STOP);
                    self.running = false;
                }
                Ok(())
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Generic }
    fn busy(&self) -> bool { false }
}

impl Generic for TeensyPulseGenerator {}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    /// Build a 5-byte response packet.
    fn resp(cmd: u8, val: u32) -> Vec<u8> {
        let v = val.to_le_bytes();
        vec![cmd, v[0], v[1], v[2], v[3]]
    }

    fn make_device() -> TeensyPulseGenerator {
        let t = MockTransport::new()
            // version: send_command(0x00, 0), get_response(0)
            .expect_binary(&resp(0x00, 42))
            // interval: enquire(0x03), get_response(0x03)
            .expect_binary(&resp(CMD_INTERVAL, 100_000))
            // pulse dur: enquire(0x04), get_response(0x04)
            .expect_binary(&resp(CMD_PULSE_DUR, 10_000))
            // trigger: enquire(0x05), get_response(0x05)
            .expect_binary(&resp(CMD_TRIGGER, 0))
            // nr pulses: enquire(0x06), get_response(0x06)
            .expect_binary(&resp(CMD_NR_PULSES, 0));
        TeensyPulseGenerator::new().with_transport(Box::new(t))
    }

    #[test]
    fn initialize_ok() {
        let mut dev = make_device();
        dev.initialize().unwrap();
        assert_eq!(dev.version, 42);
        assert!((dev.interval_ms - 100.0).abs() < 1e-6);
        assert!(dev.run_until_stopped);
    }
}
