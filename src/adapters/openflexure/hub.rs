//! SangaBoardHub — manages serial port for the Sangaboard.

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Hub};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use parking_lot::Mutex;

const STAGE_STILL_MOVING: &str = "Stage is still moving. Current move aborted.";

pub struct SangaBoardHub {
    props: PropertyMap,
    transport: Mutex<Option<Box<dyn Transport>>>,
    initialized: bool,
    serial_response: String,
    manual_command: String,
}

impl SangaBoardHub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        props
            .define_property("SerialCommand", PropertyValue::String(String::new()), false)
            .unwrap();
        props
            .define_property("SerialResponse", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Stage Step Delay (us)", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .set_allowed_values(
                "Stage Step Delay (us)",
                &["1000", "2000", "3000", "4000", "5000"],
            )
            .unwrap();
        props
            .define_property("Stage Ramp Time (us)", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .set_allowed_values("Stage Ramp Time (us)", &["0", "100000", "200000", "300000"])
            .unwrap();
        props
            .define_property(
                "Xtra Stage Commands",
                PropertyValue::String("None".into()),
                false,
            )
            .unwrap();
        props
            .set_allowed_values(
                "Xtra Stage Commands",
                &["stop", "zero", "release", "version"],
            )
            .unwrap();
        Self {
            props,
            transport: Mutex::new(None),
            initialized: false,
            serial_response: String::new(),
            manual_command: String::new(),
        }
    }

    pub fn with_transport(self, t: Box<dyn Transport>) -> Self {
        *self.transport.lock() = Some(t);
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        let mut transport = self.transport.lock();
        match transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn query_busy(&self) -> MmResult<bool> {
        self.call_transport(|t| {
            t.purge()?;
            Ok(t.send_recv("moving?")?.contains("true"))
        })
    }

    /// Send a command and read the response line.
    pub fn send_command(&mut self, cmd: &str) -> MmResult<String> {
        if self.query_busy()? && cmd.contains("mr") {
            return Err(MmError::LocallyDefined(STAGE_STILL_MOVING.into()));
        }
        let response = self.call_transport(|t| Ok(t.send_recv(cmd)?.trim().to_string()))?;
        self.serial_response = response.clone();
        if let Some(entry) = self.props.entry_mut("SerialResponse") {
            entry.value = PropertyValue::String(response.clone());
        }
        Ok(response)
    }

    fn extract_number(text: &str) -> i64 {
        text.split_whitespace()
            .find_map(|word| word.parse::<i64>().ok())
            .unwrap_or(0)
    }

    fn sync_state(&mut self) -> MmResult<()> {
        let step_delay = Self::extract_number(&self.send_command("dt?")?);
        if let Some(entry) = self.props.entry_mut("Stage Step Delay (us)") {
            entry.value = PropertyValue::Integer(step_delay);
        }

        let ramp_time = Self::extract_number(&self.send_command("ramp_time?")?);
        if let Some(entry) = self.props.entry_mut("Stage Ramp Time (us)") {
            entry.value = PropertyValue::Integer(ramp_time);
        }

        Ok(())
    }
}

