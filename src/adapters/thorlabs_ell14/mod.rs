pub mod ell14;
pub use ell14::ThorlabsEll14;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: " ELL14",
    description: " ELL14",
    device_type: DeviceType::Generic,
}];

pub struct ThorlabsEll14Adapter;

impl AdapterModule for ThorlabsEll14Adapter {
    fn module_name(&self) -> &'static str {
        "Thorlabs_ELL14"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            " ELL14" => Some(AnyDevice::Generic(Box::new(ThorlabsEll14::new()))),
            _ => None,
        }
    }
}
