pub mod xy_stage;
pub mod z_stage;
pub use xy_stage::ChuoSeikiQTXYStage;
pub use z_stage::ChuoSeikiQTZStage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub struct ChuoSeikiQtAdapter;

const DEVICES: &[DeviceInfo] = &[
    DeviceInfo {
        name: "ChuoSeiki_QT 2-Axis",
        description: "ChuoSeiki 2-stage driver adapter",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: "ChuoSeiki_QT 1-Axis",
        description: "ChuoSeiki 1-stage driver",
        device_type: DeviceType::Stage,
    },
];

impl AdapterModule for ChuoSeikiQtAdapter {
    fn module_name(&self) -> &'static str {
        "ChuoSeiki_QT"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICES
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            "ChuoSeiki_QT 2-Axis" => Some(AnyDevice::XYStage(Box::new(ChuoSeikiQTXYStage::new()))),
            "ChuoSeiki_QT 1-Axis" => Some(AnyDevice::Stage(Box::new(ChuoSeikiQTZStage::new()))),
            _ => None,
        }
    }
}
