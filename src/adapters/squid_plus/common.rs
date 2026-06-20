/// Shared helpers for Squid+ MCU devices.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::transport::Transport;
use crate::types::PropertyValue;

use super::protocol;

/// Send a binary packet and wait for a COMPLETED response.
pub fn send_and_wait(transport: &mut dyn Transport, pkt: &[u8]) -> MmResult<()> {
    transport.send_bytes(pkt)?;
    let resp = transport.receive_bytes(protocol::MSG_LENGTH)?;
    match protocol::parse_response(&resp) {
        Some((_id, status)) if status == protocol::STATUS_COMPLETED => Ok(()),
        Some((_id, status)) if status == protocol::STATUS_IN_PROGRESS => loop {
            let resp = transport.receive_bytes(protocol::MSG_LENGTH)?;
            match protocol::parse_response(&resp) {
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
