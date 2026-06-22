/// ASI W-PTR (Wellplate Transfer Robot) adapter.
///
/// Serial protocol (ASCII, `\r\n`-terminated):
///   All commands are echoed back (3-character command word) on success.
///
/// Commands:
///   `ORG\r\n`             → `ORG`  home / origin
///   `GET <stage>,<slot>\r\n` → `GET`  retrieve plate from stage/slot
///   `PUT <stage>,<slot>\r\n` → `PUT`  place plate to stage/slot
///   `AES\r\n`             → `AES`  emergency stop
///   `DRT\r\n`             → `DRT`  drive reset (re-enable after AES)
///
/// Stage and slot are integer coordinates (stage 1..N, slot 1..N).
///
/// This is a GenericDevice (no specific MicroManager type beyond Device).
/// The user selects Stage and Slot as properties, then sets Command = ORG/GET/PUT/AES/DRT.
pub mod wptr;
pub use wptr::AsiWPTR;
