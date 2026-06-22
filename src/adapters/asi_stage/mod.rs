pub mod xy_stage;
pub mod z_stage;
pub use xy_stage::AsiXYStage;
pub use z_stage::AsiZStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct AsiStageAdapter;

const DEVICES: &[DeviceInfo] = &[
    DeviceInfo {
        name: "ASI-XYStage",
        description: "ASI XY-stage",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: "ZStage",
        description: "ASI Z Stage",
        device_type: DeviceType::Stage,
    },
];

impl AdapterModule for AsiStageAdapter {
    fn module_name(&self) -> &'static str {
        "ASIStage"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "ASI-XYStage" => Some(AnyDevice::XYStage(Box::new(AsiXYStage::new()))),
            "ZStage" => Some(AnyDevice::Stage(Box::new(AsiZStage::new()))),
            _ => None,
        }
    }
}
