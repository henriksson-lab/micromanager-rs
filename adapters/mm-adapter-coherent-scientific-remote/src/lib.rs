//! Coherent Scientific Remote laser controller adapter.
//!
//! Controls Coherent OBIS lasers via the Coherent Scientific Remote (CSR) controller.
//! Supports up to 6 lasers. Uses SCPI-like protocol with `?` suffix for queries.
//!
//! Protocol (text, `\n` terminator):
//!
//! | Command                             | Response       | Meaning                        |
//! |-------------------------------------|----------------|--------------------------------|
//! | `*IDN?\n`                           | id string      | Identify controller            |
//! | `SYST1:INF:MOD?\n`                  | model string   | Get laser 1 model              |
//! | `SOUR1:AM:STATE On\n`               | echo           | Turn laser 1 on                |
//! | `SOUR1:AM:STATE Off\n`              | echo           | Turn laser 1 off               |
//! | `SOUR1:AM:STATE?\n`                 | On/Off         | Get laser 1 state              |
//! | `SOUR1:POW:LEV:IMM:AMPL <W>\n`     | echo           | Set laser 1 power (Watts)      |
//! | `SOUR1:POW:LEV:IMM:AMPL?\n`        | float          | Get laser 1 power setpoint (W) |
//! | `SOUR1:POW:LIM:HIGH?\n`            | float          | Get max power (W)              |
//! | `SOUR1:POW:LIM:LOW?\n`             | float          | Get min power (W)              |
//! | `SYST1:INF:WAV?\n`                 | float          | Get wavelength (nm)            |
//! | `SYST1:DIOD:HOUR?\n`               | float          | Get usage hours                |
//! | `SYST1:INF:SNUM?\n`                | string         | Get head serial number         |

pub mod csr;

pub use csr::CoherentScientificRemote;

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

pub const DEVICE_NAME_CSR: &str = "Coherent-Scientific Remote";

static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: DEVICE_NAME_CSR,
    description: "CoherentScientificRemote Laser",
    device_type: DeviceType::Shutter,
}];

pub struct CoherentScientificRemoteAdapter;

impl AdapterModule for CoherentScientificRemoteAdapter {
    fn module_name(&self) -> &'static str {
        "coherent-scientific-remote"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_CSR => {
                Some(AnyDevice::Shutter(Box::new(CoherentScientificRemote::new())))
            }
            _ => None,
        }
    }
}
