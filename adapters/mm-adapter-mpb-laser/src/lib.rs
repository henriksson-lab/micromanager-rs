//! MPB Communications Inc. laser controller adapter.
//!
//! Protocol (text, `\r` terminator):
//!
//! | Command                    | Response      | Meaning                          |
//! |----------------------------|---------------|----------------------------------|
//! | `getldenable\r`            | `0` or `1`    | Get laser diode on/off           |
//! | `setldenable 0\r`          | `0`           | Turn laser diode off             |
//! | `setldenable 1\r`          | `1`           | Turn laser diode on              |
//! | `getpowerenable\r`         | `0` or `1`    | Get mode (0=ACC, 1=APC)          |
//! | `powerenable 0\r`          | `0`           | Set ACC mode                     |
//! | `powerenable 1\r`          | `1`           | Set APC mode                     |
//! | `getpower 0\r`             | float         | Get power setpoint               |
//! | `setpower 0 <f>\r`         | float         | Set power setpoint               |
//! | `getlaserstate\r`          | integer       | Get laser state                  |
//! | `getpowersetptlim 0\r`     | `<min> <max>` | Get power setpoint limits        |
//! | `getacccurmax\r`           | integer       | Get max ACC current (mA)         |
//! | `getldlim 1\r`             | `<min> <max>` | Get LD current limits            |
//! | `getinput 2\r`             | `0` or `1`    | Get key lock state               |

pub mod mpb_laser;

pub use mpb_laser::MpbLaser;

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

pub const DEVICE_NAME_MPB: &str = "MPBLaser";

static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: DEVICE_NAME_MPB,
    description: "Lasers from MPB Communications Inc.",
    device_type: DeviceType::Shutter,
}];

pub struct MpbLaserAdapter;

impl AdapterModule for MpbLaserAdapter {
    fn module_name(&self) -> &'static str {
        "mpb-laser"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_MPB => Some(AnyDevice::Shutter(Box::new(MpbLaser::new()))),
            _ => None,
        }
    }
}
