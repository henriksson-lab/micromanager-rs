/// Basler camera adapter (Pylon SDK).
///
/// Wraps the Basler Pylon C++ SDK via the [`pylon-cxx`](https://crates.io/crates/pylon-cxx)
/// crate behind the MicroManager `Camera` trait.
///
/// # Setup
///
/// 1. Install the [Basler Pylon SDK](https://www.baslerweb.com/en/downloads/software-downloads/)
///    for your platform (Windows / macOS / Linux).
/// 2. Build with: `cargo build -p mm-adapter-basler --features basler`
///
/// The `pylon-cxx` build script locates the SDK via the `PYLON_ROOT` environment
/// variable (set by the Pylon installer) or common install paths.
///
/// # Properties
///
/// | Property | R/W | Description |
/// |---|---|---|
/// | `SerialNumber` | R/W (pre-init) | Camera serial number; empty = first found |
/// | `Exposure` | R/W | Exposure time in **milliseconds** (converts to µs internally) |
/// | `Gain` | R/W | Analog gain (camera-native float units) |
/// | `PixelType` | R/W | GenICam pixel format string, e.g. `"Mono8"` |
/// | `Binning` | R/W | Symmetric horizontal+vertical binning factor |
/// | `Width` | R | Active image width in pixels |
/// | `Height` | R | Active image height in pixels |
/// | `Temperature` | R | Device temperature string (if supported) |
///
/// # Snap vs. sequence
///
/// `snap_image()` starts a one-shot grab (count = 1), waits for the result, and
/// stops.  `start_sequence_acquisition()` starts a continuous grab; subsequent
/// `snap_image()` calls dequeue the next available frame without restarting.

#[cfg(feature = "basler")]
pub mod camera;
#[cfg(feature = "basler")]
pub use camera::BaslerCamera;
