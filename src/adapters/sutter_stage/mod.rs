pub mod xy_stage;
pub mod z_stage;
pub use xy_stage::SutterXYStage;
pub use z_stage::SutterZStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct SutterStageAdapter;

const DEVICES: &[DeviceInfo] = &[DeviceInfo {
    name: "XYStage",
    description: "SutterStage XY stage driver adapter",
    device_type: DeviceType::XYStage,
}];

impl AdapterModule for SutterStageAdapter {
    fn module_name(&self) -> &'static str {
        "SutterStage"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "XYStage" => Some(AnyDevice::XYStage(Box::new(SutterXYStage::new()))),
            "Stage" => Some(AnyDevice::Stage(Box::new(SutterZStage::new('Z')))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_factory_alias_is_not_advertised() {
        let adapter = SutterStageAdapter;
        let names: Vec<&str> = adapter.devices().iter().map(|d| d.name).collect();
        assert_eq!(names, vec!["XYStage"]);
        assert!(matches!(
            adapter.create_device("Stage"),
            Some(AnyDevice::Stage(_))
        ));
    }
}
