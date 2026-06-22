/// Vincent Uniblitz shutter controller (D1/D3).
///
/// Binary protocol — single byte commands, NO response from device.
///
/// Command byte = base + offset:
///   Address 'x' (broadcast) → base = 64 (0x40)
///   Address 0–7             → base = 128 + address * 16
///
/// D1 offsets (single shutter A or dual A+B):
///   +0 = Open A,  +1 = Close A
///   +4 = Open B,  +5 = Close B
///
/// D3 offsets (3-channel):
///   +0 = Open ch1,  +1 = Close ch1
///   +2 = Open ch2,  +3 = Close ch2
///   +4 = Open ch3,  +5 = Close ch3
///   +6 = Open all,  +7 = Close all
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Shutter};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::time::Instant;

/// Which Vincent controller variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VincentModel {
    D1,
    D3,
}

pub struct VincentShutter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    model: VincentModel,
    address: u8, // 0–7 or 0xFF for broadcast ('x')
    channel: u8, // 0 = A (D1) or channel index (D3)
    is_open: bool,
    opening_time_ms: f64,
    closing_time_ms: f64,
    last_command: String,
    changed_time: Option<Instant>,
}

impl VincentShutter {
    pub fn new(model: VincentModel) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Address", PropertyValue::String("x".into()), false)
            .unwrap();
        props
            .set_allowed_values("Address", &["x", "0", "1", "2", "3", "4", "5", "6", "7"])
            .unwrap();
        match model {
            VincentModel::D1 => {
                props
                    .define_property("Shutter A or B", PropertyValue::String("A".into()), false)
                    .unwrap();
                props
                    .set_allowed_values("Shutter A or B", &["A", "B"])
                    .unwrap();
            }
            VincentModel::D3 => {
                props
                    .define_property("Channel #", PropertyValue::String("Ch. #1".into()), false)
                    .unwrap();
                props
                    .set_allowed_values("Channel #", &["Ch. #1", "Ch. #2", "Ch. #3", "All"])
                    .unwrap();
            }
        }
        props
            .define_property("Time to close (ms)", PropertyValue::Float(35.0), false)
            .unwrap();
        props
            .define_property("Time to open (ms)", PropertyValue::Float(35.0), false)
            .unwrap();

