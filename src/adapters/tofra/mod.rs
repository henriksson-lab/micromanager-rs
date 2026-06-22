pub mod filter_wheel;
pub mod xy_stage;
pub mod z_stage;
pub use filter_wheel::TofraFilterWheel;
pub use xy_stage::TofraXYStage;
pub use z_stage::TofraZStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct TofraAdapter;

const DEVICES: &[DeviceInfo] = &[
    DeviceInfo {
        name: "TOFRA Filter Wheel",
        description: "TOFRA Filter Wheel with Integrated Controller 10, 12, 18 or 22 pos.",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TOFRA XYStage",
        description: "TOFRA XYStage with Integrated Controller",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: "TOFRA Z-Drive",
        description: "TOFRA Z-Drive with Integrated Controller",
        device_type: DeviceType::Stage,
    },
];

impl AdapterModule for TofraAdapter {
    fn module_name(&self) -> &'static str {
        "Tofra"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "TOFRA Filter Wheel" => Some(AnyDevice::StateDevice(Box::new(TofraFilterWheel::new()))),
            "TOFRA XYStage" => Some(AnyDevice::XYStage(Box::new(TofraXYStage::new()))),
            "TOFRA Z-Drive" => Some(AnyDevice::Stage(Box::new(TofraZStage::new()))),
            _ => None,
        }
    }
}
