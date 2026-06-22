/// Intelligent Imaging Innovations (3i) Diskovery spinning disk adapter.
///
/// Serial protocol (ASCII, `\r`-terminated):
///   Queries:  `Q:<PARAM>\r`      → `A:<PARAM>,<value>\r`
///   Commands: `A:<PARAM>,<val>\r`→ `A:<PARAM>,<val>\r`  (echo)
///
/// Selected commands used here:
///   `Q:VERSION_HW_MAJOR\r`    → version info (u16)
///   `Q:VERSION_FW_MAJOR\r`    → firmware info (u16)
///   `Q:PRESET_SD\r`           → spinning disk position (1-based preset)
///   `A:PRESET_SD,<n>\r`       → set spinning disk preset
///   `Q:PRESET_WF\r`           → wide-field position
///   `A:PRESET_WF,<n>\r`       → set wide-field preset
///   `Q:PRESET_FILTER_W\r`     → filter wheel W position
///   `A:PRESET_FILTER_W,<n>\r` → set filter wheel W
///   `Q:PRESET_FILTER_T\r`     → filter turret T position
///   `A:PRESET_FILTER_T,<n>\r` → set filter turret T
///   `Q:PRESET_IRIS\r`         → iris / objective selector
///   `A:PRESET_IRIS,<n>\r`     → set iris preset
///   `Q:MOTOR_RUNNING_SD\r`    → motor running state (0 or 1)
///   `A:MOTOR_RUNNING_SD,<n>\r`→ set motor running state
///
/// Devices (all StateDevice):
///   DiskoverySD       — spinning disk position (5 presets)
///   DiskoveryWF       — wide-field illumination size (4 presets)
///   DiskoveryFilterW  — filter wheel W (4 presets)
///   DiskoveryFilterT  — filter turret T (4 presets)
///   DiskoveryIris     — objective selector / iris (4 presets)
///   DiskoveryMotor    — spinning disk motor running (2 states)
pub mod state_device;

pub use state_device::{
    DiskoveryFilterT, DiskoveryFilterW, DiskoveryIris, DiskoveryMotor, DiskoverySD, DiskoveryWF,
};
