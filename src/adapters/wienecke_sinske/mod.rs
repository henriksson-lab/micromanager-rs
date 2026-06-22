pub mod xy_stage;
pub mod z_stage;
pub use xy_stage::WSXYStage;
pub use z_stage::WSZStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct WieneckeSinskeAdapter;

const DEVICES: &[DeviceInfo] = &[
    DeviceInfo {
        name: "WS-XYStage",
        description: "Wienecke & Sinske WSB PiezoDrive XY stage",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: "WS-ZStage",
        description: "Wienecke & Sinske WSB ZPiezo stage",
        device_type: DeviceType::Stage,
    },
];

impl AdapterModule for WieneckeSinskeAdapter {
    fn module_name(&self) -> &'static str {
        "WieneckeSinske"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "WS-XYStage" => Some(AnyDevice::XYStage(Box::new(WSXYStage::new()))),
            "WS-ZStage" => Some(AnyDevice::Stage(Box::new(WSZStage::new()))),
            _ => None,
        }
    }
}
