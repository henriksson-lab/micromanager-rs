pub mod hub;
pub mod shutter;
pub mod turret;
pub mod xy_stage;
pub mod z_stage;

pub use hub::ZeissHub;
pub use shutter::ZeissShutter;
pub use turret::{TurretId, ZeissTurret};
pub use xy_stage::ZeissMcu28XYStage;
pub use z_stage::ZeissFocusStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

use hub::{
    DEVICE_NAME_BASE_PORT, DEVICE_NAME_CONDENSER, DEVICE_NAME_EXT_FILTER, DEVICE_NAME_FILTER1,
    DEVICE_NAME_FILTER2, DEVICE_NAME_FOCUS, DEVICE_NAME_HALOGEN, DEVICE_NAME_HUB,
    DEVICE_NAME_LAMP_MIRROR, DEVICE_NAME_OBJECTIVES, DEVICE_NAME_OPTOVAR, DEVICE_NAME_REFLECTOR,
    DEVICE_NAME_SHUTTER, DEVICE_NAME_SHUTTER_MF, DEVICE_NAME_SIDE_PORT, DEVICE_NAME_TUBELENS,
    DEVICE_NAME_XY,
};

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo {
        name: DEVICE_NAME_HUB,
        description: "Zeiss CAN-bus hub",
        device_type: DeviceType::Hub,
    },
    DeviceInfo {
        name: DEVICE_NAME_SHUTTER,
        description: "Shutter",
        device_type: DeviceType::Shutter,
    },
    DeviceInfo {
        name: DEVICE_NAME_SHUTTER_MF,
        description: "ShutterMFFirmware",
        device_type: DeviceType::Shutter,
    },
    DeviceInfo {
        name: DEVICE_NAME_FOCUS,
        description: "Z-drive",
        device_type: DeviceType::Stage,
    },
    DeviceInfo {
        name: DEVICE_NAME_XY,
        description: "XY Stage",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: DEVICE_NAME_REFLECTOR,
        description: "Zeiss Reflector Turret adapter",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_OBJECTIVES,
        description: "Zeiss Objective Turret adapter",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_OPTOVAR,
        description: "Zeiss Optovar Turret adapter",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_FILTER1,
        description: "Zeiss FilterWheel1 adapter",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_FILTER2,
        description: "Zeiss FilterWheel2 adapter",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_CONDENSER,
        description: "Zeiss Condenser adapter",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_TUBELENS,
        description: "Zeiss Tubelens Turret adapter",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_BASE_PORT,
        description: "Zeiss BasePort Slider adapter",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_SIDE_PORT,
        description: "Zeiss SidePort Turret adapter",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_LAMP_MIRROR,
        description: "Zeiss Lamp Mirror adapter",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_EXT_FILTER,
        description: "Zeiss External FilterWheel adapter",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_HALOGEN,
        description: "Zeiss Halogen Lamp",
        device_type: DeviceType::Shutter,
    },
];

pub struct ZeissCanAdapter;

impl AdapterModule for ZeissCanAdapter {
    fn module_name(&self) -> &'static str {
        "zeiss_can"
    }
    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }
    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_HUB => Some(AnyDevice::Hub(Box::new(ZeissHub::new()))),
            DEVICE_NAME_SHUTTER => Some(AnyDevice::Shutter(Box::new(ZeissShutter::new()))),
            DEVICE_NAME_SHUTTER_MF => Some(AnyDevice::Shutter(Box::new(ZeissShutter::new_mf()))),
            DEVICE_NAME_FOCUS => Some(AnyDevice::Stage(Box::new(ZeissFocusStage::new()))),
            DEVICE_NAME_XY => Some(AnyDevice::XYStage(Box::new(ZeissMcu28XYStage::new()))),
            DEVICE_NAME_REFLECTOR => Some(AnyDevice::StateDevice(Box::new(ZeissTurret::new(
                TurretId::Reflector,
                6,
            )))),
            DEVICE_NAME_OBJECTIVES => Some(AnyDevice::StateDevice(Box::new(ZeissTurret::new(
                TurretId::Objective,
                6,
            )))),
            DEVICE_NAME_OPTOVAR => Some(AnyDevice::StateDevice(Box::new(ZeissTurret::new(
                TurretId::Optovar,
                6,
            )))),
            DEVICE_NAME_FILTER1 => Some(AnyDevice::StateDevice(Box::new(ZeissTurret::new(
                TurretId::FilterWheel1,
                6,
            )))),
            DEVICE_NAME_FILTER2 => Some(AnyDevice::StateDevice(Box::new(ZeissTurret::new(
                TurretId::FilterWheel2,
                6,
            )))),
            DEVICE_NAME_CONDENSER => Some(AnyDevice::StateDevice(Box::new(ZeissTurret::new(
                TurretId::Condenser,
                6,
            )))),
            DEVICE_NAME_TUBELENS => Some(AnyDevice::StateDevice(Box::new(ZeissTurret::new(
                TurretId::TubeLens,
                6,
            )))),
            DEVICE_NAME_BASE_PORT => Some(AnyDevice::StateDevice(Box::new(ZeissTurret::new(
                TurretId::BasePort,
                5,
            )))),
            DEVICE_NAME_SIDE_PORT => Some(AnyDevice::StateDevice(Box::new(ZeissTurret::new(
                TurretId::SidePort,
                5,
            )))),
            DEVICE_NAME_LAMP_MIRROR => Some(AnyDevice::StateDevice(Box::new(ZeissTurret::new(
                TurretId::LampMirror,
                2,
            )))),
            DEVICE_NAME_EXT_FILTER => Some(AnyDevice::StateDevice(Box::new(ZeissTurret::new(
                TurretId::ExternalFilterWheel,
                6,
            )))),
            _ => None,
        }
    }
}
