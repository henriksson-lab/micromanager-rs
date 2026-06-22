pub mod xy_stage;
pub mod z_stage;
pub use xy_stage::LStepXYStage;
pub use z_stage::LStepZStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct MarzhauserLStepAdapter;

const DEVICES: &[DeviceInfo] = &[
    DeviceInfo {
        name: "XYStage",
        description: "L-Step XY stage driver adapter",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: "ZAxis",
        description: "L-Step Z axis driver",
        device_type: DeviceType::Stage,
    },
];

impl AdapterModule for MarzhauserLStepAdapter {
    fn module_name(&self) -> &'static str {
        "MarzhauserLStep"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "XYStage" => Some(AnyDevice::XYStage(Box::new(LStepXYStage::new()))),
            "ZAxis" => Some(AnyDevice::Stage(Box::new(LStepZStage::new()))),
            _ => None,
        }
    }
}
