//! Arduino 32-bit Boards adapter.
//!
//! Protocol:
//! - Hub identification: send byte 30 → "MM-Ard\r\n"; send byte 31 → firmware version integer
//! - Switch: send `[1, value_byte]` → response byte `1` (single-byte write, 8-bit port)
//! - DA/PWM: send `[3, channel-1, hi_byte, lo_byte]` → response byte `3` (12-bit value)
//!
//! Devices exported:
//! - `Arduino32-Hub`      — Hub; manages the serial connection and shared state
//! - `Arduino32-Shutter`  — Shutter controlling the digital output LSB
//! - `Arduino32-Switch`   — 8-bit digital output StateDevice
//! - `Arduino32-DAC/PWM-1..8` — 12-bit DAC/PWM SignalIO channels

pub mod hub;
pub mod shutter;
pub mod switch;
pub mod da;

pub use hub::Arduino32Hub;
pub use shutter::Arduino32Shutter;
pub use switch::Arduino32Switch;
pub use da::Arduino32Da;

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

pub const DEVICE_NAME_HUB: &str = "Arduino32-Hub";
pub const DEVICE_NAME_SHUTTER: &str = "Arduino32-Shutter";
pub const DEVICE_NAME_SWITCH: &str = "Arduino32-Switch";
pub const DEVICE_NAME_DA1: &str = "Arduino32-DAC/PWM-1";
pub const DEVICE_NAME_DA2: &str = "Arduino32-DAC/PWM-2";
pub const DEVICE_NAME_DA3: &str = "Arduino32-DAC/PWM-3";
pub const DEVICE_NAME_DA4: &str = "Arduino32-DAC/PWM-4";
pub const DEVICE_NAME_DA5: &str = "Arduino32-DAC/PWM-5";
pub const DEVICE_NAME_DA6: &str = "Arduino32-DAC/PWM-6";
pub const DEVICE_NAME_DA7: &str = "Arduino32-DAC/PWM-7";
pub const DEVICE_NAME_DA8: &str = "Arduino32-DAC/PWM-8";

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo { name: DEVICE_NAME_HUB,     description: "Arduino32 Hub (required)",          device_type: DeviceType::Hub },
    DeviceInfo { name: DEVICE_NAME_SHUTTER,  description: "Arduino32 shutter (digital LSB)",  device_type: DeviceType::Shutter },
    DeviceInfo { name: DEVICE_NAME_SWITCH,   description: "Arduino32 8-bit digital output",   device_type: DeviceType::State },
    DeviceInfo { name: DEVICE_NAME_DA1,      description: "Arduino32 DAC/PWM channel 1",      device_type: DeviceType::SignalIO },
    DeviceInfo { name: DEVICE_NAME_DA2,      description: "Arduino32 DAC/PWM channel 2",      device_type: DeviceType::SignalIO },
    DeviceInfo { name: DEVICE_NAME_DA3,      description: "Arduino32 DAC/PWM channel 3",      device_type: DeviceType::SignalIO },
    DeviceInfo { name: DEVICE_NAME_DA4,      description: "Arduino32 DAC/PWM channel 4",      device_type: DeviceType::SignalIO },
    DeviceInfo { name: DEVICE_NAME_DA5,      description: "Arduino32 DAC/PWM channel 5",      device_type: DeviceType::SignalIO },
    DeviceInfo { name: DEVICE_NAME_DA6,      description: "Arduino32 DAC/PWM channel 6",      device_type: DeviceType::SignalIO },
    DeviceInfo { name: DEVICE_NAME_DA7,      description: "Arduino32 DAC/PWM channel 7",      device_type: DeviceType::SignalIO },
    DeviceInfo { name: DEVICE_NAME_DA8,      description: "Arduino32 DAC/PWM channel 8",      device_type: DeviceType::SignalIO },
];

pub struct Arduino32Adapter;

impl AdapterModule for Arduino32Adapter {
    fn module_name(&self) -> &'static str { "arduino32" }

    fn devices(&self) -> &'static [DeviceInfo] { DEVICE_LIST }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_HUB     => Some(AnyDevice::Hub(Box::new(Arduino32Hub::new()))),
            DEVICE_NAME_SHUTTER => Some(AnyDevice::Shutter(Box::new(Arduino32Shutter::new()))),
            DEVICE_NAME_SWITCH  => Some(AnyDevice::StateDevice(Box::new(Arduino32Switch::new()))),
            DEVICE_NAME_DA1     => Some(AnyDevice::SignalIO(Box::new(Arduino32Da::new(1)))),
            DEVICE_NAME_DA2     => Some(AnyDevice::SignalIO(Box::new(Arduino32Da::new(2)))),
            DEVICE_NAME_DA3     => Some(AnyDevice::SignalIO(Box::new(Arduino32Da::new(3)))),
            DEVICE_NAME_DA4     => Some(AnyDevice::SignalIO(Box::new(Arduino32Da::new(4)))),
            DEVICE_NAME_DA5     => Some(AnyDevice::SignalIO(Box::new(Arduino32Da::new(5)))),
            DEVICE_NAME_DA6     => Some(AnyDevice::SignalIO(Box::new(Arduino32Da::new(6)))),
            DEVICE_NAME_DA7     => Some(AnyDevice::SignalIO(Box::new(Arduino32Da::new(7)))),
            DEVICE_NAME_DA8     => Some(AnyDevice::SignalIO(Box::new(Arduino32Da::new(8)))),
            _ => None,
        }
    }
}
