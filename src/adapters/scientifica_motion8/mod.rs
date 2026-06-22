pub mod xy_stage;
pub mod z_stage;
pub use xy_stage::Motion8XYStage;
pub use z_stage::Motion8ZStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct ScientificaMotion8Adapter;

const DEVICES: &[DeviceInfo] = &[
    DeviceInfo {
        name: "Scientifica-Motion8-XY_Device_1",
        description: "XY Stage (Device 1)",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: "Scientifica-Motion8-Z_Device_1",
        description: "Z Stage (Device 1)",
        device_type: DeviceType::Stage,
    },
    DeviceInfo {
        name: "Scientifica-Moiton8-XY_Device_2",
        description: "XY Stage (Device 2)",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: "Scientifica-Moition8-Z_Device_2",
        description: "Z Stage (Device 2)",
        device_type: DeviceType::Stage,
    },
];

impl AdapterModule for ScientificaMotion8Adapter {
    fn module_name(&self) -> &'static str {
        "ScientificaMotion8"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "Scientifica-Motion8-XY_Device_1" => {
                Some(AnyDevice::XYStage(Box::new(Motion8XYStage::new(0))))
            }
            "Scientifica-Motion8-Z_Device_1" => {
                Some(AnyDevice::Stage(Box::new(Motion8ZStage::new(0))))
            }
            "Scientifica-Moiton8-XY_Device_2" => {
                Some(AnyDevice::XYStage(Box::new(Motion8XYStage::new(1))))
            }
            "Scientifica-Moition8-Z_Device_2" => {
                Some(AnyDevice::Stage(Box::new(Motion8ZStage::new(1))))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_upstream_motion8_device_names() {
        let adapter = ScientificaMotion8Adapter;
        let names: Vec<&str> = adapter.devices().iter().map(|d| d.name).collect();
        assert_eq!(
            names,
            vec![
                "Scientifica-Motion8-XY_Device_1",
                "Scientifica-Motion8-Z_Device_1",
                "Scientifica-Moiton8-XY_Device_2",
                "Scientifica-Moition8-Z_Device_2",
            ]
        );

        assert!(adapter
            .create_device("Scientifica-Motion8-XY_Device_1")
            .is_some());
        assert!(adapter
            .create_device("Scientifica-Motion8-Z_Device_1")
            .is_some());
        assert!(adapter
            .create_device("Scientifica-Moiton8-XY_Device_2")
            .is_some());
        assert!(adapter
            .create_device("Scientifica-Moition8-Z_Device_2")
            .is_some());
        assert!(adapter
            .create_device("ScientificaMotion8-XYStage")
            .is_none());
        assert!(adapter.create_device("ScientificaMotion8-ZStage").is_none());
    }
}
