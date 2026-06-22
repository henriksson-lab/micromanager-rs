pub mod xy_stage;
pub mod z_stage;
pub use xy_stage::AsiTigerXYStage;
pub use z_stage::AsiTigerZStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct AsiTigerAdapter;

const DEVICES: &[DeviceInfo] = &[
    DeviceInfo {
        name: "AsiTigerXYStage",
        description: "ASI Tiger XY Stage",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: "AsiTigerZStage",
        description: "ASI Tiger Z Stage",
        device_type: DeviceType::Stage,
    },
];

impl AdapterModule for AsiTigerAdapter {
    fn module_name(&self) -> &'static str {
        "ASITiger"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "AsiTigerXYStage" => Some(AnyDevice::XYStage(Box::new(AsiTigerXYStage::new()))),
            "AsiTigerZStage" => Some(AnyDevice::Stage(Box::new(AsiTigerZStage::new()))),
            _ => None,
        }
    }
}
