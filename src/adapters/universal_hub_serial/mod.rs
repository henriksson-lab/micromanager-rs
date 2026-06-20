pub mod device;
pub mod generic_device;
pub mod hub;

pub use device::{UniversalShutter, UniversalStage, UniversalStateDevice, UniversalXYStage};
pub use generic_device::UniversalGeneric;
pub use hub::UniversalHub;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub const DEVICE_NAME_HUB: &str = "UniversalMMHubSerial";
pub const DEVICE_NAME_GENERIC: &str = "UniversalGeneric";
pub const DEVICE_NAME_SHUTTER: &str = "UniversalShutter";
pub const DEVICE_NAME_STATE: &str = "UniversalState";
pub const DEVICE_NAME_STAGE: &str = "UniversalStage";
pub const DEVICE_NAME_XYSTAGE: &str = "UniversalXYStage";

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo {
        name: DEVICE_NAME_HUB,
        description: "Universal hardware hub (serial)",
        device_type: DeviceType::Hub,
    },
    DeviceInfo {
        name: DEVICE_NAME_GENERIC,
        description: "Universal Serial Hub generic child prototype",
        device_type: DeviceType::Generic,
    },
    DeviceInfo {
        name: DEVICE_NAME_SHUTTER,
        description: "Universal Serial Hub shutter child prototype",
        device_type: DeviceType::Shutter,
    },
    DeviceInfo {
        name: DEVICE_NAME_STATE,
        description: "Universal Serial Hub state child prototype",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_STAGE,
        description: "Universal Serial Hub stage child prototype",
        device_type: DeviceType::Stage,
    },
    DeviceInfo {
        name: DEVICE_NAME_XYSTAGE,
        description: "Universal Serial Hub XY stage child prototype",
        device_type: DeviceType::XYStage,
    },
];

pub struct UniversalHubSerialAdapter;

impl AdapterModule for UniversalHubSerialAdapter {
    fn module_name(&self) -> &'static str {
        "universal_hub_serial"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_HUB => Some(AnyDevice::Hub(Box::new(UniversalHub::new()))),
            DEVICE_NAME_GENERIC => Some(AnyDevice::Generic(Box::new(UniversalGeneric::new(
                DEVICE_NAME_GENERIC,
            )))),
            DEVICE_NAME_SHUTTER => Some(AnyDevice::Shutter(Box::new(UniversalShutter::new(
                DEVICE_NAME_SHUTTER,
            )))),
            DEVICE_NAME_STATE => Some(AnyDevice::StateDevice(Box::new(UniversalStateDevice::new(
                DEVICE_NAME_STATE,
            )))),
            DEVICE_NAME_STAGE => Some(AnyDevice::Stage(Box::new(UniversalStage::new(
                DEVICE_NAME_STAGE,
            )))),
            DEVICE_NAME_XYSTAGE => Some(AnyDevice::XYStage(Box::new(UniversalXYStage::new(
                DEVICE_NAME_XYSTAGE,
            )))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_registers_hub_and_generic_prototype() {
        let adapter = UniversalHubSerialAdapter;
        let names: Vec<&str> = adapter.devices().iter().map(|d| d.name).collect();

        assert_eq!(adapter.module_name(), "universal_hub_serial");
        assert!(names.contains(&DEVICE_NAME_HUB));
        assert!(names.contains(&DEVICE_NAME_GENERIC));
        assert!(names.contains(&DEVICE_NAME_SHUTTER));
        assert!(names.contains(&DEVICE_NAME_STATE));
        assert!(names.contains(&DEVICE_NAME_STAGE));
        assert!(names.contains(&DEVICE_NAME_XYSTAGE));
        assert!(matches!(
            adapter.create_device(DEVICE_NAME_HUB),
            Some(AnyDevice::Hub(_))
        ));
        assert!(matches!(
            adapter.create_device(DEVICE_NAME_GENERIC),
            Some(AnyDevice::Generic(_))
        ));
        assert!(matches!(
            adapter.create_device(DEVICE_NAME_SHUTTER),
            Some(AnyDevice::Shutter(_))
        ));
        assert!(matches!(
            adapter.create_device(DEVICE_NAME_STATE),
            Some(AnyDevice::StateDevice(_))
        ));
        assert!(matches!(
            adapter.create_device(DEVICE_NAME_STAGE),
            Some(AnyDevice::Stage(_))
        ));
        assert!(matches!(
            adapter.create_device(DEVICE_NAME_XYSTAGE),
            Some(AnyDevice::XYStage(_))
        ));
    }
}
