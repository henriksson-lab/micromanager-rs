//! Arduino Counter device adapter.
//!
//! Implements an Arduino-based pulse counter. The original C++ wraps a real camera
//! and adds photon-counting functionality. In this Rust port, we model the counter
//! as a Generic device that manages the serial protocol with the ArduinoCounter
//! firmware.
//!
//! Protocol (ASCII over serial):
//! - Identify: send `i` → response "ArduinoCounter ... <version>\r\n"
//! - Start counting N pulses: send `g<N>\n` → ack line
//! - Stop counting: send `s` → ack line
//! - Query/set logic polarity:
//!   - send `p?`  → "Direct\r\n" or "Invert\r\n"
//!   - send `pi`  → "Invert\r\n"
//!   - send `pd`  → "Direct\r\n"
//!
//! Devices exported:
//! - `ArduinoCounter` — Generic device managing the counter firmware

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

pub mod counter;
pub use counter::ArduinoCounter;

pub const DEVICE_NAME_COUNTER: &str = "ArduinoCounter";

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo {
        name: DEVICE_NAME_COUNTER,
        description: "Arduino pulse counter device",
        device_type: DeviceType::Generic,
    },
];

pub struct ArduinoCounterAdapter;

impl AdapterModule for ArduinoCounterAdapter {
    fn module_name(&self) -> &'static str { "arduino-counter" }
    fn devices(&self) -> &'static [DeviceInfo] { DEVICE_LIST }
    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_COUNTER => Some(AnyDevice::Generic(Box::new(ArduinoCounter::new()))),
            _ => None,
        }
    }
}
