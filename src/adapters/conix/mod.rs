pub mod filter;
pub mod xy_stage;
pub mod z_stage;
pub use filter::{ConixHexFilter, ConixQuadFilter};
pub use xy_stage::ConixXYStage;
pub use z_stage::ConixZStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct ConixAdapter;

const DEVICES: &[DeviceInfo] = &[
    DeviceInfo {
        name: "ConixQuadFilter",
        description: "Conix Motorized Qud-Filter changer for Nikon TE200/300",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "ConixHexFilter",
        description: "Conix Motorized Hexa-Filter changer for Nikon TE200/300",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "ConixXYStage",
        description: "Conix XY stage driver",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: "ConixZStage",
        description: "Conix Z stage driver",
        device_type: DeviceType::Stage,
    },
];

impl AdapterModule for ConixAdapter {
    fn module_name(&self) -> &'static str {
        "Conix"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "ConixQuadFilter" => Some(AnyDevice::StateDevice(Box::new(ConixQuadFilter::new()))),
            "ConixHexFilter" => Some(AnyDevice::StateDevice(Box::new(ConixHexFilter::new()))),
            "ConixXYStage" => Some(AnyDevice::XYStage(Box::new(ConixXYStage::new()))),
            "ConixZStage" => Some(AnyDevice::Stage(Box::new(ConixZStage::new()))),
            _ => None,
        }
    }
}
