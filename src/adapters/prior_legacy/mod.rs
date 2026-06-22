pub mod xy_stage;
pub mod z_stage;
pub use xy_stage::PriorLegacyXYStage;
pub use z_stage::PriorLegacyZStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct PriorLegacyAdapter;

const DEVICES: &[DeviceInfo] = &[
    DeviceInfo {
        name: "XYStage",
        description: "Legacy XY Stage",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: "ZStage",
        description: "Legacy Z stage",
        device_type: DeviceType::Stage,
    },
];

impl AdapterModule for PriorLegacyAdapter {
    fn module_name(&self) -> &'static str {
        "PriorLegacy"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "XYStage" => Some(AnyDevice::XYStage(Box::new(PriorLegacyXYStage::new()))),
            "ZStage" => Some(AnyDevice::Stage(Box::new(PriorLegacyZStage::new()))),
            _ => None,
        }
    }
}