impl Default for SangaBoardHub {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for SangaBoardHub {
    fn name(&self) -> &str {
        "SangaBoardHub"
    }
    fn description(&self) -> &str {
        "Sangaboard Hub"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.lock().is_none() {
            return Err(MmError::NotConnected);
        }

        // Version check
        let resp = self.send_command("version")?;
        if !resp.contains("Sangaboard") {
            return Err(MmError::LocallyDefined(
                "Sangaboard not found — unexpected version response".into(),
            ));
        }

        self.sync_state()?;

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
        if name == "SerialCommand" && self.initialized {
            let command = val.as_str().to_string();
            let response = self.send_command(&command)?;
            self.manual_command = command;
            self.sync_state()?;
            if let Some(entry) = self.props.entry_mut("SerialResponse") {
                entry.value = PropertyValue::String(response);
            }
        }
        if name == "Stage Step Delay (us)" && self.initialized {
            let n = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            let cmd = format!("dt {}", n);
            self.send_command(&cmd)?;
        }
        if name == "Stage Ramp Time (us)" && self.initialized {
            let n = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
            let cmd = format!("ramp_time {}", n);
            self.send_command(&cmd)?;
        }
        if name == "Xtra Stage Commands" && self.initialized {
            let command = val.as_str().to_string();
            let response = self.send_command(&command)?;
            if let Some(entry) = self.props.entry_mut("SerialResponse") {
                entry.value = PropertyValue::String(response);
            }
            if let Some(entry) = self.props.entry_mut("Xtra Stage Commands") {
                entry.value = PropertyValue::String("None".into());
            }
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
        DeviceType::Hub
    }
    fn busy(&self) -> bool {
        self.query_busy().unwrap_or(false)
    }
}

impl Hub for SangaBoardHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        Ok(vec![
            "OFXYStage".into(),
            "OFZStage".into(),
            "OFShutter".into(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_ok() {
        let t = MockTransport::new()
            .expect("moving?", "\nfalse")
            .expect("version", "Sangaboard v0.5.1")
            .expect("moving?", "\nfalse")
            .expect("dt?", "minimum step delay 1000")
            .expect("moving?", "\nfalse")
            .expect("ramp_time?", "ramp_time 0");
        let mut hub = SangaBoardHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
    }

    #[test]
    fn wrong_device_rejected() {
        let t = MockTransport::new()
            .expect("moving?", "\nfalse")
            .expect("version", "UnknownDevice v1.0")
            .any("done");
        let mut hub = SangaBoardHub::new().with_transport(Box::new(t));
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn busy_queries_moving_state() {
        let t = MockTransport::new().expect("moving?", "\ntrue");
        let hub = SangaBoardHub::new().with_transport(Box::new(t));

        assert!(hub.busy());
    }

    #[test]
    fn send_command_rejects_relative_move_while_busy() {
        let t = MockTransport::new().expect("moving?", "\ntrue");
        let mut hub = SangaBoardHub::new().with_transport(Box::new(t));

        assert_eq!(
            hub.send_command("mrx 10").unwrap_err(),
            MmError::LocallyDefined(STAGE_STILL_MOVING.into())
        );
    }

    #[test]
    fn send_command_allows_relative_move_when_not_busy() {
        let t = MockTransport::new()
            .expect("moving?", "\nfalse")
            .expect("mrx 10", "done");
        let mut hub = SangaBoardHub::new().with_transport(Box::new(t));

        assert_eq!(hub.send_command("mrx 10").unwrap(), "done");
    }

    #[test]
    fn send_command_polls_busy_before_non_move_commands() {
        let t = MockTransport::new()
            .expect("moving?", "\nfalse")
            .expect("version", "Sangaboard v0.5.1");
        let mut hub = SangaBoardHub::new().with_transport(Box::new(t));

        assert_eq!(hub.send_command("version").unwrap(), "Sangaboard v0.5.1");
    }

    #[test]
    fn extra_stage_commands_reset_to_none_and_show_response() {
        let t = MockTransport::new()
            .expect("moving?", "\nfalse")
            .expect("version", "Sangaboard v0.5.1")
            .expect("moving?", "\nfalse")
            .expect("dt?", "minimum step delay 1000")
            .expect("moving?", "\nfalse")
            .expect("ramp_time?", "ramp_time 0")
            .expect("moving?", "\nfalse")
            .expect("stop", "done");
        let mut hub = SangaBoardHub::new().with_transport(Box::new(t));

        hub.initialize().unwrap();
        hub.set_property("Xtra Stage Commands", PropertyValue::String("stop".into()))
            .unwrap();

        assert_eq!(
            hub.get_property("Xtra Stage Commands").unwrap(),
            PropertyValue::String("None".into())
        );
        assert_eq!(
            hub.get_property("SerialResponse").unwrap(),
            PropertyValue::String("done".into())
        );
    }
}
