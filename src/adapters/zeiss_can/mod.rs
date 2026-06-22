pub mod halogen_lamp;
pub mod hub;
pub mod shutter;
pub mod turret;
pub mod xy_stage;
pub mod z_stage;

pub use halogen_lamp::ZeissHalogenLamp;
pub use hub::ZeissHub;
pub use shutter::ZeissShutter;
pub use turret::{TurretId, ZeissTurret};
pub use xy_stage::ZeissMcu28XYStage;
pub use z_stage::ZeissFocusStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

use hub::{
    DEVICE_NAME_BASE_PORT, DEVICE_NAME_CONDENSER, DEVICE_NAME_EXT_FILTER, DEVICE_NAME_FILTER1,
    DEVICE_NAME_FILTER2, DEVICE_NAME_FOCUS, DEVICE_NAME_HALOGEN_LAMP, DEVICE_NAME_HUB,
    DEVICE_NAME_LAMP_MIRROR, DEVICE_NAME_OBJECTIVES, DEVICE_NAME_OPTOVAR, DEVICE_NAME_REFLECTOR,
    DEVICE_NAME_SHUTTER, DEVICE_NAME_SHUTTER_MF, DEVICE_NAME_SIDE_PORT, DEVICE_NAME_TUBELENS,
    DEVICE_NAME_XY, DEVICE_NAME_Z_STAGE,
};

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo {
        name: DEVICE_NAME_HUB,
        description: "Zeiss Axiovert 200m controlled through serial interface",
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
        description: "Z-Drive",
        device_type: DeviceType::Stage,
    },
    DeviceInfo {
        name: DEVICE_NAME_XY,
        description: "XY Stage (MCU 28)",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: DEVICE_NAME_Z_STAGE,
        description: "Z Stage on Axioskop 2",
        device_type: DeviceType::Stage,
    },
    DeviceInfo {
        name: DEVICE_NAME_REFLECTOR,
        description: "Reflector Turret (dichroics)",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_OBJECTIVES,
        description: "Objective Turret",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_OPTOVAR,
        description: "Optovar",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_FILTER1,
        description: "FilterWheel 1",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_FILTER2,
        description: "FilterWheel 2",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_CONDENSER,
        description: "Condenser Turret",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_TUBELENS,
        description: "Tubelens",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_BASE_PORT,
        description: "BasePort Slider",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_SIDE_PORT,
        description: "SidePort switcher",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_LAMP_MIRROR,
        description: "Lamp Switcher",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_HALOGEN_LAMP,
        description: "Halogen Lamp",
        device_type: DeviceType::Shutter,
    },
    DeviceInfo {
        name: DEVICE_NAME_EXT_FILTER,
        description: "External FilterWheel",
        device_type: DeviceType::State,
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
            DEVICE_NAME_Z_STAGE => Some(AnyDevice::Stage(Box::new(ZeissFocusStage::new_z_stage()))),
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
            DEVICE_NAME_HALOGEN_LAMP => Some(AnyDevice::Shutter(Box::new(ZeissHalogenLamp::new()))),
            DEVICE_NAME_EXT_FILTER => Some(AnyDevice::StateDevice(Box::new(ZeissTurret::new(
                TurretId::ExternalFilterWheel,
                6,
            )))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_descriptions_match_upstream_registration() {
        let adapter = ZeissCanAdapter;
        let descriptions: Vec<(&str, &str)> = adapter
            .devices()
            .iter()
            .map(|info| (info.name, info.description))
            .collect();

        assert!(descriptions.contains(&(
            DEVICE_NAME_HUB,
            "Zeiss Axiovert 200m controlled through serial interface"
        )));
        assert!(descriptions.contains(&(DEVICE_NAME_REFLECTOR, "Reflector Turret (dichroics)")));
        assert!(descriptions.contains(&(DEVICE_NAME_SIDE_PORT, "SidePort switcher")));
        assert!(descriptions.contains(&(DEVICE_NAME_BASE_PORT, "BasePort Slider")));
        assert!(descriptions.contains(&(DEVICE_NAME_OBJECTIVES, "Objective Turret")));
        assert!(descriptions.contains(&(DEVICE_NAME_CONDENSER, "Condenser Turret")));
        assert!(descriptions.contains(&(DEVICE_NAME_OPTOVAR, "Optovar")));
        assert!(descriptions.contains(&(DEVICE_NAME_TUBELENS, "Tubelens")));
        assert!(descriptions.contains(&(DEVICE_NAME_LAMP_MIRROR, "Lamp Switcher")));
        assert!(descriptions.contains(&(DEVICE_NAME_HALOGEN_LAMP, "Halogen Lamp")));
        assert!(descriptions.contains(&(DEVICE_NAME_FOCUS, "Z-Drive")));
        assert!(descriptions.contains(&(DEVICE_NAME_EXT_FILTER, "External FilterWheel")));
        assert!(descriptions.contains(&(DEVICE_NAME_FILTER1, "FilterWheel 1")));
        assert!(descriptions.contains(&(DEVICE_NAME_FILTER2, "FilterWheel 2")));
        assert!(descriptions.contains(&(DEVICE_NAME_XY, "XY Stage (MCU 28)")));
        assert!(descriptions.contains(&(DEVICE_NAME_Z_STAGE, "Z Stage on Axioskop 2")));
    }
}