        Self {
            props,
            transport: None,
            initialized: false,
            model,
            address: 0xFF,
            channel: 0,
            is_open: false,
            opening_time_ms: 35.0,
            closing_time_ms: 35.0,
            last_command: "Undefined".into(),
            changed_time: None,
        }
    }

    fn ensure_command_property(&mut self) -> MmResult<()> {
        if self.props.has_property("Command") {
            return Ok(());
        }
        self.props
            .define_property("Command", PropertyValue::String("Close".into()), false)?;
        match self.model {
            VincentModel::D1 => self
                .props
                .set_allowed_values("Command", &["Close", "Open", "Trigger", "Reset"]),
            VincentModel::D3 => self.props.set_allowed_values("Command", &["Close", "Open"]),
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

    fn base_byte(&self) -> u8 {
        if self.address == 0xFF {
            64
        } else {
            128 + self.address * 16
        }
    }

    fn open_offset(&self) -> u8 {
        match self.model {
            VincentModel::D1 => {
                if self.channel == 0 {
                    0
                } else {
                    4
                }
            }
            VincentModel::D3 => self.channel * 2,
        }
    }

    fn close_offset(&self) -> u8 {
        self.open_offset() + 1
    }

    fn command_offset(&self, command: &str) -> MmResult<u8> {
        match (self.model, command) {
            (VincentModel::D1, "Open") => Ok(if self.channel == 0 { 0 } else { 4 }),
            (VincentModel::D1, "Close") => Ok(if self.channel == 0 { 1 } else { 5 }),
            (VincentModel::D1, "Trigger") => Ok(2),
            (VincentModel::D1, "Reset") => Ok(3),
            (VincentModel::D3, "Open") => Ok(if self.channel >= 3 {
                6
            } else {
                self.channel * 2
            }),
            (VincentModel::D3, "Close") => Ok(if self.channel >= 3 {
                7
            } else {
                self.channel * 2 + 1
            }),
            _ => Err(MmError::InvalidPropertyValue),
        }
    }

    fn execute_command(&mut self, command: &str) -> MmResult<()> {
        let offset = self.command_offset(command)?;
        let cmd = self.base_byte() + offset;
        self.call_transport(|t| {
            t.purge()?;
            t.send_bytes(&[cmd])
        })?;
        self.last_command = command.to_string();
        self.changed_time = Some(Instant::now());
        if command == "Open" {
            self.is_open = true;
        } else if command == "Close" {
            self.is_open = false;
        }
        self.props
            .set("Command", PropertyValue::String(command.into()))?;
        Ok(())
    }

    fn set_address_from_property(&mut self, value: &str) -> MmResult<()> {
        self.address = if value == "x" {
            0xFF
        } else {
            value
                .parse::<u8>()
                .ok()
                .filter(|v| *v < 8)
                .ok_or(MmError::InvalidPropertyValue)?
        };
        Ok(())
    }
}

impl Default for VincentShutter {
    fn default() -> Self {
        Self::new(VincentModel::D1)
    }
}

impl Device for VincentShutter {
    fn name(&self) -> &str {
        match self.model {
            VincentModel::D1 => "Vincent-D1",
            VincentModel::D3 => "Vincent-D3",
        }
    }
    fn description(&self) -> &str {
        match self.model {
            VincentModel::D1 => "Vincent D1 controller adapter",
            VincentModel::D3 => "Vincent D3 controller adapter",
        }
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.ensure_command_property()?;
        if self.model == VincentModel::D1 {
            self.execute_command("Close")?;
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::CanNotSetProperty),
            "Address" => {
                let address = val.as_str().to_string();
                self.set_address_from_property(&address)?;
                self.props.set(name, PropertyValue::String(address))
            }
            "Shutter A or B" => {
                let channel = val.as_str().to_string();
                self.channel = if channel == "A" { 0 } else { 1 };
                self.props.set(name, PropertyValue::String(channel))
            }
            "Channel #" => {
                let channel = val.as_str().to_string();
                self.channel = match channel.as_str() {
                    "Ch. #1" => 0,
                    "Ch. #2" => 1,
                    "Ch. #3" => 2,
                    "All" => 3,
                    _ => return Err(MmError::InvalidPropertyValue),
                };
                self.props.set(name, PropertyValue::String(channel))
            }
            "Time to close (ms)" => {
                let value = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if value < 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.closing_time_ms = value;
                self.props.set(name, PropertyValue::Float(value))
            }
            "Time to open (ms)" => {
                let value = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if value < 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.opening_time_ms = value;
                self.props.set(name, PropertyValue::Float(value))
            }
            "Command" => {
                let command = val.as_str().to_string();
                if self.initialized {
                    self.execute_command(&command)
                } else {
                    self.props.set(name, PropertyValue::String(command))
                }
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
        if name == "Port" && self.initialized {
            return true;
        }
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Shutter
    }
    fn busy(&self) -> bool {
        let Some(changed) = self.changed_time else {
            return false;
        };
        let elapsed_ms = changed.elapsed().as_secs_f64() * 1000.0;
        match self.last_command.as_str() {
            "Open" => elapsed_ms <= self.opening_time_ms,
            "Close" => elapsed_ms <= self.closing_time_ms,
            _ => false,
        }
    }
}

impl Shutter for VincentShutter {
    fn set_open(&mut self, open: bool) -> MmResult<()> {
        if !self.initialized {
            return Err(MmError::UnknownLabel("Command".into()));
        }
        self.execute_command(if open { "Open" } else { "Close" })
    }
    fn get_open(&self) -> MmResult<bool> {
        Ok(self.is_open)
    }
    fn fire(&mut self, _dt: f64) -> MmResult<()> {
        Err(MmError::UnsupportedCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn d1_broadcast_commands() {
        // broadcast base=64; close=64+1=65, open=64+0=64
        let t = MockTransport::new(); // no responses expected
        let mut s = VincentShutter::new(VincentModel::D1).with_transport(Box::new(t));
        s.initialize().unwrap(); // sends close byte 65
        assert!(!s.get_open().unwrap());
        assert_eq!(s.base_byte(), 64);
        assert_eq!(s.open_offset(), 0);
        assert_eq!(s.close_offset(), 1);
    }

    #[test]
    fn d1_open_close() {
        let t = MockTransport::new();
        let mut s = VincentShutter::new(VincentModel::D1).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());
        s.set_open(false).unwrap();
        assert!(!s.get_open().unwrap());
        // Verify bytes sent: [65 (init close), 64 (open), 65 (close)]
        // (can't inspect bytes from outside test, but they're in received_bytes)
    }

    #[test]
    fn d3_channel_offsets() {
        let mut s = VincentShutter::new(VincentModel::D3);
        s.channel = 2; // channel 3
        assert_eq!(s.open_offset(), 4);
        assert_eq!(s.close_offset(), 5);
    }

    #[test]
    fn addressed_base_byte() {
        let mut s = VincentShutter::new(VincentModel::D1);
        s.address = 2;
        assert_eq!(s.base_byte(), 128 + 2 * 16); // = 160
    }

    #[test]
    fn upstream_d1_properties_drive_command_bytes() {
        let t = MockTransport::new();
        let mut s = VincentShutter::new(VincentModel::D1).with_transport(Box::new(t));
        s.set_property("Address", PropertyValue::String("2".into()))
            .unwrap();
        s.set_property("Shutter A or B", PropertyValue::String("B".into()))
            .unwrap();
        s.initialize().unwrap();
        assert_eq!(s.base_byte(), 160);
        assert_eq!(s.open_offset(), 4);
        s.set_property("Command", PropertyValue::String("Open".into()))
            .unwrap();
        assert!(s.get_open().unwrap());
        assert!(s.busy());
    }

    #[test]
    fn d1_trigger_and_reset_offsets() {
        let s = VincentShutter::new(VincentModel::D1);
        assert_eq!(s.command_offset("Trigger").unwrap(), 2);
        assert_eq!(s.command_offset("Reset").unwrap(), 3);
    }

    #[test]
    fn d3_all_channel_offsets() {
        let mut s = VincentShutter::new(VincentModel::D3);
        s.set_property("Channel #", PropertyValue::String("All".into()))
            .unwrap();
        assert_eq!(s.command_offset("Open").unwrap(), 6);
        assert_eq!(s.command_offset("Close").unwrap(), 7);
    }

    #[test]
    fn busy_uses_configured_timing() {
        let t = MockTransport::new();
        let mut s = VincentShutter::new(VincentModel::D1).with_transport(Box::new(t));
        s.set_property("Time to open (ms)", PropertyValue::Float(0.0))
            .unwrap();
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(!s.busy());
    }

    #[test]
    fn no_transport_error() {
        assert!(VincentShutter::new(VincentModel::D1).initialize().is_err());
    }

    #[test]
    fn command_property_is_created_during_initialize() {
        let mut d1 = VincentShutter::new(VincentModel::D1);
        assert!(!d1.has_property("Command"));
        assert_eq!(
            d1.set_open(true),
            Err(MmError::UnknownLabel("Command".into()))
        );

        d1 = d1.with_transport(Box::new(MockTransport::new()));
        d1.initialize().unwrap();
        assert!(d1.has_property("Command"));
        assert_eq!(
            d1.get_property("Command").unwrap(),
            PropertyValue::String("Close".into())
        );
    }

    #[test]
    fn d3_initialize_creates_command_without_closing() {
        let mut d3 =
            VincentShutter::new(VincentModel::D3).with_transport(Box::new(MockTransport::new()));
        assert!(!d3.has_property("Command"));

        d3.initialize().unwrap();

        assert!(d3.has_property("Command"));
        assert!(!d3.busy());
        assert!(!d3.get_open().unwrap());
    }

    #[test]
    fn d1_and_d3_have_separate_device_identity() {
        let d1 = VincentShutter::new(VincentModel::D1);
        let d3 = VincentShutter::new(VincentModel::D3);
        assert_eq!(d1.name(), "Vincent-D1");
        assert_eq!(d3.name(), "Vincent-D3");
        assert_eq!(d1.description(), "Vincent D1 controller adapter");
        assert_eq!(d3.description(), "Vincent D3 controller adapter");
    }

    #[test]
    fn d3_uses_upstream_channel_labels() {
        let mut s = VincentShutter::new(VincentModel::D3);
        assert_eq!(
            s.get_property("Channel #").unwrap(),
            PropertyValue::String("Ch. #1".into())
        );
        s.set_property("Channel #", PropertyValue::String("Ch. #2".into()))
            .unwrap();
        assert_eq!(s.command_offset("Open").unwrap(), 2);
        assert_eq!(
            s.set_property("Channel #", PropertyValue::String("2".into())),
            Err(MmError::InvalidPropertyValue)
        );
    }

    #[test]
    fn port_is_locked_after_initialize() {
        let t = MockTransport::new();
        let mut s = VincentShutter::new(VincentModel::D1).with_transport(Box::new(t));
        assert!(!s.is_property_read_only("Port"));
        s.initialize().unwrap();
        assert!(s.is_property_read_only("Port"));
        assert_eq!(
            s.set_property("Port", PropertyValue::String("COM4".into())),
            Err(MmError::CanNotSetProperty)
        );
    }

    #[test]
    fn shutdown_only_clears_initialized_state() {
        let t = MockTransport::new();
        let mut s = VincentShutter::new(VincentModel::D1).with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_open(true).unwrap();
        assert!(s.get_open().unwrap());

        s.shutdown().unwrap();

        assert!(s.get_open().unwrap());
        assert!(!s.is_property_read_only("Port"));
    }
}
