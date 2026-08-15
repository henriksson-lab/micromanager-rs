//! Bus-agnostic PVCAM wire protocol — encoders/decoders recovered by clean-room
//! reverse engineering of `libpvcam.so` (see
//! `extra_Photometrix/PVCAM_REVERSE_ENGINEERING.md`).
//!
//! This layer is pure Rust with no I/O: it builds the byte frames that a
//! [`super::transport::PvcamBus`] carries over PCIe or USB, and parses responses.
//! Everything here is unit-tested against the documented byte layouts.

/// Host-command message class byte (frame[0]). Recovered from
/// `HostCommandMessage::Finalize` (`mov $0x3f`).
pub const MSG_CLASS: u8 = 0x3F;
/// Per-entry frame markers appended by `AddControlCommand`.
pub const CTRL_BEGIN: u8 = 0x26;
pub const CTRL_END: u8 = 0x28;

/// `E_CAMERA_BRIDGE_COMMANDS` — the bridge-envelope `cmd` byte (subset; full list
/// in the RE notes §4.1).
#[allow(dead_code)]
pub mod bridge {
    pub const INIT_CCL: u8 = 0x0B;
    pub const ABORT_ACQUISITION: u8 = 0x0C;
    pub const BYTE_ORDER: u8 = 0x0D;
    pub const ACQ_PARMS: u8 = 0x19;
    pub const PREP_TRANSFER: u8 = 0x1A;
    pub const READOUT_TIMEOUT: u8 = 0x1B;
    pub const SET_PIXEL_BIT_DEPTH: u8 = 0x1D;
    pub const SET_TRANSFER_SPEED: u8 = 0x1E;
    pub const ENABLE_PIXEL_TRANSFER: u8 = 0x1F;
    pub const GET_XFER_BUFFER_SIZE: u8 = 0x20;
}

/// `E_SIMPLIFIED_CCL_COMMANDS` — high-level exposure description opcodes (§4.2).
/// Each is emitted as a 5-byte `[op][value:BE u32]` SCCL entry.
#[allow(dead_code)]
pub mod sccl {
    pub const EXPOSE_RESOLUTION: u8 = 0x00;
    pub const EXPOSURETIME: u8 = 0x01;
    pub const EXPOSUREMODE: u8 = 0x02;
    pub const FRAME_COUNT: u8 = 0x03;
    pub const MODE: u8 = 0x04;
    pub const SUB_ROI_COUNT: u8 = 0x05;
    pub const BIN_X: u8 = 0x06;
    pub const BIN_Y: u8 = 0x07;
    pub const IMGCOORD_TOPL_X: u8 = 0x08;
    pub const IMGCOORD_TOPL_Y: u8 = 0x09;
    pub const IMGCOORD_BOTR_X: u8 = 0x0A;
    pub const IMGCOORD_BOTR_Y: u8 = 0x0B;
    pub const SENSOR_CLEAR_MODE: u8 = 0x0C;
    pub const SENSOR_CLEAR_COUNT: u8 = 0x0D;
}

/// Host-command codes for parameters (subset; full 116-entry map in
/// `extra_Photometrix/pvcam_host_command_codes.md`). `R` = read code, `W` = write.
#[allow(dead_code)]
pub mod code {
    pub const TEMP_R: u8 = 0x07; // i16, centi-°C
    pub const TEMP_SETPOINT_R: u8 = 0x23;
    pub const TEMP_SETPOINT_W: u8 = 0x2D;
    pub const ADC_OFFSET_R: u8 = 0x3C;
    pub const ADC_OFFSET_W: u8 = 0x3D;
    pub const GAIN_INDEX_R: u8 = 0x1B;
    pub const GAIN_INDEX_W: u8 = 0x2C;
    pub const SPDTAB_INDEX_W: u8 = 0x2B;
    pub const READOUT_PORT_R: u8 = 0x57;
    pub const READOUT_PORT_W: u8 = 0x56;
    pub const PIX_TIME_R: u8 = 0x02;
    pub const READOUT_TIME_R: u8 = 0x78; // u32
    pub const EXPOSURE_TIME_R: u8 = 0xA8; // i64
    pub const SHTR_OPEN_DELAY_R: u8 = 0x1F;
    pub const SHTR_OPEN_DELAY_W: u8 = 0x30;
    pub const SHTR_CLOSE_DELAY_R: u8 = 0x20;
    pub const SHTR_CLOSE_DELAY_W: u8 = 0x31;
    pub const FAN_SPEED_R: u8 = 0x88;
    pub const FAN_SPEED_W: u8 = 0x89;
    pub const IMAGE_FORMAT_R: u8 = 0xBE;
    pub const IMAGE_FORMAT_W: u8 = 0xC2;
    pub const METADATA_ENABLED_R: u8 = 0x8A;
    pub const METADATA_ENABLED_W: u8 = 0x8B;
    pub const ROI_COUNT_MAX_R: u8 = 0x8D;
    pub const COLOR_MODE_R: u8 = 0xA3; // u8
    pub const SPDTAB_NAME_R: u8 = 0xE1; // 32B string
    pub const GAIN_NAME_R: u8 = 0x98; // 32B string
    pub const SCRIPT_STATUS_R: u8 = 0xE0;
    pub const SCRIPT_UPLOAD_W: u8 = 0x0A; // CCL script (INIT_CCL family)
    pub const START_SEQ_W: u8 = 0x39;
    pub const STOP_CCS_W: u8 = 0x33;
}

