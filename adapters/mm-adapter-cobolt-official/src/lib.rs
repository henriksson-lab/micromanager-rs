//! Cobolt Official laser controller adapter.
//!
//! Official Cobolt adapter for all Cobolt laser series (06-01, 06-MLD, DPL, Skyra).
//! Protocol (text, `\r` terminator, response terminated by `\r\n`):
//!
//! | Command           | Response      | Meaning                             |
//! |-------------------|---------------|-------------------------------------|
//! | `sn?\r`           | serial number | Get serial number                   |
//! | `l1\r`            | `OK`          | Turn laser on                       |
//! | `l0\r`            | `OK`          | Turn laser off                      |
//! | `l?\r`            | `0` or `1`    | Get laser on/off state              |
//! | `p?\r`            | float         | Get actual output power (mW)        |
//! | `slp <mW>\r`      | `OK`          | Set laser power setpoint            |
//! | `glp?\r`          | float         | Get laser power setpoint (mW)       |
//! | `ver?\r`          | string        | Get firmware version                |
//! | `hrs?\r`          | float         | Get usage hours                     |

pub mod cobolt_official;

pub use cobolt_official::CoboltOfficialLaser;

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

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
