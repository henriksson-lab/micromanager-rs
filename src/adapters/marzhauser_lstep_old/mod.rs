pub mod xy_stage;
pub use xy_stage::LStepOldXYStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct MarzhauserLStepOldAdapter;

const DEVICES: &[DeviceInfo] = &[DeviceInfo {
    name: "XYStage",
    description: "LStepOld XY Stage",
    device_type: DeviceType::XYStage,
}];

impl AdapterModule for MarzhauserLStepOldAdapter {
    fn module_name(&self) -> &'static str {
        "MarzhauserLStepOld"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "XYStage" => Some(AnyDevice::XYStage(Box::new(LStepOldXYStage::new()))),
            _ => None,
        }
    }
}