/// Payload endianness selector. The `DataFormat` argument observed in
/// `addPayload`/`copyBuffer` controls a byte-swap of the value bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataFormat {
    /// Bytes copied verbatim (little-endian on the host).
    LittleEndian,
    /// Bytes reversed (big-endian on the wire).
    BigEndian,
}

/// Build a host-command **read** frame: `[0x3F][LE16 len][0x26][code][resp×size][0x28]`.
///
/// The response area is reserved (`size` zero bytes) and overwritten in place by
/// the device (the request/response buffer is shared, RE §4bis). Returns the frame
/// plus the byte offset at which the response value lands.
pub fn encode_read(code: u8, size: u16) -> (Vec<u8>, usize) {
    let mut body = Vec::new();
    body.push(CTRL_BEGIN);
    body.push(code);
    let resp_off = HEADER_LEN + body.len();
    body.extend(std::iter::repeat(0u8).take(size as usize));
    body.push(CTRL_END);
    (finalize(body), resp_off)
}

/// Build a host-command **write** frame: `[0x3F][LE16 len][0x26][code][value][0x28]`.
/// `value` bytes are emitted in `fmt` order.
pub fn encode_write(code: u8, value: &[u8], fmt: DataFormat) -> Vec<u8> {
    let mut body = Vec::new();
    body.push(CTRL_BEGIN);
    body.push(code);
    match fmt {
        DataFormat::LittleEndian => body.extend_from_slice(value),
        DataFormat::BigEndian => body.extend(value.iter().rev().copied()),
    }
    body.push(CTRL_END);
    finalize(body)
}

/// Extract the value/payload bytes from a device response.
///
/// The device overwrites the shared buffer, so the response comes back framed
/// like the request: `[0x3F][LE16 len][0x26][code][value…][0x28]`. Returns the
/// `value…` slice. If the response isn't framed (some transports hand back only
/// the payload), the whole slice is returned unchanged.
pub fn decode_response(resp: &[u8]) -> &[u8] {
    if resp.len() >= 6 && resp[0] == MSG_CLASS && resp[3] == CTRL_BEGIN {
        let payload_len = (resp[1] as usize) | ((resp[2] as usize) << 8);
        let end = (HEADER_LEN + payload_len).min(resp.len());
        // strip [0x26][code] prefix and a trailing [0x28] if present
        let start = 5.min(end);
        let mut stop = end;
        if stop > start && resp[stop - 1] == CTRL_END {
            stop -= 1;
        }
        &resp[start..stop]
    } else {
        resp
    }
}

const HEADER_LEN: usize = 3; // class + LE16 length

/// Prepend the 3-byte header `[0x3F][LE16 payload-len]` to a framed body.
fn finalize(body: Vec<u8>) -> Vec<u8> {
    let len = body.len() as u16; // payload length = bytes after the 3-byte header
    let mut out = Vec::with_capacity(HEADER_LEN + body.len());
    out.push(MSG_CLASS);
    out.push((len & 0xFF) as u8);
    out.push((len >> 8) as u8);
    out.extend_from_slice(&body);
    out
}

/// Compile a Simplified-CCL script: a sequence of `(op, value)` pairs, each encoded
/// as `[op:u8][value:u32 big-endian]` (RE §4ter, `pm_script_load_op_sccl`).
pub fn encode_sccl(ops: &[(u8, u32)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ops.len() * 5);
    for &(op, val) in ops {
        out.push(op);
        out.extend_from_slice(&val.to_be_bytes()); // BIG-endian operand
    }
    out
}

