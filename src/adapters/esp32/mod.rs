//! ESP32 device adapter.
//!
//! ASCII protocol (all commands terminated with \r\n):
//! - Version:    send `V`       → "MM-ESP32,<version>"
//! - Stage range: send `U,<axis>` → "U,<range_um>"   (axis 0=X, 1=Y, 2=Z)
//! - Switch:     send `S,<val>\r\n` (simplex, no handshake)
//! - DA/PWM:     send `O,<ch>,<val>\r\n`
//! - XY move (relative): send `mrx <steps>\r\n` then `mry <steps>\r\n`
//! - Z move (relative):  send `mrz <steps>\r\n`
//! - Trigger:    send `R\r\n` (start), `E\r\n` (end) → "E,<count>"
//!
//! Devices exported:
//! - `ESP32-Hub`     — Hub
//! - `ESP32-Switch`  — 8-bit StateDevice (12 positions)
//! - `ESP32-Shutter` — Shutter
//! - `ESP32-PWM0..4` — SignalIO PWM channels (5 channels, 0-based)
//! - `ZStage`        — Z Stage
//! - `XYStage`       — XY Stage

pub mod hub;
pub mod input;
pub mod pwm;
pub mod shutter;
pub mod switch;
pub mod xystage;
pub mod zstage;

pub use hub::Esp32Hub;
pub use input::Esp32Input;
pub use pwm::Esp32Pwm;
pub use shutter::Esp32Shutter;
pub use switch::Esp32Switch;
pub use xystage::Esp32XYStage;
pub use zstage::Esp32ZStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub const DEVICE_NAME_HUB: &str = "ESP32-Hub";
pub const DEVICE_NAME_SWITCH: &str = "ESP32-Switch";
pub const DEVICE_NAME_SHUTTER: &str = "ESP32-Shutter";
pub const DEVICE_NAME_INPUT: &str = "ESP32-Input";
pub const DEVICE_NAME_PWM0: &str = "ESP32-PWM0";
pub const DEVICE_NAME_PWM1: &str = "ESP32-PWM1";
pub const DEVICE_NAME_PWM2: &str = "ESP32-PWM2";
pub const DEVICE_NAME_PWM3: &str = "ESP32-PWM3";
pub const DEVICE_NAME_PWM4: &str = "ESP32-PWM4";
pub const DEVICE_NAME_ZSTAGE: &str = "ZStage";
pub const DEVICE_NAME_XYSTAGE: &str = "XYStage";

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo {
        name: DEVICE_NAME_HUB,
        description: "Hub (required)",
        device_type: DeviceType::Hub,
    },
    DeviceInfo {
        name: DEVICE_NAME_SWITCH,
        description: "Switch on/off channels 0 to 10",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_SHUTTER,
        description: "Shutter",
        device_type: DeviceType::Shutter,
    },
    DeviceInfo {
        name: DEVICE_NAME_PWM0,
        description: "PWM channel 0",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: DEVICE_NAME_PWM1,
        description: "PWM channel 1",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: DEVICE_NAME_PWM2,
        description: "PWM channel 2",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: DEVICE_NAME_PWM3,
        description: "PWM channel 3",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: DEVICE_NAME_PWM4,
        description: "PWM channel 4",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: DEVICE_NAME_ZSTAGE,
        description: "Z Stage",
        device_type: DeviceType::Stage,
    },
    DeviceInfo {
        name: DEVICE_NAME_XYSTAGE,
        description: "XY Stage",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: DEVICE_NAME_INPUT,
        description: "ADC",
        device_type: DeviceType::Generic,
    },
];

pub struct Esp32Adapter;

impl AdapterModule for Esp32Adapter {
    fn module_name(&self) -> &'static str {
        "esp32"
    }
    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }
    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_HUB => Some(AnyDevice::Hub(Box::new(Esp32Hub::new()))),
            DEVICE_NAME_SWITCH => Some(AnyDevice::StateDevice(Box::new(Esp32Switch::new()))),
            DEVICE_NAME_SHUTTER => Some(AnyDevice::Shutter(Box::new(Esp32Shutter::new()))),
            DEVICE_NAME_INPUT => Some(AnyDevice::Generic(Box::new(Esp32Input::new()))),
            DEVICE_NAME_PWM0 => Some(AnyDevice::SignalIO(Box::new(Esp32Pwm::new(0)))),
            DEVICE_NAME_PWM1 => Some(AnyDevice::SignalIO(Box::new(Esp32Pwm::new(1)))),
            DEVICE_NAME_PWM2 => Some(AnyDevice::SignalIO(Box::new(Esp32Pwm::new(2)))),
            DEVICE_NAME_PWM3 => Some(AnyDevice::SignalIO(Box::new(Esp32Pwm::new(3)))),
            DEVICE_NAME_PWM4 => Some(AnyDevice::SignalIO(Box::new(Esp32Pwm::new(4)))),
            DEVICE_NAME_ZSTAGE => Some(AnyDevice::Stage(Box::new(Esp32ZStage::new()))),
            DEVICE_NAME_XYSTAGE => Some(AnyDevice::XYStage(Box::new(Esp32XYStage::new()))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_devices_match_upstream_order_descriptions_and_types() {
        let devices = Esp32Adapter.devices();
        let actual: Vec<_> = devices
            .iter()
            .map(|d| (d.name, d.description, d.device_type))
            .collect();

        assert_eq!(
            actual,
            vec![
                (DEVICE_NAME_HUB, "Hub (required)", DeviceType::Hub),
                (
                    DEVICE_NAME_SWITCH,
                    "Switch on/off channels 0 to 10",
                    DeviceType::State
                ),
                (DEVICE_NAME_SHUTTER, "Shutter", DeviceType::Shutter),
                (DEVICE_NAME_PWM0, "PWM channel 0", DeviceType::SignalIO),
                (DEVICE_NAME_PWM1, "PWM channel 1", DeviceType::SignalIO),
                (DEVICE_NAME_PWM2, "PWM channel 2", DeviceType::SignalIO),
                (DEVICE_NAME_PWM3, "PWM channel 3", DeviceType::SignalIO),
                (DEVICE_NAME_PWM4, "PWM channel 4", DeviceType::SignalIO),
                (DEVICE_NAME_ZSTAGE, "Z Stage", DeviceType::Stage),
                (DEVICE_NAME_XYSTAGE, "XY Stage", DeviceType::XYStage),
                (DEVICE_NAME_INPUT, "ADC", DeviceType::Generic),
            ]
        );
    }
}
