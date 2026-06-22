pub mod dac;
pub mod hub;
pub mod ttl;

pub use dac::TriggerScopeDAC;
pub use hub::TriggerScopeHub;
pub use ttl::TriggerScopeTTL;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub const DEVICE_NAME_HUB: &str = "TriggerScope-Hub";
pub const DEVICE_NAME_TTL_MASTER: &str = "TriggerScope-TTL-Master";
pub const DEVICE_NAME_CAM1: &str = "TriggerScope-CAM1";
pub const DEVICE_NAME_CAM2: &str = "TriggerScope-CAM2";
pub const DEVICE_NAME_FOCUS: &str = "TriggerScope-Focus";

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo {
        name: DEVICE_NAME_HUB,
        description: "ARC TriggerScope hub",
        device_type: DeviceType::Hub,
    },
    DeviceInfo {
        name: "TriggerScope-DAC01",
        description: "TriggerScope DAC channel 1",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-DAC02",
        description: "TriggerScope DAC channel 2",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-DAC03",
        description: "TriggerScope DAC channel 3",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-DAC04",
        description: "TriggerScope DAC channel 4",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-DAC05",
        description: "TriggerScope DAC channel 5",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-DAC06",
        description: "TriggerScope DAC channel 6",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-DAC07",
        description: "TriggerScope DAC channel 7",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-DAC08",
        description: "TriggerScope DAC channel 8",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-DAC09",
        description: "TriggerScope DAC channel 9",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-DAC10",
        description: "TriggerScope DAC channel 10",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-DAC11",
        description: "TriggerScope DAC channel 11",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-DAC12",
        description: "TriggerScope DAC channel 12",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-DAC13",
        description: "TriggerScope DAC channel 13",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-DAC14",
        description: "TriggerScope DAC channel 14",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-DAC15",
        description: "TriggerScope DAC channel 15",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-DAC16",
        description: "TriggerScope DAC channel 16",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TriggerScope-TTL01",
        description: "TriggerScope TTL channel 1",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TriggerScope-TTL02",
        description: "TriggerScope TTL channel 2",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TriggerScope-TTL03",
        description: "TriggerScope TTL channel 3",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TriggerScope-TTL04",
        description: "TriggerScope TTL channel 4",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TriggerScope-TTL05",
        description: "TriggerScope TTL channel 5",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TriggerScope-TTL06",
        description: "TriggerScope TTL channel 6",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TriggerScope-TTL07",
        description: "TriggerScope TTL channel 7",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TriggerScope-TTL08",
        description: "TriggerScope TTL channel 8",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TriggerScope-TTL09",
        description: "TriggerScope TTL channel 9",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TriggerScope-TTL10",
        description: "TriggerScope TTL channel 10",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TriggerScope-TTL11",
        description: "TriggerScope TTL channel 11",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TriggerScope-TTL12",
        description: "TriggerScope TTL channel 12",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TriggerScope-TTL13",
        description: "TriggerScope TTL channel 13",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TriggerScope-TTL14",
        description: "TriggerScope TTL channel 14",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TriggerScope-TTL15",
        description: "TriggerScope TTL channel 15",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TriggerScope-TTL16",
        description: "TriggerScope TTL channel 16",
        device_type: DeviceType::State,
    },
];

pub struct TriggerScopeAdapter;

impl AdapterModule for TriggerScopeAdapter {
    fn module_name(&self) -> &'static str {
        "triggerscope"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        if name == DEVICE_NAME_HUB {
            return Some(AnyDevice::Hub(Box::new(TriggerScopeHub::new())));
        }
        if name == DEVICE_NAME_TTL_MASTER {
            return Some(AnyDevice::StateDevice(Box::new(TriggerScopeTTL::new(0))));
        }
        if let Some(channel) = parse_numbered_name(name, "TriggerScope-DAC") {
            return Some(AnyDevice::SignalIO(Box::new(TriggerScopeDAC::new(channel))));
        }
        if let Some(channel) = parse_numbered_name(name, "TriggerScope-TTL") {
            return Some(AnyDevice::StateDevice(Box::new(TriggerScopeTTL::new(
                channel,
            ))));
        }
        None
    }
}

fn parse_numbered_name(name: &str, prefix: &str) -> Option<u8> {
    let channel = name.strip_prefix(prefix)?.parse::<u8>().ok()?;
    (1..=16).contains(&channel).then_some(channel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_registers_hub_and_supported_children() {
        let adapter = TriggerScopeAdapter;
        let names: Vec<&str> = adapter.devices().iter().map(|d| d.name).collect();

        assert!(names.contains(&DEVICE_NAME_HUB));
        assert!(!names.contains(&DEVICE_NAME_TTL_MASTER));
        assert!(names.contains(&"TriggerScope-DAC16"));
        assert!(names.contains(&"TriggerScope-TTL16"));
        assert!(adapter.create_device(DEVICE_NAME_CAM1).is_none());
        assert!(adapter.create_device(DEVICE_NAME_FOCUS).is_none());
        assert!(matches!(
            adapter.create_device("TriggerScope-DAC01"),
            Some(AnyDevice::SignalIO(_))
        ));
        assert!(matches!(
            adapter.create_device("TriggerScope-TTL-Master"),
            Some(AnyDevice::StateDevice(_))
        ));
    }
}
