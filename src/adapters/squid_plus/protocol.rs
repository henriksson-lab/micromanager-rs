/// Squid+ microcontroller binary protocol.
///
/// Commands are 8-byte packets; responses are 24-byte packets.
/// Both carry a CRC-8 (CCITT) in the final byte.
///
/// Packet layout (command, 8 bytes):
///   [0] command ID  (auto-incremented, for ACK matching)
///   [1] command code
///   [2..6] payload (command-specific)
///   [7] CRC-8 of bytes 0..6

pub const CMD_LENGTH: usize = 8;
pub const MSG_LENGTH: usize = 24;

// Command codes — motion
pub const CMD_MOVE_X: u8 = 0x00;
pub const CMD_MOVE_Y: u8 = 0x01;
pub const CMD_MOVE_Z: u8 = 0x02;
pub const CMD_MOVE_W: u8 = 0x04;
pub const CMD_HOME_OR_ZERO: u8 = 0x05;
pub const CMD_MOVETO_X: u8 = 0x06;
pub const CMD_MOVETO_Y: u8 = 0x07;
pub const CMD_MOVETO_Z: u8 = 0x08;

// Command codes — illumination
pub const CMD_TURN_ON_ILLUMINATION: u8 = 0x0A;
pub const CMD_TURN_OFF_ILLUMINATION: u8 = 0x0B;
pub const CMD_SET_ILLUMINATION: u8 = 0x0C;

// Axis identifiers
pub const AXIS_X: u8 = 0;
pub const AXIS_Y: u8 = 1;
pub const AXIS_Z: u8 = 2;
pub const AXIS_W: u8 = 5;
pub const AXIS_XY: u8 = 4;

// Home direction / zero flags
pub const HOME_POSITIVE: u8 = 0;
pub const HOME_NEGATIVE: u8 = 1;
pub const ZERO: u8 = 2;

// Execution status codes (response byte 1)
pub const STATUS_COMPLETED: u8 = 0x00;
pub const STATUS_IN_PROGRESS: u8 = 0x01;
pub const STATUS_CHECKSUM_ERROR: u8 = 0x02;

