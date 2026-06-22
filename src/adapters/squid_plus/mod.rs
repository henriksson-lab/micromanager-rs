pub mod common;
pub mod filter_wheel;
pub mod protocol;
pub mod xy_stage;
pub mod z_stage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub use filter_wheel::SquidPlusFilterWheel;
pub use xy_stage::SquidPlusXYStage;
pub use z_stage::SquidPlusZStage;

pub const DEVICE_NAME_FILTER_WHEEL: &str = "SquidPlusFilterWheel";
pub const DEVICE_NAME_Z_STAGE: &str = "SquidPlusZStage";
pub const DEVICE_NAME_XY_STAGE: &str = "SquidPlusXYStage";

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo {
        name: DEVICE_NAME_FILTER_WHEEL,
        description: "Squid+ filter wheel (8-position, stepper on W axis)",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_Z_STAGE,
        description: "Squid+ Z focus stage",
        device_type: DeviceType::Stage,
    },
    DeviceInfo {
        name: DEVICE_NAME_XY_STAGE,
        description: "Squid+ XY translation stage",
        device_type: DeviceType::XYStage,
    },
];

pub struct SquidPlusAdapter;

impl AdapterModule for SquidPlusAdapter {
    fn module_name(&self) -> &'static str {
        "SquidPlus"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_FILTER_WHEEL => Some(AnyDevice::StateDevice(Box::new(
                SquidPlusFilterWheel::new(),
            ))),
            DEVICE_NAME_Z_STAGE => Some(AnyDevice::Stage(Box::new(SquidPlusZStage::new()))),
            DEVICE_NAME_XY_STAGE => Some(AnyDevice::XYStage(Box::new(SquidPlusXYStage::new()))),
            _ => None,
        }
    }
}