/// The 14 SCCL ops `pm_script_generate_sccl` emits, in the recovered order.
/// `topl`/`botr` are inclusive sensor-pixel ROI corners.
#[allow(clippy::too_many_arguments)]
pub fn sccl_acquisition_script(
    exp_resolution: u32,
    exposure: u32,
    exposure_mode: u32,
    frame_count: u32,
    mode: u32,
    sub_roi_count: u32,
    bin_x: u32,
    bin_y: u32,
    topl_x: u32,
    topl_y: u32,
    botr_x: u32,
    botr_y: u32,
    clear_mode: u32,
    clear_count: u32,
) -> Vec<u8> {
    encode_sccl(&[
        (sccl::EXPOSE_RESOLUTION, exp_resolution),
        (sccl::EXPOSURETIME, exposure),
        (sccl::EXPOSUREMODE, exposure_mode),
        (sccl::FRAME_COUNT, frame_count),
        (sccl::MODE, mode),
        (sccl::SUB_ROI_COUNT, sub_roi_count),
        (sccl::BIN_X, bin_x),
        (sccl::BIN_Y, bin_y),
        (sccl::IMGCOORD_TOPL_X, topl_x),
        (sccl::IMGCOORD_TOPL_Y, topl_y),
        (sccl::IMGCOORD_BOTR_X, botr_x),
        (sccl::IMGCOORD_BOTR_Y, botr_y),
        (sccl::SENSOR_CLEAR_MODE, clear_mode),
        (sccl::SENSOR_CLEAR_COUNT, clear_count),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_frame_layout() {
        // temperature read: code 0x07, 2-byte response.
        let (f, off) = encode_read(code::TEMP_R, 2);
        // [0x3F][len_lo][len_hi][0x26][0x07][00][00][0x28]
        assert_eq!(f[0], MSG_CLASS);
        let payload_len = (f[1] as u16) | ((f[2] as u16) << 8);
        assert_eq!(payload_len as usize, f.len() - HEADER_LEN);
        assert_eq!(f[3], CTRL_BEGIN);
        assert_eq!(f[4], code::TEMP_R);
        assert_eq!(*f.last().unwrap(), CTRL_END);
        assert_eq!(off, 5); // response value lands right after [begin][code]
        assert_eq!(&f[off..off + 2], &[0, 0]);
    }

    #[test]
    fn write_frame_le_and_be() {
        // setpoint write (i16 LE on host): value 0x1234 -> bytes 34 12
        let f = encode_write(
            code::TEMP_SETPOINT_W,
            &0x1234u16.to_le_bytes(),
            DataFormat::LittleEndian,
        );
        assert_eq!(&f[3..6], &[CTRL_BEGIN, code::TEMP_SETPOINT_W, 0x34]);
        assert_eq!(f[6], 0x12);
        // big-endian swap reverses the value bytes
        let f2 = encode_write(0x99, &[0xAA, 0xBB], DataFormat::BigEndian);
        assert_eq!(&f2[5..7], &[0xBB, 0xAA]);
    }

    #[test]
    fn decode_response_strips_frame() {
        // framed response carrying value bytes 0x6C 0xEE (temp = -4500)
        let resp = [MSG_CLASS, 4, 0, CTRL_BEGIN, code::TEMP_R, 0x6C, 0xEE, CTRL_END];
        assert_eq!(decode_response(&resp), &[0x6C, 0xEE]);
        // unframed payload passes through
        assert_eq!(decode_response(&[0x6C, 0xEE]), &[0x6C, 0xEE]);
    }

    #[test]
    fn sccl_entry_is_op_plus_be32() {
        let s = encode_sccl(&[(sccl::EXPOSURETIME, 0x00112233)]);
        assert_eq!(s, vec![sccl::EXPOSURETIME, 0x00, 0x11, 0x22, 0x33]);
    }

    #[test]
    fn acquisition_script_has_14_entries() {
        let s = sccl_acquisition_script(0, 40, 0, 1, 0, 1, 1, 1, 0, 0, 2047, 2047, 0, 2);
        assert_eq!(s.len(), 14 * 5);
        assert_eq!(s[0], sccl::EXPOSE_RESOLUTION);
        // exposuretime entry (2nd) operand == 40, big-endian
        assert_eq!(&s[5..10], &[sccl::EXPOSURETIME, 0, 0, 0, 40]);
    }
}
