//! Arduino device adapter.
//!
//! Implements the MicroManager Arduino firmware protocol (firmware v1–v5).
//!
//! Devices exported:
//! - `Arduino-Hub`      — Hub; manages the serial connection and shared state
//! - `Arduino-Shutter`  — Controls digital out via the switch state (LSB)
//! - `Arduino-Switch`   — 8-bit digital output port (StateDevice)
//! - `Arduino-DAC1..8`  — 12-bit DAC channels (SignalIO)

pub mod hub;
pub mod shutter;
pub mod switch;
pub mod da;

pub use hub::ArduinoHub;
pub use shutter::ArduinoShutter;
pub use switch::ArduinoSwitch;
pub use da::ArduinoDa;

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

pub const DEVICE_NAME_HUB: &str = "Arduino-Hub";
pub const DEVICE_NAME_SHUTTER: &str = "Arduino-Shutter";
pub const DEVICE_NAME_SWITCH: &str = "Arduino-Switch";
pub const DEVICE_NAME_DA1: &str = "Arduino-DAC1";
pub const DEVICE_NAME_DA2: &str = "Arduino-DAC2";

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo { name: DEVICE_NAME_HUB,     description: "Arduino Hub (required)",   device_type: DeviceType::Hub },
    DeviceInfo { name: DEVICE_NAME_SHUTTER,  description: "Arduino shutter (digital out LSB)", device_type: DeviceType::Shutter },
    DeviceInfo { name: DEVICE_NAME_SWITCH,   description: "Arduino 8-bit digital output",      device_type: DeviceType::State },
    DeviceInfo { name: DEVICE_NAME_DA1,      description: "Arduino DAC channel 1",             device_type: DeviceType::SignalIO },
    DeviceInfo { name: DEVICE_NAME_DA2,      description: "Arduino DAC channel 2",             device_type: DeviceType::SignalIO },
];

pub struct ArduinoAdapter;

impl AdapterModule for ArduinoAdapter {
    fn module_name(&self) -> &'static str {
        "arduino"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_HUB     => Some(AnyDevice::Hub(Box::new(ArduinoHub::new()))),
            DEVICE_NAME_SHUTTER => Some(AnyDevice::Shutter(Box::new(ArduinoShutter::new()))),
            DEVICE_NAME_SWITCH  => Some(AnyDevice::StateDevice(Box::new(ArduinoSwitch::new()))),
            DEVICE_NAME_DA1     => Some(AnyDevice::SignalIO(Box::new(ArduinoDa::new(1)))),
            DEVICE_NAME_DA2     => Some(AnyDevice::SignalIO(Box::new(ArduinoDa::new(2)))),
            _ => None,
        }
    }
}
