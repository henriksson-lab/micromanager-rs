pub mod dac;
pub mod hub;
pub mod ttl;

pub use dac::TriggerScopeMMDAC;
pub use hub::TriggerScopeMMHub;
pub use ttl::TriggerScopeMMTTL;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub const DEVICE_NAME_HUB: &str = "TriggerScopeMM-Hub";
pub const DEVICE_NAME_TTL1_8: &str = "TS_TTL1-8";
pub const DEVICE_NAME_TTL9_16: &str = "TS_TTL9-16";

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo {
        name: DEVICE_NAME_HUB,
        description: "ARC TriggerScope MM hub",
        device_type: DeviceType::Hub,
    },
    DeviceInfo {
        name: DEVICE_NAME_TTL1_8,
        description: "TriggerScope MM TTL channels 1-8",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: DEVICE_NAME_TTL9_16,
        description: "TriggerScope MM TTL channels 9-16",
        device_type: DeviceType::State,
    },
    DeviceInfo {
        name: "TS_DAC01",
        description: "TriggerScope MM DAC channel 1",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TS_DAC02",
        description: "TriggerScope MM DAC channel 2",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TS_DAC03",
        description: "TriggerScope MM DAC channel 3",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TS_DAC04",
        description: "TriggerScope MM DAC channel 4",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TS_DAC05",
        description: "TriggerScope MM DAC channel 5",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TS_DAC06",
        description: "TriggerScope MM DAC channel 6",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TS_DAC07",
        description: "TriggerScope MM DAC channel 7",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TS_DAC08",
        description: "TriggerScope MM DAC channel 8",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TS_DAC09",
        description: "TriggerScope MM DAC channel 9",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TS_DAC10",
        description: "TriggerScope MM DAC channel 10",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TS_DAC11",
        description: "TriggerScope MM DAC channel 11",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TS_DAC12",
        description: "TriggerScope MM DAC channel 12",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TS_DAC13",
        description: "TriggerScope MM DAC channel 13",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TS_DAC14",
        description: "TriggerScope MM DAC channel 14",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TS_DAC15",
        description: "TriggerScope MM DAC channel 15",
        device_type: DeviceType::SignalIO,
    },
    DeviceInfo {
        name: "TS_DAC16",
        description: "TriggerScope MM DAC channel 16",
        device_type: DeviceType::SignalIO,
    },
];

pub struct TriggerScopeMMAdapter;

impl AdapterModule for TriggerScopeMMAdapter {
    fn module_name(&self) -> &'static str {
        "triggerscope_mm"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_HUB => Some(AnyDevice::Hub(Box::new(TriggerScopeMMHub::new()))),
            DEVICE_NAME_TTL1_8 => Some(AnyDevice::StateDevice(Box::new(TriggerScopeMMTTL::new(0)))),
            DEVICE_NAME_TTL9_16 => {
                Some(AnyDevice::StateDevice(Box::new(TriggerScopeMMTTL::new(1))))
            }
            _ => parse_numbered_name(name, "TS_DAC").map(|channel| {
                AnyDevice::SignalIO(Box::new(TriggerScopeMMDAC::new(channel)) as Box<_>)
            }),
        }
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
    fn adapter_registers_mm_hub_and_children() {
        let adapter = TriggerScopeMMAdapter;
        let names: Vec<&str> = adapter.devices().iter().map(|d| d.name).collect();

        assert!(names.contains(&DEVICE_NAME_HUB));
        assert!(names.contains(&DEVICE_NAME_TTL1_8));
        assert!(names.contains(&DEVICE_NAME_TTL9_16));
        assert!(names.contains(&"TS_DAC16"));
        assert!(matches!(
            adapter.create_device(DEVICE_NAME_TTL9_16),
            Some(AnyDevice::StateDevice(_))
        ));
        assert!(matches!(
            adapter.create_device("TS_DAC01"),
            Some(AnyDevice::SignalIO(_))
        ));
        assert!(adapter.create_device("TS_DAC17").is_none());
    }
}
