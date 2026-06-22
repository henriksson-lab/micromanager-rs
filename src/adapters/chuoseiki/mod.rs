pub mod xy_stage;
pub mod z_stage;
pub use xy_stage::ChuoSeikiXYStage;
pub use z_stage::ChuoSeikiZStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct ChuoSeikiAdapter;

const DEVICES: &[DeviceInfo] = &[
    DeviceInfo {
        name: "ChuoSeiki_MD 2-Axis",
        description: "XY Stage",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: "ChuoSeiki_MD 1-Axis",
        description: "Z Stage",
        device_type: DeviceType::Stage,
    },
];

impl AdapterModule for ChuoSeikiAdapter {
    fn module_name(&self) -> &'static str {
        "ChuoSeiki"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "ChuoSeiki_MD 2-Axis" => Some(AnyDevice::XYStage(Box::new(ChuoSeikiXYStage::new()))),
            "ChuoSeiki_MD 1-Axis" => Some(AnyDevice::Stage(Box::new(ChuoSeikiZStage::new()))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_registers_both_md5000_devices() {
        let adapter = ChuoSeikiAdapter;
        assert_eq!(adapter.module_name(), "ChuoSeiki");
        assert_eq!(adapter.devices().len(), 2);
        assert!(matches!(
            adapter.create_device("ChuoSeiki_MD 2-Axis"),
            Some(AnyDevice::XYStage(_))
        ));
        assert!(matches!(
            adapter.create_device("ChuoSeiki_MD 1-Axis"),
            Some(AnyDevice::Stage(_))
        ));
    }
}
