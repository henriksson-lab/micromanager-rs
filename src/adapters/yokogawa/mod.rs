pub mod dichroic;
/// Yokogawa CSU-X1 spinning disk confocal adapter.
///
/// NOTE: The Yokogawa CSU-W1 is already implemented as mm-adapter-csuw1.
/// This adapter covers the CSU-X1 / CSUX family (older Yokogawa disks) which
/// use a different ASCII protocol than the W1.
///
/// Serial protocol (ASCII, `\r`-terminated):
///   Commands are sent with `\r` and acknowledged with `A` (OK) or `N` (error).
///   For queries, a value line is returned first, then `A`.
///
/// Key commands:
///   Filter wheel:
///     `FW_POS, <wheel>, <pos>\r`  → `A`  set filter wheel position (1-based)
///     `FW_POS, <wheel>, ?\r`      → `<pos>\r` then `A`  query position
///     `FW_SPEED, <wheel>, <spd>\r`→ `A`  set filter wheel speed (0-3)
///     `FW_SPEED, <wheel>, ?\r`    → `<spd>\r` then `A`  query speed
///   Dichroic mirror:
///     `DM_POS, <pos>\r`           → `A`  set dichroic position (1-based)
///     `DM_POS, ?\r`               → `<pos>\r` then `A`  query position
///   Shutter:
///     `SHO\r`                     → `A`  open shutter
///     `SHC\r`                     → `A`  close shutter
///     `SH, ?\r`                   → `OPEN\rA` or `CLOSED\rA`
///
/// Devices:
///   CsuXFilterWheel (StateDevice) — filter wheel (two wheels available, wheel 1 or 2)
///   CsuXDichroic    (StateDevice) — dichroic mirror
///   CsuXShutter     (Shutter)     — main shutter
pub mod filter_wheel;
pub mod shutter;

pub use dichroic::CsuXDichroic;
pub use filter_wheel::CsuXFilterWheel;
pub use shutter::CsuXShutter;
