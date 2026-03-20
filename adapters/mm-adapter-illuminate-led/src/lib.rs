/// Illuminate LED Array adapter.
///
/// Serial JSON protocol (`\n`-terminated commands, `-==-\n` response terminator).
/// Requires firmware interface version 2.30–10.0.
///
/// After `initialize()` the adapter sends `machine\n` to switch to JSON mode,
/// then `pprops\n` to read device metadata (LED count, channel count, etc.).
///
/// # Illumination patterns (Pattern property)
///
/// | Value | Command |
/// |---|---|
/// | `Brightfield` | `bf` |
/// | `Darkfield` | `df` |
/// | `DPC` | `dpc.<angle>` (0/90/180/270 or custom) |
/// | `ColorDPC` | `cdpc` |
/// | `ColorDarkfield` | `cdf` |
/// | `Annulus` | `an.<start_na100>.<width_na100>` |
/// | `HalfAnnulus` | `ha.<dir>.<start_na100>.<width_na100>` |
/// | `Center` | `l.0` |
/// | `Manual` | `l.<i1>.<i2>...` (LED indices) |
///
/// `set_open(true)` fires the current pattern; `set_open(false)` clears all LEDs (`x`).

pub mod led_array;
pub use led_array::IlluminateLedArray;
