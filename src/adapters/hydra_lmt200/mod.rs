pub mod hydra;
pub use hydra::HydraXYStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct HydraLmt200Adapter;

const DEVICES: &[DeviceInfo] = &[DeviceInfo {
    name: "XY Stage",
    description: "Hydra XY stage driver adapter",
    device_type: DeviceType::XYStage,
}];

impl AdapterModule for HydraLmt200Adapter {
    fn module_name(&self) -> &'static str {
        "HydraLMT200"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "XY Stage" => Some(AnyDevice::XYStage(Box::new(HydraXYStage::new()))),
            _ => None,
        }
    }
}
