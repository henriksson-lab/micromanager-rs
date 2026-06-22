/// CrestOptics X-Light V3 spinning disk confocal adapter.
///
/// Serial protocol (ASCII, `\r`-terminated, echo-based):
///
/// Each command uses a prefix letter(s). The device echoes the command back
/// with the result value appended. The V3 protocol uses a consistent scheme:
///
///   Get position:   `r<PREFIX>\r`      → echoes `r<PREFIX><value>`
///   Get num pos:    `r<PREFIX>N\r`     → echoes `r<PREFIX>N<value>`
///   Set position:   `<PREFIX><value>\r`→ echoes `<PREFIX><value>`
///
/// For filter wheels (Emission=B, Dichroic=C, Excitation=A): positions are
/// 1-based on the wire (device position = MM position + 1).
/// For mechanical devices (Spinning slider=D, Camera slider=P, Motor=N):
/// positions are 0-based on the wire.
/// For Iris devices (Emission iris=V, Illumination iris=J): integer aperture.
///
/// Hub identification: `v\r` → `Crest driver Ver <x.y>`
///
/// Devices implemented (all StateDevice):
///   XLightV3EmissionWheel   (prefix B, 1-based)
///   XLightV3DichroicWheel   (prefix C, 1-based)
///   XLightV3ExcitationWheel (prefix A, 1-based)
///   XLightV3SpinningSlider  (prefix D, 0-based)
///   XLightV3CameraSlider    (prefix P, 0-based)
///   XLightV3SpinningMotor   (prefix N, 0-based, 2 positions)
pub mod state_device;

pub use state_device::{
    XLightV3CameraSlider, XLightV3DichroicWheel, XLightV3EmissionWheel, XLightV3ExcitationWheel,
    XLightV3SpinningMotor, XLightV3SpinningSlider,
};
