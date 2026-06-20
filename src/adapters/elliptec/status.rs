use crate::error::MmError;

pub fn status_error(message: &str, device: &str) -> Option<MmError> {
    if message.len() < 5 || &message[1..3] != "GS" {
        return None;
    }

    match &message[3..5] {
        "00" => None,
        code => Some(status_code_to_error(code, device)),
    }
}

pub fn status_code_to_error(code: &str, device: &str) -> MmError {
    match code.get(0..2).unwrap_or(code) {
        "01" => MmError::SerialTimeout,
        "02" => MmError::LocallyDefined(format!("{} mechanical timeout", device)),
        "03" => MmError::UnsupportedCommand,
        "04" => MmError::InvalidPropertyValue,
        "05" => MmError::LocallyDefined(format!("{} module isolated", device)),
        "06" => MmError::LocallyDefined(format!("{} module out of isolation", device)),
        "07" => MmError::LocallyDefined(format!("{} initializing error", device)),
        "08" => MmError::LocallyDefined(format!("{} thermal error", device)),
        "09" => MmError::LocallyDefined(format!("{} busy", device)),
        "0A" => MmError::LocallyDefined(format!("{} sensor error", device)),
        "0B" => MmError::LocallyDefined(format!("{} motor error", device)),
        "0C" => MmError::InvalidPropertyValue,
        "0D" => MmError::LocallyDefined(format!("{} over-current error", device)),
        code => MmError::LocallyDefined(format!("{} status error {}", device, code)),
    }
}
