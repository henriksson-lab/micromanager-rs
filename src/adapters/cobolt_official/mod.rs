//! Cobolt Official laser controller adapter.
//!
//! Official Cobolt adapter for all Cobolt laser series (06-01, 06-MLD, DPL, Skyra).
//! Protocol (text, `\r` terminator, response terminated by `\r\n`):
//!
//! | Command           | Response      | Meaning                             |
//! |-------------------|---------------|-------------------------------------|
//! | `gsn?\r`          | serial number | Get serial number                   |
//! | `l1r\r`           | `OK`          | Open emission shutter               |
//! | `l0r\r`           | `OK`          | Close emission shutter              |
//! | `l?\r`            | `0` or `1`    | Get laser on/off state              |
//! | `pa?\r`           | float         | Get actual output power (mW)        |
//! | `slp <mW>\r`      | `OK`          | Set laser power setpoint            |
//! | `glp?\r`          | float         | Get laser power setpoint (mW)       |
//! | `gfv?\r`          | string        | Get firmware version                |
//! | `hrs?\r`          | float         | Get usage hours                     |

pub mod cobolt_official;

pub use cobolt_official::CoboltOfficialLaser;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub const DEVICE_NAME_COBOLT_OFFICIAL: &str = "Cobolt Laser";

static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: DEVICE_NAME_COBOLT_OFFICIAL,
    description: "Official device adapter for Cobolt lasers.",
    device_type: DeviceType::Shutter,
}];

pub struct CoboltOfficialAdapter;

impl AdapterModule for CoboltOfficialAdapter {
    fn module_name(&self) -> &'static str {
        "cobolt-official"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_COBOLT_OFFICIAL => {
                Some(AnyDevice::Shutter(Box::new(CoboltOfficialLaser::new())))
            }
            _ => None,
        }
    }
}
