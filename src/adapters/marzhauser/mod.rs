pub mod xy_stage;
pub use xy_stage::MarzhauserXYStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct MarzhauserAdapter;

const DEVICES: &[DeviceInfo] = &[DeviceInfo {
    name: "XYStage",
    description: "Tango XY stage driver adapter",
    device_type: DeviceType::XYStage,
}];

impl AdapterModule for MarzhauserAdapter {
    fn module_name(&self) -> &'static str {
        "Marzhauser"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "XYStage" => Some(AnyDevice::XYStage(Box::new(MarzhauserXYStage::new()))),
            _ => None,
        }
    }
}
