/// Daheng Galaxy camera adapter.
///
/// Wraps the Daheng GxIAPI C library for direct access to Daheng industrial
/// cameras (MER, MER2, Mars, Venus series) via USB3 Vision or GigE Vision.
///
/// # Setup
///
/// 1. Install the [Daheng Galaxy SDK](https://www.dahengimaging.com/) for your platform
/// 2. Ensure `libgxiapi.so` (Linux) or `GxIAPI.dll` (Windows) is in the library path
///    or set `DAHENG_SDK_ROOT` / `GALAXY_ROOT` to the SDK root. Linux SDKs
///    usually place runtime libraries under `lib/<arch>`; put that directory in
///    `LD_LIBRARY_PATH` as well because `libgxiapi.so` depends on colocated
///    Galaxy libraries such as `liblog4cplus_gx.so`.
/// 3. Build with: `cargo build --features daheng`
///
/// # Properties
///
/// | Property | R/W | Description |
/// |---|---|---|
/// | `SerialNumber` | R/W (pre-init) | Camera serial number; empty = first found |
/// | `Exposure` | R/W | Exposure time in **milliseconds** (converts to µs internally) |
/// | `Gain` | R/W | Analog gain (camera-native float units) |
/// | `PixelFormat` | R/W | Pixel format: Mono8, Mono10, Mono12, Mono16, BayerRG8, etc. |
/// | `Binning` | R/W | Symmetric horizontal+vertical binning factor |
/// | `Width` | R | Active image width in pixels |
/// | `Height` | R | Active image height in pixels |

#[cfg(feature = "daheng")]
pub mod camera;
#[cfg(feature = "daheng")]
pub mod ffi;
#[cfg(feature = "daheng")]
pub use camera::DahengCamera;

#[cfg(feature = "daheng")]
use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
#[cfg(feature = "daheng")]
use crate::types::DeviceType;

#[cfg(feature = "daheng")]
pub const DEVICE_NAME: &str = "DahengCamera";

#[cfg(feature = "daheng")]
static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: DEVICE_NAME,
    description: "Daheng Camera",
    device_type: DeviceType::Camera,
}];

#[cfg(feature = "daheng")]
pub struct DahengAdapter;

#[cfg(feature = "daheng")]
impl AdapterModule for DahengAdapter {
    fn module_name(&self) -> &'static str {
        "DahengGalaxy"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME => Some(AnyDevice::Camera(Box::new(DahengCamera::new()))),
            _ => None,
        }
    }
}

#[cfg(all(test, feature = "daheng"))]
mod tests {
    use super::*;

    #[test]
    fn registered_device_matches_upstream_name_description_and_type() {
        let devices = DahengAdapter.devices();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "DahengCamera");
        assert_eq!(devices[0].description, "Daheng Camera");
        assert_eq!(devices[0].device_type, DeviceType::Camera);
    }
}
