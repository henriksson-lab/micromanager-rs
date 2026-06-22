pub mod stage;
pub mod xy_stage;
pub use stage::ZaberStage;
pub use xy_stage::ZaberXYStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct ZaberAdapter;

const DEVICES: &[DeviceInfo] = &[
    DeviceInfo {
        name: "XYStage",
        description: "Zaber XY Stage",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: "Stage",
        description: "Zaber Stage",
        device_type: DeviceType::Stage,
    },
];

impl AdapterModule for ZaberAdapter {
    fn module_name(&self) -> &'static str {
        "Zaber"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "Stage" | "ZaberStage" => Some(AnyDevice::Stage(Box::new(ZaberStage::new()))),
            "XYStage" | "ZaberXYStage" => Some(AnyDevice::XYStage(Box::new(ZaberXYStage::new()))),
            _ => None,
        }
    }
}
