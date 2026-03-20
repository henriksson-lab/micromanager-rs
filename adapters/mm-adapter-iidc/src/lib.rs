/// IIDC (IEEE 1394 FireWire) camera adapter.
///
/// Wraps libdc1394 behind the MicroManager `Camera` trait.
///
/// # Setup
/// - macOS: no driver needed; install libdc1394 via `brew install libdc1394`
/// - Linux: `apt install libdc1394-dev`
///
/// Build with: `cargo build -p mm-adapter-iidc --features iidc`
///
/// # Properties
///
/// | Property | R/W | Description |
/// |---|---|---|
/// | `CameraIndex` | R/W (pre-init) | Index of the IIDC camera to open (0-based) |
/// | `VideoMode` | R/W | Video mode string, e.g. `"640x480_MONO8"` |
/// | `Exposure` | R/W | Shutter feature raw value (camera-specific units) |
/// | `Gain` | R/W | Gain feature raw value |
/// | `Width` | R | Image width in pixels (set by active VideoMode) |
/// | `Height` | R | Image height in pixels (set by active VideoMode) |
/// | `PixelFormat` | R | Pixel format string derived from VideoMode |
///
/// # Snap vs. sequence
///
/// `snap_image()` sets up DMA, captures one frame, and tears down.
/// `start_sequence_acquisition()` leaves DMA running; subsequent `snap_image()`
/// calls dequeue the next available frame without stopping capture.

#[cfg(feature = "iidc")]
pub mod ffi;
#[cfg(feature = "iidc")]
pub mod camera;
#[cfg(feature = "iidc")]
pub use camera::IIDCCamera;
