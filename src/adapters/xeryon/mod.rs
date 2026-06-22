pub mod turret;
pub mod xy_stage;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub use turret::SquidPlusObjectiveTurret;
pub use xy_stage::XeryonXYStage;

pub const DEVICE_NAME_XYSTAGE: &str = "XeryonXYStage";
pub const DEVICE_NAME_TURRET: &str = "SquidPlusObjectiveTurret";

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo {
        name: DEVICE_NAME_XYSTAGE,
        description: "Xeryon piezo XY stage",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: DEVICE_NAME_TURRET,
        description: "Squid+ objective turret (Xeryon XLS-1250-3N)",
        device_type: DeviceType::State,
    },
];

pub struct XeryonAdapter;

impl AdapterModule for XeryonAdapter {
    fn module_name(&self) -> &'static str {
        "Xeryon"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_XYSTAGE => Some(AnyDevice::XYStage(Box::new(XeryonXYStage::new()))),
            DEVICE_NAME_TURRET => Some(AnyDevice::StateDevice(Box::new(
                SquidPlusObjectiveTurret::new(),
            ))),
            _ => None,
        }
    }
}
