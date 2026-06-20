pub mod laser_quantum;
pub use laser_quantum::LaserQuantumLaser;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub const DEVICE_NAME_LASER_QUANTUM: &str = "Laser";

static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: DEVICE_NAME_LASER_QUANTUM,
    description: "LaserQuantum laser",
    device_type: DeviceType::Generic,
}];

pub struct LaserQuantumAdapter;

impl AdapterModule for LaserQuantumAdapter {
    fn module_name(&self) -> &'static str {
        "laser_quantum"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_LASER_QUANTUM => {
                Some(AnyDevice::Generic(Box::new(LaserQuantumLaser::new())))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_upstream_laser_device() {
        let adapter = LaserQuantumAdapter;
        assert_eq!(adapter.module_name(), "laser_quantum");
        assert_eq!(adapter.devices()[0].name, "Laser");

        let dev = adapter
            .create_device("Laser")
            .expect("LaserQuantum Laser device should be registered");
        assert_eq!(dev.as_device().name(), "Laser");
        assert_eq!(dev.as_device().device_type(), DeviceType::Generic);
        assert!(adapter.create_device("LaserQuantumLaser").is_none());
    }
}
