mod common;
/// CrestOptics X-Light spinning disk confocal adapter.
///
/// Serial protocol (ASCII, terminated with `\r`, echo-based):
///   Each command is echoed back by the device after completion.
///   Commands:
///     `rC\r`   → `rCN\r`   query dichroic position (N = 1-based digit)
///     `CN\r`   → `CN\r`    set dichroic to position N (1-based, N=1..5)
///     `rB\r`   → `rBN\r`   query emission wheel position
///     `BN\r`   → `BN\r`    set emission wheel to position N (1-based, N=1..8)
///     `rE\r`   → `rEN\r`   query excitation wheel position (or `rA` on newer firmware)
///     `EN\r`   → `EN\r`    set excitation wheel (or `AN` on newer firmware)
///     `rD\r`   → `rDN\r`   query disk slider position (0-based, 0=out)
///     `DN\r`   → `DN\r`    set disk slider position N (N=0..2)
///     `rN\r`   → `rNN\r`   query spin motor state (0=off, 1=on)
///     `NN\r`   → `NN\r`    set spin motor state (N=0 or 1)
///     `rM\r`   → `rMN\r`   query touchscreen state
///     `MN\r`   → `MN\r`    set touchscreen state (0=active, 1=locked)
///
/// Devices implemented:
///   - XLightDichroic  (StateDevice, 5 positions, 1-based on wire)
///   - XLightEmission  (StateDevice, 8 positions, 1-based on wire)
///   - XLightExcitation (StateDevice, 8 positions, 1-based on wire)
///   - XLightDiskSlider (StateDevice, 3 positions, 0-based on wire)
///   - XLightSpinMotor  (StateDevice, 2 positions, 0-based on wire)
///   - XLightTouchScreen (StateDevice, 2 positions, 0-based on wire)
pub mod dichroic;
pub mod disk_slider;
pub mod emission;
pub mod excitation;
pub mod spin_motor;
pub mod touchscreen;

pub use dichroic::XLightDichroic;
pub use disk_slider::XLightDiskSlider;
pub use emission::XLightEmission;
pub use excitation::XLightExcitation;
pub use spin_motor::XLightSpinMotor;
pub use touchscreen::XLightTouchScreen;
