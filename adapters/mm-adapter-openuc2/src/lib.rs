//! OpenUC2 device adapter.
//!
//! JSON protocol over serial (newline-terminated commands, \r terminated responses):
//! - Firmware check: {"task":"/state_get"}  → JSON with "UC2_Feather" or similar
//! - XY move: {"task":"/motor_act","motor":{"steppers":[{"stepperid":1,"position":<x>,"speed":5000,"isabs":1},{"stepperid":2,"position":<y>,"speed":5000,"isabs":1}]}}
//! - Z move:  {"task":"/motor_act","motor":{"steppers":[{"stepperid":3,"position":<z>,"speed":2000,"isabs":1}]}}
//! - Shutter: {"task":"/laser_act","LASERid":1,"LASERval":<0 or 255>}
//!
//! Devices exported:
//! - `UC2Hub`    — Hub
//! - `XYStage`   — XYStage
//! - `ZStage`    — Stage
//! - `UC2Shutter` — Shutter

pub mod hub;
pub mod xystage;
pub mod zstage;
pub mod shutter;

pub use hub::Uc2Hub;
pub use xystage::Uc2XYStage;
pub use zstage::Uc2ZStage;
pub use shutter::Uc2Shutter;

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

pub const DEVICE_NAME_HUB: &str = "UC2Hub";
pub const DEVICE_NAME_XYSTAGE: &str = "UC2XYStage";
pub const DEVICE_NAME_ZSTAGE: &str = "UC2ZStage";
pub const DEVICE_NAME_SHUTTER: &str = "UC2Shutter";

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo { name: DEVICE_NAME_HUB,     description: "openUC2 hub device",                device_type: DeviceType::Hub },
    DeviceInfo { name: DEVICE_NAME_XYSTAGE,  description: "XY Stage for openUC2",              device_type: DeviceType::XYStage },
    DeviceInfo { name: DEVICE_NAME_ZSTAGE,   description: "Z Stage for openUC2",               device_type: DeviceType::Stage },
    DeviceInfo { name: DEVICE_NAME_SHUTTER,  description: "LED/Laser Shutter for openUC2",     device_type: DeviceType::Shutter },
];

pub struct Uc2Adapter;

impl AdapterModule for Uc2Adapter {
    fn module_name(&self) -> &'static str { "openuc2" }
    fn devices(&self) -> &'static [DeviceInfo] { DEVICE_LIST }
    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_HUB     => Some(AnyDevice::Hub(Box::new(Uc2Hub::new()))),
            DEVICE_NAME_XYSTAGE => Some(AnyDevice::XYStage(Box::new(Uc2XYStage::new()))),
            DEVICE_NAME_ZSTAGE  => Some(AnyDevice::Stage(Box::new(Uc2ZStage::new()))),
            DEVICE_NAME_SHUTTER => Some(AnyDevice::Shutter(Box::new(Uc2Shutter::new()))),
            _ => None,
        }
    }
}
