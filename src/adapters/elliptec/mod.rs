pub mod slider;
pub mod stage;
pub(crate) mod status;
pub use slider::{ElliptecSlider, ElliptecSliderModel};
pub use stage::ElliptecStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo {
        name: "Thorlabs ELL17/ELL20",
        description: "Thorlabs ELL17/ELL20",
        device_type: DeviceType::Stage,
    },
    DeviceInfo {
        name: "Thorlabs ELL9",
        description: "Thorlabs ELL9",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "Thorlabs ELL6",
        description: "Thorlabs ELL6",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "Thorlabs ELL6 shutter",
        description: "Thorlabs ELL6 shutter",
        device_type: DeviceType::Shutter,
    },
];

pub struct ElliptecAdapter;

impl AdapterModule for ElliptecAdapter {
    fn module_name(&self) -> &'static str {
        "ThorlabsElliptecSlider"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "Thorlabs ELL17/ELL20" => Some(AnyDevice::Stage(Box::new(ElliptecStage::default()))),
            "Thorlabs ELL9" => Some(AnyDevice::StateDevice(Box::new(ElliptecSlider::ell9('0')))),
            "Thorlabs ELL6" => Some(AnyDevice::StateDevice(Box::new(ElliptecSlider::new('0')))),
            "Thorlabs ELL6 shutter" => Some(AnyDevice::Shutter(Box::new(
                ElliptecSlider::ell6_shutter('0'),
            ))),
            _ => None,
        }
    }
}