/// CRC-8 CCITT lookup table.
const CRC8_TABLE: [u8; 256] = {
    let mut table = [0u8; 256];
    let poly: u8 = 0x07;
    let mut i = 0u16;
    while i < 256 {
        let mut crc = i as u8;
        let mut bit = 0;
        while bit < 8 {
            if crc & 0x80 != 0 {
                crc = (crc << 1) ^ poly;
            } else {
                crc <<= 1;
            }
            bit += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
};

/// Compute CRC-8 CCITT over the given bytes.
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &b in data {
        crc = CRC8_TABLE[(crc ^ b) as usize];
    }
    crc
}

/// Build a MOVE command for any axis.
///
/// `cmd_code`: one of CMD_MOVE_X/Y/Z/W.
/// `usteps`: signed microstep count (positive = forward).
pub fn build_move(cmd_id: u8, cmd_code: u8, usteps: i32) -> [u8; CMD_LENGTH] {
    let mut pkt = [0u8; CMD_LENGTH];
    pkt[0] = cmd_id;
    pkt[1] = cmd_code;
    let bytes = usteps.to_be_bytes();
    pkt[2] = bytes[0];
    pkt[3] = bytes[1];
    pkt[4] = bytes[2];
    pkt[5] = bytes[3];
    pkt[7] = crc8(&pkt[..7]);
    pkt
}

/// Build a MOVE_W command packet (convenience wrapper).
pub fn build_move_w(cmd_id: u8, usteps: i32) -> [u8; CMD_LENGTH] {
    build_move(cmd_id, CMD_MOVE_W, usteps)
}

/// Build a HOME_OR_ZERO command for an axis.
///
/// `mode`: HOME_NEGATIVE (1), HOME_POSITIVE (0), or ZERO (2).
pub fn build_home(cmd_id: u8, axis: u8, mode: u8) -> [u8; CMD_LENGTH] {
    let mut pkt = [0u8; CMD_LENGTH];
    pkt[0] = cmd_id;
    pkt[1] = CMD_HOME_OR_ZERO;
    pkt[2] = axis;
    pkt[3] = mode;
    pkt[7] = crc8(&pkt[..7]);
    pkt
}

/// Build a HOME_OR_ZERO command for the XY pair.
pub fn build_home_xy(cmd_id: u8, mode_x: u8, mode_y: u8) -> [u8; CMD_LENGTH] {
    let mut pkt = [0u8; CMD_LENGTH];
    pkt[0] = cmd_id;
    pkt[1] = CMD_HOME_OR_ZERO;
    pkt[2] = AXIS_XY;
    pkt[3] = mode_x;
    pkt[4] = mode_y;
    pkt[7] = crc8(&pkt[..7]);
    pkt
}

/// Build a HOME_OR_ZERO command for the W axis (convenience wrapper).
pub fn build_home_w(cmd_id: u8) -> [u8; CMD_LENGTH] {
    build_home(cmd_id, AXIS_W, HOME_NEGATIVE)
}

/// Build TURN_ON_ILLUMINATION command.
pub fn build_turn_on_illumination(cmd_id: u8) -> [u8; CMD_LENGTH] {
    let mut pkt = [0u8; CMD_LENGTH];
    pkt[0] = cmd_id;
    pkt[1] = CMD_TURN_ON_ILLUMINATION;
    pkt[7] = crc8(&pkt[..7]);
    pkt
}

/// Build TURN_OFF_ILLUMINATION command.
pub fn build_turn_off_illumination(cmd_id: u8) -> [u8; CMD_LENGTH] {
    let mut pkt = [0u8; CMD_LENGTH];
    pkt[0] = cmd_id;
    pkt[1] = CMD_TURN_OFF_ILLUMINATION;
    pkt[7] = crc8(&pkt[..7]);
    pkt
}

/// Build SET_ILLUMINATION command.
///
/// `source`: illumination source index (e.g. 11 = 405nm, 12 = 488nm).
/// `intensity_u16`: 16-bit intensity value (0–65535).
pub fn build_set_illumination(cmd_id: u8, source: u8, intensity_u16: u16) -> [u8; CMD_LENGTH] {
    let mut pkt = [0u8; CMD_LENGTH];
    pkt[0] = cmd_id;
    pkt[1] = CMD_SET_ILLUMINATION;
    pkt[2] = source;
    pkt[3] = (intensity_u16 >> 8) as u8;
    pkt[4] = (intensity_u16 & 0xFF) as u8;
    pkt[7] = crc8(&pkt[..7]);
    pkt
}

/// Parse a 24-byte response.  Returns `(cmd_id, status)`.
pub fn parse_response(buf: &[u8]) -> Option<(u8, u8)> {
    if buf.len() < MSG_LENGTH {
        return None;
    }
    let expected_crc = crc8(&buf[..MSG_LENGTH - 1]);
    if buf[MSG_LENGTH - 1] != expected_crc {
        return None;
    }
    Some((buf[0], buf[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc8_empty() {
        assert_eq!(crc8(&[]), 0);
    }

    #[test]
    fn move_w_packet_structure() {
        let pkt = build_move_w(42, 12800);
        assert_eq!(pkt[0], 42); // cmd_id
        assert_eq!(pkt[1], CMD_MOVE_W);
        // 12800 = 0x00003200 big-endian
        assert_eq!(pkt[2], 0x00);
        assert_eq!(pkt[3], 0x00);
        assert_eq!(pkt[4], 0x32);
        assert_eq!(pkt[5], 0x00);
        assert_eq!(pkt[7], crc8(&pkt[..7]));
    }

    #[test]
    fn move_w_negative() {
        let pkt = build_move_w(0, -12800);
        let bytes = (-12800i32).to_be_bytes();
        assert_eq!(pkt[2..6], bytes);
    }

    #[test]
    fn home_w_packet() {
        let pkt = build_home_w(7);
        assert_eq!(pkt[0], 7);
        assert_eq!(pkt[1], CMD_HOME_OR_ZERO);
        assert_eq!(pkt[2], AXIS_W);
        assert_eq!(pkt[3], HOME_NEGATIVE);
        assert_eq!(pkt[7], crc8(&pkt[..7]));
    }

    #[test]
    fn parse_valid_response() {
        let mut buf = [0u8; MSG_LENGTH];
        buf[0] = 5; // cmd_id
        buf[1] = STATUS_COMPLETED;
        buf[MSG_LENGTH - 1] = crc8(&buf[..MSG_LENGTH - 1]);
        let (id, status) = parse_response(&buf).unwrap();
        assert_eq!(id, 5);
        assert_eq!(status, STATUS_COMPLETED);
    }

    #[test]
    fn parse_bad_crc() {
        let mut buf = [0u8; MSG_LENGTH];
        buf[0] = 5;
        buf[MSG_LENGTH - 1] = 0xFF; // wrong CRC
        assert!(parse_response(&buf).is_none());
    }

    #[test]
    fn move_z_packet() {
        let pkt = build_move(1, CMD_MOVE_Z, 1000);
        assert_eq!(pkt[1], CMD_MOVE_Z);
        let bytes = 1000i32.to_be_bytes();
        assert_eq!(pkt[2..6], bytes);
    }

    #[test]
    fn home_z_packet() {
        let pkt = build_home(2, AXIS_Z, HOME_NEGATIVE);
        assert_eq!(pkt[1], CMD_HOME_OR_ZERO);
        assert_eq!(pkt[2], AXIS_Z);
        assert_eq!(pkt[3], HOME_NEGATIVE);
    }

    #[test]
    fn illumination_on_off() {
        let on = build_turn_on_illumination(3);
        assert_eq!(on[1], CMD_TURN_ON_ILLUMINATION);
        let off = build_turn_off_illumination(4);
        assert_eq!(off[1], CMD_TURN_OFF_ILLUMINATION);
    }

    #[test]
    fn set_illumination_packet() {
        let pkt = build_set_illumination(5, 11, 32768);
        assert_eq!(pkt[1], CMD_SET_ILLUMINATION);
        assert_eq!(pkt[2], 11); // source: 405nm
        assert_eq!(pkt[3], 0x80); // 32768 >> 8
        assert_eq!(pkt[4], 0x00); // 32768 & 0xFF
    }

    #[test]
    fn zero_axis_packet() {
        let pkt = build_home(0, AXIS_X, ZERO);
        assert_eq!(pkt[2], AXIS_X);
        assert_eq!(pkt[3], ZERO);
    }
}
