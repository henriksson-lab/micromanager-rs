//! Teensy Pulse Generator adapter.
//!
//! Binary protocol (little-endian, Teensy is little-endian):
//! Commands are 5 bytes: [cmd_byte, param_u32_le]
//! Query (enquire) is 2 bytes: [0xFF, cmd_byte]
//! Response is 5 bytes: [cmd_byte, value_u32_le]
//!
//! Command bytes:
//!   0x00 = version (send with param 0, response has version in value)
//!   0x01 = start
//!   0x02 = stop
//!   0x03 = interval (microseconds)
//!   0x04 = pulse duration (microseconds)
//!   0x05 = wait_for_input (trigger mode: 0=off, 1=on)
//!   0x06 = number of pulses (0 = run until stopped)
//!
//! Devices exported:
//! - `TeensyPulseGenerator` — Generic device

pub mod pulse;
pub use pulse::TeensyPulseGenerator;

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

pub const DEVICE_NAME: &str = "TeensyPulseGenerator";

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo {
        name: DEVICE_NAME,
        description: "Teensy-based pulse generator",
        device_type: DeviceType::Generic,
    },
];

pub struct TeensyPulseAdapter;

impl AdapterModule for TeensyPulseAdapter {
    fn module_name(&self) -> &'static str { "teensy-pulse" }
    fn devices(&self) -> &'static [DeviceInfo] { DEVICE_LIST }
    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME => Some(AnyDevice::Generic(Box::new(TeensyPulseGenerator::new()))),
            _ => None,
        }
    }
}
