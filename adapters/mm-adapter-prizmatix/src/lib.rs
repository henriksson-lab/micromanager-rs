//! Prizmatix LED controller adapter.
//!
//! Controls Prizmatix UHP/FC-LED/Combi-LED devices.
//! The device uses a binary-ish protocol where commands are text strings
//! sent to the USB/serial port.
//!
//! Key commands (text with `\n` terminator, response terminated by `\r\n`):
//!
//! | Command         | Response          | Meaning                         |
//! |-----------------|-------------------|---------------------------------|
//! | `V:0\n`         | `V:0_<nLEDs>`     | Get version / number of LEDs    |
//! | `V:1\n`         | `V:1_<firmware>`  | Get firmware type code          |
//! | `S:0\n`         | LED names string  | Get LED channel names           |
//! | `P:<ch>,<val>\n`| OK                | Set channel power (0-100)       |
//! | `O:<ch>,<0/1>\n`| OK                | Set channel on/off              |

pub mod prizmatix;

pub use prizmatix::PrizmatixController;

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

pub const DEVICE_NAME_PRIZMATIX: &str = "Prizmatix Ctrl";

static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: DEVICE_NAME_PRIZMATIX,
    description: "Prizmatix LED Controller",
    device_type: DeviceType::Shutter,
}];

pub struct PrizmatixAdapter;

impl AdapterModule for PrizmatixAdapter {
    fn module_name(&self) -> &'static str {
        "prizmatix"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_PRIZMATIX => {
                Some(AnyDevice::Shutter(Box::new(PrizmatixController::new())))
            }
            _ => None,
        }
    }
}
