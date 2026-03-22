//! Yodn E600 LED light source controller adapter.
//!
//! The E600 uses a binary protocol over serial/USB.
//! Each command is a byte sequence; responses are fixed-length byte packets.
//!
//! Key binary protocol commands:
//!
//! | Command bytes       | Response    | Meaning                           |
//! |---------------------|-------------|-----------------------------------|
//! | `[0x70]`            | `[0x70, ...]`| Open / handshake                  |
//! | `[0x57, 0x00]`      | 3 bytes     | Get lamp state (byte[2]=state)    |
//! | `[0x56, ch]`        | 3 bytes     | Get channel intensity (byte[2])   |
//! | `[0x55, ch]`        | 3 bytes     | Get channel temperature (byte[2]) |
//! | `[0x53, ch]`        | 4 bytes     | Get channel use time (hours)      |
//! | `[0x52]`            | 2 bytes     | Get error code (byte[1])          |
//! | `[0x60, 0x00, 0x01]`| -           | Turn lamp ON                      |
//! | `[0x60, 0x00, 0x00]`| -           | Turn lamp OFF                     |
//! | `[0x75]`            | -           | Close / shutdown                  |
//!
//! Channel IDs: CH1=0x01, CH2=0x02, CH3=0x03

pub mod e600;

pub use e600::YodnE600;

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

pub const DEVICE_NAME_E600: &str = "YodnE600";

static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: DEVICE_NAME_E600,
    description: "YODN Hyper E600",
    device_type: DeviceType::Shutter,
}];

pub struct YodnE600Adapter;

impl AdapterModule for YodnE600Adapter {
    fn module_name(&self) -> &'static str {
        "yodn-e600"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_E600 => Some(AnyDevice::Shutter(Box::new(YodnE600::new()))),
            _ => None,
        }
    }
}
