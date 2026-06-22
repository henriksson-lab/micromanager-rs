pub mod xy_stage;
pub mod z_stage;
pub use xy_stage::ScientificaXYStage;
pub use z_stage::ScientificaZStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct ScientificaAdapter;

const DEVICES: &[DeviceInfo] = &[
    DeviceInfo {
        name: "ScientificaXYStage",
        description: "Scientifica XY stage",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: "ScientificaZStage",
        description: "Scientifica Z stage",
        device_type: DeviceType::Stage,
    },
];

impl AdapterModule for ScientificaAdapter {
    fn module_name(&self) -> &'static str {
        "Scientifica"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "ScientificaXYStage" => Some(AnyDevice::XYStage(Box::new(ScientificaXYStage::new()))),
            "ScientificaZStage" => Some(AnyDevice::Stage(Box::new(ScientificaZStage::new()))),
            _ => None,
        }
    }
}
