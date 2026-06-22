#[cfg(feature = "picam")]
pub mod camera;
/// Princeton Instruments camera adapter (PVCAM SDK).
///
/// Wraps the PVCAM C API behind the MicroManager `Camera` trait via a thin C
/// shim (`src/shim.c`) that exposes a simplified opaque API.
///
/// # Platform notes
///
/// | Platform | SDK headers | Library |
/// |---|---|---|
/// | macOS   | `<PICAM/pvcam.h>` | `/Library/Frameworks/PICAM.framework` |
/// | Linux   | `<pvcam/pvcam.h>` | `libpvcam.so` |
/// | Windows | `<picam.h>`       | `pvcam32.lib` |
///
/// # Setup
///
/// 1. Install PVCAM from Teledyne Vision Solutions' software and firmware
///    downloads page. Current Linux SDK downloads require a Teledyne account.
/// 2. Build with: `cargo build -p mm-adapter-picam --features picam`
///
/// Set `PVCAM_ROOT` to override the default SDK search path. Linux defaults
/// to `/opt/pvcam` and expects the installed SDK/runtime layout:
/// `$PVCAM_ROOT/sdk/include/master.h`, `$PVCAM_ROOT/sdk/include/pvcam.h`, and
/// `$PVCAM_ROOT/library/<arch>/libpvcam.so`. For an unpacked SDK/runtime pair,
/// set `PVCAM_LIB_DIR` directly to the directory containing `libpvcam.so`.
/// Use `PVCAM_STUB=1` only for local compile/test runs without the vendor SDK;
/// it does not exercise real PVCAM.
///
/// # Properties
///
/// | Property | R/W | Description |
/// |---|---|---|
/// | `CameraName`   | R/W (pre-init) | PVCAM camera name, e.g. `"pvcam0"`; empty = first found |
/// | `Exposure`     | R/W | Exposure time in **milliseconds** |
/// | `GainIndex`    | R/W | Gain index (1-based, range 1..GainMax) |
/// | `Binning`      | R/W | Symmetric binning factor |
/// | `Width`        | R   | Active image width in pixels |
/// | `Height`       | R   | Active image height in pixels |
/// | `BitDepth`     | R   | Bits per pixel for the current readout speed |
/// | `Temperature`  | R   | Sensor temperature in °C |
/// | `TempSetpoint` | R/W | Cooling setpoint in °C |
/// | `SerialNumber` | R   | Camera serial number |
/// | `ChipName`     | R   | Sensor chip name |
///
/// # Snap vs. sequence
///
/// `snap_image()` uses `pl_exp_setup_seq` + `pl_exp_start_seq` + polling.
/// `start_sequence_acquisition()` uses `pl_exp_setup_cont` + `pl_exp_start_cont`;
/// subsequent `snap_image()` calls dequeue frames via `pl_exp_get_oldest_frame`.

#[cfg(feature = "picam")]
pub mod ffi;
#[cfg(feature = "picam")]
pub use camera::PICAMCamera;
