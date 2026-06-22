pub mod corvus;
pub use corvus::CorvusXYStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct CorvusAdapter;

const DEVICES: &[DeviceInfo] = &[DeviceInfo {
    name: "XY Stage",
    description: "XY Stage",
    device_type: DeviceType::XYStage,
}];

impl AdapterModule for CorvusAdapter {
    fn module_name(&self) -> &'static str {
        "Corvus"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "XY Stage" => Some(AnyDevice::XYStage(Box::new(CorvusXYStage::new()))),
            _ => None,
        }
    }
}
