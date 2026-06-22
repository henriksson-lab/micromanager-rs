pub mod shutter;
pub mod wheel;
pub mod xy_stage;
pub mod z_stage;
pub use shutter::PriorShutter;
pub use wheel::PriorWheel;
pub use xy_stage::PriorXYStage;
pub use z_stage::PriorZStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct PriorAdapter;

const DEVICES: &[DeviceInfo] = &[
    DeviceInfo {
        name: "PriorXYStage",
        description: "Prior Scientific ProScan XY stage",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: "PriorZStage",
        description: "Prior Scientific ProScan Z stage",
        device_type: DeviceType::Stage,
    },
    DeviceInfo {
        name: "Shutter-1",
        description: "Pro Scan shutter 1",
        device_type: DeviceType::Shutter,
    },
    DeviceInfo {
        name: "Shutter-2",
        description: "Pro Scan shutter 2",
        device_type: DeviceType::Shutter,
    },
    DeviceInfo {
        name: "Shutter-3",
        description: "Pro Scan shutter 3",
        device_type: DeviceType::Shutter,
    },
    DeviceInfo {
        name: "Wheel-1",
        description: "Pro Scan filter wheel 1",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "Wheel-2",
        description: "Pro Scan filter wheel 2",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "Wheel-3",
        description: "Pro Scan filter wheel 3",
        device_type: DeviceType::State,
    },
];

impl AdapterModule for PriorAdapter {
    fn module_name(&self) -> &'static str {
        "Prior"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "PriorXYStage" => Some(AnyDevice::XYStage(Box::new(PriorXYStage::new()))),
            "PriorZStage" => Some(AnyDevice::Stage(Box::new(PriorZStage::new()))),
            "Shutter-1" => Some(AnyDevice::Shutter(Box::new(PriorShutter::new(1)))),
            "Shutter-2" => Some(AnyDevice::Shutter(Box::new(PriorShutter::new(2)))),
            "Shutter-3" => Some(AnyDevice::Shutter(Box::new(PriorShutter::new(3)))),
            "Wheel-1" => Some(AnyDevice::StateDevice(Box::new(PriorWheel::new(1)))),
            "Wheel-2" => Some(AnyDevice::StateDevice(Box::new(PriorWheel::new(2)))),
            "Wheel-3" => Some(AnyDevice::StateDevice(Box::new(PriorWheel::new(3)))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::AdapterModule;

    #[test]
    fn registers_upstream_shutter_names() {
        let adapter = PriorAdapter;
        let names: Vec<_> = adapter.devices().iter().map(|d| d.name).collect();
        assert!(names.contains(&"Shutter-1"));
        assert!(names.contains(&"Shutter-2"));
        assert!(names.contains(&"Shutter-3"));
        assert!(!names.contains(&"PriorShutter"));
        assert!(adapter.create_device("PriorShutter").is_none());

        let dev = adapter.create_device("Shutter-2").unwrap();
        match dev {
            AnyDevice::Shutter(s) => assert_eq!(s.name(), "Shutter-2"),
            _ => panic!("expected shutter"),
        }
    }

    #[test]
    fn registers_upstream_wheel_descriptions() {
        let adapter = PriorAdapter;
        let devices = adapter.devices();
        assert_eq!(
            devices
                .iter()
                .find(|d| d.name == "Wheel-1")
                .unwrap()
                .description,
            "Pro Scan filter wheel 1"
        );
        assert_eq!(
            devices
                .iter()
                .find(|d| d.name == "Wheel-2")
                .unwrap()
                .description,
            "Pro Scan filter wheel 2"
        );
        assert_eq!(
            devices
                .iter()
                .find(|d| d.name == "Wheel-3")
                .unwrap()
                .description,
            "Pro Scan filter wheel 3"
        );
    }
}
