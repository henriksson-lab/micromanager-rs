/// Shared helpers for Squid+ MCU devices.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::transport::Transport;
use crate::types::PropertyValue;

use super::protocol;

/// Send a binary packet and wait for a COMPLETED response.
pub fn send_and_wait(transport: &mut dyn Transport, pkt: &[u8]) -> MmResult<()> {
    let expected_id = pkt.first().copied().ok_or(MmError::SerialInvalidResponse)?;
    transport.send_bytes(pkt)?;
    let resp = transport.receive_bytes(protocol::MSG_LENGTH)?;
    match protocol::parse_response(&resp) {
        Some((id, _)) if id != expected_id => Err(MmError::SerialInvalidResponse),
        Some((_id, status)) if status == protocol::STATUS_COMPLETED => Ok(()),
        Some((_id, status)) if status == protocol::STATUS_IN_PROGRESS => loop {
            let resp = transport.receive_bytes(protocol::MSG_LENGTH)?;
            match protocol::parse_response(&resp) {
                Some((id, _)) if id != expected_id => return Err(MmError::SerialInvalidResponse),
                Some((_id, s)) if s == protocol::STATUS_COMPLETED => return Ok(()),
                Some(_) => continue,
                None => return Err(MmError::SerialInvalidResponse),
            }
        },
        Some(_) => Err(MmError::SerialCommandFailed),
        None => Err(MmError::SerialInvalidResponse),
    }
}

/// Illumination source mapping: property name suffix → source index.
const ILLUMINATION_SOURCES: &[(&str, u8)] = &[
    ("Illumination-405nm", 11),
    ("Illumination-488nm", 12),
    ("Illumination-561nm", 14),
    ("Illumination-638nm", 13),
    ("Illumination-730nm", 15),
    ("Illumination-LED", 20),
];

/// Define all illumination properties on a PropertyMap.
pub fn define_illumination_props(props: &mut PropertyMap) {
    for &(name, _) in ILLUMINATION_SOURCES {
        props
            .define_property(name, PropertyValue::Float(0.0), false)
            .unwrap();
    }
    props
        .define_property("Illumination-On", PropertyValue::Integer(0), false)
        .unwrap();
}

/// Handle set_property for illumination names.
/// Returns `Some(Ok/Err)` if the property was an illumination property,
/// `None` if the property name is not illumination-related.
pub fn handle_illumination_set(
    name: &str,
    val: &PropertyValue,
    transport: &mut dyn Transport,
    cmd_id: &mut u8,
) -> Option<MmResult<()>> {
    if name == "Illumination-On" {
        let on = val.as_i64().unwrap_or(0) != 0;
        let id = next_id(cmd_id);
        let pkt = if on {
            protocol::build_turn_on_illumination(id)
        } else {
            protocol::build_turn_off_illumination(id)
        };
        return Some(send_and_wait(transport, &pkt));
    }

    for &(prop_name, source) in ILLUMINATION_SOURCES {
        if name == prop_name {
            let intensity_pct = val.as_f64().unwrap_or(0.0).clamp(0.0, 100.0);
            let intensity_u16 = (intensity_pct / 100.0 * 65535.0) as u16;
            let id = next_id(cmd_id);
            let pkt = protocol::build_set_illumination(id, source, intensity_u16);
            return Some(send_and_wait(transport, &pkt));
        }
    }

    None
}

fn next_id(cmd_id: &mut u8) -> u8 {
    *cmd_id = cmd_id.wrapping_add(1);
    *cmd_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn response(cmd_id: u8, status: u8) -> Vec<u8> {
        let mut buf = vec![0u8; protocol::MSG_LENGTH];
        buf[0] = cmd_id;
        buf[1] = status;
        buf[protocol::MSG_LENGTH - 1] = protocol::crc8(&buf[..protocol::MSG_LENGTH - 1]);
        buf
    }

    #[test]
    fn send_and_wait_rejects_mismatched_ack_id() {
        let pkt = protocol::build_home(7, protocol::AXIS_Z, protocol::HOME_POSITIVE);
        let mut transport =
            MockTransport::new().expect_binary(&response(8, protocol::STATUS_COMPLETED));

        assert_eq!(
            send_and_wait(&mut transport, &pkt),
            Err(MmError::SerialInvalidResponse)
        );
    }

    #[test]
    fn send_and_wait_rejects_mismatched_completion_after_progress() {
        let pkt = protocol::build_home(7, protocol::AXIS_Z, protocol::HOME_POSITIVE);
        let mut transport = MockTransport::new()
            .expect_binary(&response(7, protocol::STATUS_IN_PROGRESS))
            .expect_binary(&response(8, protocol::STATUS_COMPLETED));

        assert_eq!(
            send_and_wait(&mut transport, &pkt),
            Err(MmError::SerialInvalidResponse)
        );
    }
}
