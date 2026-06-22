pub mod conex;
pub mod smc;
pub use conex::NewportConex;
pub use smc::NewportSmc;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct NewportStageAdapter;

const DEVICES: &[DeviceInfo] = &[
    DeviceInfo {
        name: "XAxis",
        description: "Conex_Axis X Axis",
        device_type: DeviceType::Stage,
    },
    DeviceInfo {
        name: "YAxis",
        description: "Conex_Axis Y Axis",
        device_type: DeviceType::Stage,
    },
    DeviceInfo {
        name: "ZAxis",
        description: "Conex_Axis Z Axis",
        device_type: DeviceType::Stage,
    },
    DeviceInfo {
        name: "NewportConex",
        description: "Newport CONEX-CC single-axis controller",
        device_type: DeviceType::Stage,
    },
    DeviceInfo {
        name: "NewportZStage",
        description: "Newport SMC100CC controller adapter",
        device_type: DeviceType::Stage,
    },
];

impl AdapterModule for NewportStageAdapter {
    fn module_name(&self) -> &'static str {
        "NewportStage"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "XAxis" => Some(AnyDevice::Stage(Box::new(NewportConex::with_identity(
                "XAxis",
                "Conex XAxis driver",
            )))),
            "YAxis" => Some(AnyDevice::Stage(Box::new(NewportConex::with_identity(
                "YAxis",
                "Conex YAxis driver",
            )))),
            "ZAxis" => Some(AnyDevice::Stage(Box::new(NewportConex::with_identity(
                "ZAxis",
                "Conex ZAxis driver",
            )))),
            "NewportConex" => Some(AnyDevice::Stage(Box::new(NewportConex::new()))),
            "NewportZStage" => Some(AnyDevice::Stage(Box::new(NewportSmc::new()))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_upstream_conex_axis_aliases() {
        let adapter = NewportStageAdapter;
        let devices = adapter.devices();
        assert!(devices
            .iter()
            .any(|d| d.name == "XAxis" && d.description == "Conex_Axis X Axis"));
        assert!(devices
            .iter()
            .any(|d| d.name == "YAxis" && d.description == "Conex_Axis Y Axis"));
        assert!(devices
            .iter()
            .any(|d| d.name == "ZAxis" && d.description == "Conex_Axis Z Axis"));

        match adapter.create_device("YAxis").unwrap() {
            AnyDevice::Stage(dev) => {
                assert_eq!(dev.name(), "YAxis");
                assert_eq!(dev.description(), "Conex YAxis driver");
            }
            _ => panic!("expected CONEX axis stage"),
        }
    }
}
