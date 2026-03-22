//! Cobolt / HÜBNER Photonics laser controller adapter.
//!
//! Protocol (text, \r line ending, response terminated by \r\n):
//!
//! | Command        | Response      | Meaning                      |
//! |----------------|---------------|------------------------------|
//! | `l?`           | `0` or `1`    | Query laser on/off state     |
//! | `l1`           | `OK`          | Turn laser on                |
//! | `l0`           | `OK`          | Turn laser off               |
//! | `p?`           | `<mW>`        | Query actual output power    |
//! | `slp <mW>`     | `OK`          | Set laser power setpoint     |
//! | `glp?`         | `<mW>`        | Get laser power setpoint     |
//! | `sn?`          | `<number>`    | Query serial number          |
//! | `hrs?`         | `<hours>`     | Query head usage hours       |
//! | `ver?`         | `<version>`   | Query firmware version       |
//! | `ky?`          | `0` or `1`    | Query key status             |
//! | `f?`           | `<code>`      | Query fault code             |
//! | `ilk?`         | `0` or `1`    | Query interlock status       |

pub mod cobolt;

pub use cobolt::CoboltLaser;

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

pub const DEVICE_NAME_COBOLT: &str = "Cobolt";

static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: DEVICE_NAME_COBOLT,
    description: "Cobolt laser controller (HÜBNER Photonics)",
    device_type: DeviceType::Shutter,
}];

pub struct CoboltAdapter;

impl AdapterModule for CoboltAdapter {
    fn module_name(&self) -> &'static str {
        "cobolt"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_COBOLT => Some(AnyDevice::Shutter(Box::new(CoboltLaser::new()))),
            _ => None,
        }
    }
}
