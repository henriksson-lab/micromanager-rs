//! OpenFlexure (Sangaboard) device adapter.
//!
//! ASCII protocol (newline-terminated commands, \r terminated responses):
//! - version:         send "version"       → "Sangaboard v0.5.x"
//! - position query:  send "p"             → "<x> <y> <z>"
//! - relative XY:     send "mrx <steps>"   then "mry <steps>"
//! - relative Z:      send "mrz <steps>"
//! - zero:            send "zero"
//! - stop:            send "stop"
//! - release motors:  send "release"
//! - non-blocking:    send "blocking_moves false" → "done"
//! - busy?:           send "moving?"       → "true" or "false"
//! - step delay:      send "dt <us>"       (set), "dt?" (query → "minimum step delay <n>")
//! - ramp time:       send "ramp_time <us>", "ramp_time?" → "ramp_time <n>"
//!
//! Devices exported:
//! - `SangaBoardHub` — Hub
//! - `OFXYStage`     — XYStage
//! - `OFZStage`      — Stage
//! - `OFShutter`     — Shutter (LED illumination)

pub mod hub;
pub mod xystage;
pub mod zstage;
pub mod shutter;

pub use hub::SangaBoardHub;
pub use xystage::OfXYStage;
pub use zstage::OfZStage;
pub use shutter::OfShutter;

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

pub const DEVICE_NAME_HUB: &str = "SangaBoardHub";
pub const DEVICE_NAME_XYSTAGE: &str = "OFXYStage";
pub const DEVICE_NAME_ZSTAGE: &str = "OFZStage";
pub const DEVICE_NAME_SHUTTER: &str = "OFShutter";

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo { name: DEVICE_NAME_HUB,     description: "Sangaboard Hub",           device_type: DeviceType::Hub },
    DeviceInfo { name: DEVICE_NAME_XYSTAGE,  description: "OpenFlexure XY stage",     device_type: DeviceType::XYStage },
    DeviceInfo { name: DEVICE_NAME_ZSTAGE,   description: "OpenFlexure Z stage",      device_type: DeviceType::Stage },
    DeviceInfo { name: DEVICE_NAME_SHUTTER,  description: "OpenFlexure LED shutter",  device_type: DeviceType::Shutter },
];

pub struct OpenFlexureAdapter;

impl AdapterModule for OpenFlexureAdapter {
    fn module_name(&self) -> &'static str { "openflexure" }
    fn devices(&self) -> &'static [DeviceInfo] { DEVICE_LIST }
    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_HUB     => Some(AnyDevice::Hub(Box::new(SangaBoardHub::new()))),
            DEVICE_NAME_XYSTAGE => Some(AnyDevice::XYStage(Box::new(OfXYStage::new()))),
            DEVICE_NAME_ZSTAGE  => Some(AnyDevice::Stage(Box::new(OfZStage::new()))),
            DEVICE_NAME_SHUTTER => Some(AnyDevice::Shutter(Box::new(OfShutter::new()))),
            _ => None,
        }
    }
}
