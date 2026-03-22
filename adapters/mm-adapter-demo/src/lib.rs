pub mod camera;
pub mod stage;
pub mod xy_stage;
pub mod shutter;
pub mod state_device;

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

pub use camera::DemoCamera;
pub use stage::DemoStage;
pub use xy_stage::DemoXYStage;
pub use shutter::DemoShutter;
pub use state_device::DemoStateDevice;

// ─── Device name constants ────────────────────────────────────────────────────

pub const DEVICE_NAME_CAMERA: &str = "DCamera";
pub const DEVICE_NAME_STAGE: &str = "DStage";
pub const DEVICE_NAME_XYSTAGE: &str = "DXYStage";
pub const DEVICE_NAME_SHUTTER: &str = "DShutter";
pub const DEVICE_NAME_STATE: &str = "DWheel";

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo {
        name: DEVICE_NAME_CAMERA,
        description: "Demo camera — simulates a digital camera",
        device_type: DeviceType::Camera,
    },
    DeviceInfo {
        name: DEVICE_NAME_STAGE,
        description: "Demo Z stage",
        device_type: DeviceType::Stage,
    },
    DeviceInfo {
        name: DEVICE_NAME_XYSTAGE,
        description: "Demo XY stage",
        device_type: DeviceType::XYStage,
    },
    DeviceInfo {
        name: DEVICE_NAME_SHUTTER,
        description: "Demo shutter",
        device_type: DeviceType::Shutter,
    },
    DeviceInfo {
        name: DEVICE_NAME_STATE,
        description: "Demo filter wheel",
        device_type: DeviceType::State,
    },
];

// ─── Adapter module ───────────────────────────────────────────────────────────

pub struct DemoAdapter;

impl AdapterModule for DemoAdapter {
    fn module_name(&self) -> &'static str {
        "demo"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_CAMERA => Some(AnyDevice::Camera(Box::new(DemoCamera::new()))),
            DEVICE_NAME_STAGE => Some(AnyDevice::Stage(Box::new(DemoStage::new()))),
            DEVICE_NAME_XYSTAGE => Some(AnyDevice::XYStage(Box::new(DemoXYStage::new()))),
            DEVICE_NAME_SHUTTER => Some(AnyDevice::Shutter(Box::new(DemoShutter::new()))),
            DEVICE_NAME_STATE => Some(AnyDevice::StateDevice(Box::new(DemoStateDevice::new()))),
            _ => None,
        }
    }
}
