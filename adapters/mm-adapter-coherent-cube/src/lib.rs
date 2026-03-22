//! Coherent Cube laser controller adapter.
//!
//! Protocol (text, `\r` terminated commands, `\r\n` terminated responses):
//!
//! | Command     | Response      | Meaning                            |
//! |-------------|---------------|------------------------------------|
//! | `?X`        | `X=<value>`   | Query token X                      |
//! | `X=<value>` | `X=<value>`   | Set token X (returns achieved value)|
//!
//! Important tokens:
//! - `L`    — laser on/off (0 or 1)
//! - `SP`   — power setpoint (mW, float)
//! - `P`    — power readback (mW, float)
//! - `CW`   — CW mode (0=pulsed, 1=CW)
//! - `CDRH` — CDRH safety delay (0=off for tests)
//! - `T`    — TEC servo (1=on)
//! - `EXT`  — external power control (0=off)
//! - `HID`  — head serial number (read-only)
//! - `HH`   — head usage hours (read-only)
//! - `WAVE` — wavelength nm (read-only)
//! - `MINLP`— minimum laser power (read-only)
//! - `MAXLP`— maximum laser power (read-only)

pub mod coherent_cube;

pub use coherent_cube::CoherentCube;

use micromanager::traits::{AdapterModule, AnyDevice, DeviceInfo};
use micromanager::types::DeviceType;

pub const DEVICE_NAME_CUBE: &str = "CoherentCube";

static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: DEVICE_NAME_CUBE,
    description: "Coherent Cube laser controller",
    device_type: DeviceType::Shutter,
}];

pub struct CoherentCubeAdapter;

impl AdapterModule for CoherentCubeAdapter {
    fn module_name(&self) -> &'static str {
        "coherent-cube"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_CUBE => Some(AnyDevice::Shutter(Box::new(CoherentCube::new()))),
            _ => None,
        }
    }
}
