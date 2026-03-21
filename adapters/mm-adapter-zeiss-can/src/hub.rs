/// Zeiss CAN-bus shared serial hub.
///
/// All Zeiss CAN devices share one serial port at 9600 baud, `\r` terminator.
///
/// Command routing by prefix:
///   HP* → microscope stand (reflectors, objectives, filters, shutters, Z)
///   NP* → MCU28 XY stage controller
///
/// Response prefixes:
///   PH  → stand response (HP commands)
///   PN  → MCU28 response (NP commands)
use mm_device::error::{MmError, MmResult};
use mm_device::transport::Transport;

/// Shared hub that owns the transport and provides send/receive for all sub-devices.
pub struct ZeissHub {
    transport: Option<Box<dyn Transport>>,
}

impl ZeissHub {
    pub fn new() -> Self {
        Self { transport: None }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    pub fn is_connected(&self) -> bool { self.transport.is_some() }

    /// Send a command (appends `\r`) and return the trimmed response.
    pub fn send(&mut self, command: &str) -> MmResult<String> {
        let c = format!("{}\r", command);
        match self.transport.as_mut() {
            Some(t) => Ok(t.send_recv(&c)?.trim().to_string()),
            None => Err(MmError::NotConnected),
        }
    }
}

impl Default for ZeissHub { fn default() -> Self { Self::new() } }

/// Encode a signed position as 24-bit two's complement uppercase hex (6 chars).
pub fn encode_pos(steps: i32) -> String {
    let raw = if steps >= 0 { steps as u32 } else { (steps as i64 + 0x100_0000) as u32 };
    format!("{:06X}", raw & 0xFF_FFFF)
}

/// Decode a 24-bit two's complement hex string to a signed i32.
pub fn decode_pos(hex: &str) -> MmResult<i32> {
    let raw = u32::from_str_radix(hex.trim(), 16)
        .map_err(|_| MmError::LocallyDefined(format!("Zeiss hex parse error: '{}'", hex)))?;
    Ok(if raw & 0x80_0000 != 0 { (raw as i64 - 0x100_0000) as i32 } else { raw as i32 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_positive() { assert_eq!(encode_pos(100), "000064"); }
    #[test]
    fn encode_zero() { assert_eq!(encode_pos(0), "000000"); }
    #[test]
    fn encode_negative() { assert_eq!(encode_pos(-1), "FFFFFF"); }
    #[test]
    fn roundtrip() {
        for v in [-100_000i32, -1, 0, 1, 100_000] {
            assert_eq!(decode_pos(&encode_pos(v)).unwrap(), v);
        }
    }
}
