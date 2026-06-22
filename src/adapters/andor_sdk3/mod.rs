#[cfg(feature = "andor-sdk3")]
mod ffi;

#[cfg(feature = "andor-sdk3")]
pub mod camera;

#[cfg(feature = "andor-sdk3")]
pub use camera::Andor3Camera;

#[cfg(feature = "andor-sdk3")]
use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
#[cfg(feature = "andor-sdk3")]
use crate::types::DeviceType;

#[cfg(feature = "andor-sdk3")]
pub const DEVICE_NAME: &str = "Andor sCMOS Camera";

#[cfg(feature = "andor-sdk3")]
static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: DEVICE_NAME,
    description: "SDK3 Device Adapter for sCMOS cameras",
    device_type: DeviceType::Camera,
}];

#[cfg(feature = "andor-sdk3")]
pub struct AndorSdk3Adapter;

#[cfg(feature = "andor-sdk3")]
impl AdapterModule for AndorSdk3Adapter {
    fn module_name(&self) -> &'static str {
        "AndorSDK3"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME => Some(AnyDevice::Camera(Box::new(Andor3Camera::new()))),
            _ => None,
        }
    }
}

#[cfg(all(test, feature = "andor-sdk3"))]
mod tests {
    use super::*;

    #[test]
    fn adapter_registers_upstream_camera_name() {
        let adapter = AndorSdk3Adapter;
        assert_eq!(adapter.module_name(), "AndorSDK3");
        assert_eq!(adapter.devices().len(), 1);
        assert_eq!(adapter.devices()[0].name, "Andor sCMOS Camera");
        assert_eq!(
            adapter.devices()[0].description,
            "SDK3 Device Adapter for sCMOS cameras"
        );
        assert!(adapter.create_device("Andor sCMOS Camera").is_some());
        assert!(adapter.create_device("Andor3Camera").is_none());
    }
}
