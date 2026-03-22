//! Oxxius LaserBoxx (LBX/LCX/LMX) laser controller adapter.
//!
//! Protocol (text, `\n` line ending, response terminated by `\r\n`):
//!
//! | Command        | Response         | Meaning                              |
//! |----------------|------------------|--------------------------------------|
//! | `inf?\n`       | model string     | Get model info e.g. "LBX-473-100-CSB"|
//! | `hid?\n`       | serial number    | Get hardware ID                      |
//! | `?sv\n`        | version string   | Get software version                 |
//! | `?sta\n`       | integer 1-7      | Get status (3=emission on)           |
//! | `dl 1\n`       | (no response)    | Turn emission on                     |
//! | `dl 0\n`       | (no response)    | Turn emission off                    |
//! | `p <mW>\n`     | (no response)    | Set power setpoint (mW)              |
//! | `?p\n`         | float            | Get power readback (mW)              |
//! | `?hh\n`        | float            | Get usage hours                      |
//! | `?f\n`         | integer          | Get fault code (0=none)              |
//! | `?int\n`       | 0 or 1           | Get interlock (1=closed/safe)        |

pub mod laserboxx;

pub use laserboxx::LaserBoxx;

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

pub const DEVICE_NAME_LASERBOXX: &str = "Oxxius LaserBoxx LBX or LMX or LCX";

static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: DEVICE_NAME_LASERBOXX,
    description: "Oxxius LaserBoxx laser source",
    device_type: DeviceType::Shutter,
}];

pub struct OxxiusLaserBoxxAdapter;

impl AdapterModule for OxxiusLaserBoxxAdapter {
    fn module_name(&self) -> &'static str {
        "oxxius-laserboxx"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_LASERBOXX => Some(AnyDevice::Shutter(Box::new(LaserBoxx::new()))),
            _ => None,
        }
    }
}
