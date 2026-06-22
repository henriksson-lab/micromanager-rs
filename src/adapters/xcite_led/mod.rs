pub mod xcite_led;
pub use xcite_led::{XCiteLedController, XCiteLedShutter};

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;
use xcite_led::{
    DEVICE_NAME_CONTROLLER, DEVICE_NAME_LED1, DEVICE_NAME_LED2, DEVICE_NAME_LED3, DEVICE_NAME_LED4,
};

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo {
        name: DEVICE_NAME_CONTROLLER,
        description: DEVICE_NAME_CONTROLLER,
        device_type: DeviceType::Generic,
    },
    DeviceInfo {
        name: DEVICE_NAME_LED1,
        description: DEVICE_NAME_LED1,
        device_type: DeviceType::Shutter,
    },
    DeviceInfo {
        name: DEVICE_NAME_LED2,
        description: DEVICE_NAME_LED2,
        device_type: DeviceType::Shutter,
    },
    DeviceInfo {
        name: DEVICE_NAME_LED3,
        description: DEVICE_NAME_LED3,
        device_type: DeviceType::Shutter,
    },
    DeviceInfo {
        name: DEVICE_NAME_LED4,
        description: DEVICE_NAME_LED4,
        device_type: DeviceType::Shutter,
    },
];

pub struct XCiteLedAdapter;

impl AdapterModule for XCiteLedAdapter {
    fn module_name(&self) -> &'static str {
        "XCiteLed"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_CONTROLLER => Some(AnyDevice::Generic(Box::new(XCiteLedController::new()))),
            DEVICE_NAME_LED1 => Some(AnyDevice::Shutter(Box::new(XCiteLedShutter::new(0)))),
            DEVICE_NAME_LED2 => Some(AnyDevice::Shutter(Box::new(XCiteLedShutter::new(1)))),
            DEVICE_NAME_LED3 => Some(AnyDevice::Shutter(Box::new(XCiteLedShutter::new(2)))),
            DEVICE_NAME_LED4 => Some(AnyDevice::Shutter(Box::new(XCiteLedShutter::new(3)))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_exports_upstream_device_list() {
        let adapter = XCiteLedAdapter;
        let names: Vec<_> = adapter.devices().iter().map(|d| d.name).collect();
        assert_eq!(
            names,
            vec![
                DEVICE_NAME_CONTROLLER,
                DEVICE_NAME_LED1,
                DEVICE_NAME_LED2,
                DEVICE_NAME_LED3,
                DEVICE_NAME_LED4
            ]
        );
    }

    #[test]
    fn factory_creates_named_led_channels() {
        let adapter = XCiteLedAdapter;
        let led3 = adapter.create_device(DEVICE_NAME_LED3).unwrap();
        assert_eq!(led3.as_device().name(), DEVICE_NAME_LED3);
        assert!(led3.as_shutter().is_some());

        let controller = adapter.create_device(DEVICE_NAME_CONTROLLER).unwrap();
        assert_eq!(controller.as_device().name(), DEVICE_NAME_CONTROLLER);
    }
}
