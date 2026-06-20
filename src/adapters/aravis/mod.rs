/// GenICam camera adapter via the [Aravis](https://github.com/AravisProject/aravis) library.
///
/// Supports USB3 Vision and GigE Vision cameras from any vendor that implements
/// the GenICam standard (Daheng, FLIR, Basler, Allied Vision, etc.).
///
/// # Setup
///
/// 1. Install the Aravis library (≥ 0.8.14):
///    - Debian/Ubuntu: `apt install libaravis-0.8-dev`
///    - macOS: `brew install aravis`
///    - From source: <https://github.com/AravisProject/aravis>
/// 2. Build with: `cargo build --features aravis`
///
/// # Properties
///
/// | Property | R/W | Description |
/// |---|---|---|
/// | `DeviceId` | R/W (pre-init) | Aravis device ID string; empty = first found |
/// | `Exposure` | R/W | Exposure time in **milliseconds** (converts to µs internally) |
/// | `Gain` | R/W | Analog gain (camera-native float units) |
/// | `PixelType` | R/W | GenICam pixel format string, e.g. `"Mono8"` |
/// | `Binning` | R/W | Symmetric horizontal+vertical binning factor |
/// | `Width` | R | Active image width in pixels |
/// | `Height` | R | Active image height in pixels |

#[cfg(feature = "aravis")]
pub mod camera;
#[cfg(feature = "aravis")]
pub use camera::AravisCamera;

#[cfg(feature = "aravis")]
use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
#[cfg(feature = "aravis")]
use crate::types::DeviceType;

#[cfg(feature = "aravis")]
pub const DEVICE_NAME: &str = "AravisCamera";

#[cfg(feature = "aravis")]
static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: DEVICE_NAME,
    description: "GenICam camera (Aravis — USB3 Vision / GigE Vision)",
    device_type: DeviceType::Camera,
}];

#[cfg(feature = "aravis")]
pub struct AravisAdapter;

#[cfg(feature = "aravis")]
impl AdapterModule for AravisAdapter {
    fn module_name(&self) -> &'static str {
        "Aravis"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME => Some(AnyDevice::Camera(Box::new(AravisCamera::new()))),
            _ => None,
        }
    }
}
