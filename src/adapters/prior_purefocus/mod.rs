pub mod offset_stage;
pub use offset_stage::PureFocusOffsetStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct PriorPureFocusAdapter;

const DEVICES: &[DeviceInfo] = &[DeviceInfo {
    name: "PureFocusOffset",
    description: "PureFocusOffset Drive",
    device_type: DeviceType::Stage,
}];

impl AdapterModule for PriorPureFocusAdapter {
    fn module_name(&self) -> &'static str {
        "PriorPureFocus"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "PureFocusOffset" => Some(AnyDevice::Stage(Box::new(PureFocusOffsetStage::new()))),
            _ => None,
        }
    }
}
