#[cfg(feature = "jai")]
pub mod camera;
/// JAI camera adapter (Pleora eBUS SDK).
///
/// Wraps the Pleora eBUS SDK C++ API behind the MicroManager `Camera` trait
/// via a thin C shim (`src/shim.cpp`) that exposes a plain `extern "C"` API.
///
/// # Setup
///
/// 1. Install the [Pleora eBUS SDK](https://www.pleora.com/support-center/ebus-sdk/)
///    for your platform (macOS / Linux / Windows).
/// 2. Build with: `cargo build -p mm-adapter-jai --features jai`
///
/// The build script locates the SDK via the `EBUS_SDK_ROOT` environment
/// variable (set it to the SDK root containing `Includes/` and `Libraries/`)
/// or scans `/opt/pleora/ebus_sdk/` (macOS / Linux default install).
///
/// # Properties
///
/// | Property | R/W | Description |
/// |---|---|---|
/// | `CameraIndex` | R/W (pre-init) | 0-based index of camera to open |
/// | `SerialNumber` | R/W (pre-init) | Serial number; empty = use CameraIndex |
/// | `Exposure` | R/W | Exposure time in **milliseconds** |
/// | `Gain` | R/W | Analog gain (camera-native float) |
/// | `PixelFormat` | R/W | GenICam pixel format string |
/// | `Binning` | R/W | Symmetric binning factor |
/// | `Width` | R | Active image width in pixels |
/// | `Height` | R | Active image height in pixels |
/// | `Temperature` | R | Device temperature in °C (if supported) |
/// | `Model` | R | Camera model name |
///
/// # Snap vs. sequence
///
/// `snap_image()` starts a single-frame grab (AcquisitionMode = SingleFrame),
/// waits for the result, then stops.  `start_sequence_acquisition()` switches
/// to continuous mode; subsequent `snap_image()` calls dequeue the next frame
/// without restarting acquisition.

#[cfg(feature = "jai")]
pub mod ffi;
#[cfg(feature = "jai")]
pub use camera::JAICamera;
