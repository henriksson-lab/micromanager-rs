//! Toptica iBeam Smart CW laser controller adapter.
//!
//! The iBeam Smart uses a hidden CW mode accessed via a service-level password.
//! Protocol uses `\r` line terminator and `[OK]` as end-of-response token.
//!
//! Key commands (text, `\r` terminator):
//!
//! | Command              | Response            | Meaning                      |
//! |----------------------|---------------------|------------------------------|
//! | `prom off\r`         | `[OK]`              | Disable prompt               |
//! | `id\r`               | serial + `[OK]`     | Get serial number            |
//! | `ver\r`              | version + `[OK]`    | Get firmware version         |
//! | `sta la\r`           | ON/OFF + `[OK]`     | Get laser on/off state       |
//! | `la on\r`            | `[OK]`              | Turn laser on                |
//! | `la off\r`           | `[OK]`              | Turn laser off               |
//! | `set pow <mW>\r`     | `[OK]`              | Set power setpoint           |
//! | `sh level pow\r`     | power data + `[OK]` | Read actual power            |
//! | `sta clip\r`         | PASS/FAIL + `[OK]`  | Read clip status             |

pub mod ibeam;

pub use ibeam::IBeamSmartCW;

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

pub const DEVICE_NAME_IBEAM: &str = "iBeamSmartCW";

static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: DEVICE_NAME_IBEAM,
    description: "Toptica iBeam smart laser in CW mode",
    device_type: DeviceType::Shutter,
}];

pub struct TopticaIBeamAdapter;

impl AdapterModule for TopticaIBeamAdapter {
    fn module_name(&self) -> &'static str {
        "toptica-ibeam"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_IBEAM => Some(AnyDevice::Shutter(Box::new(IBeamSmartCW::new()))),
            _ => None,
        }
    }
}
