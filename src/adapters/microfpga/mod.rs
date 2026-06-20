//! MicroFPGA device adapter.
//!
//! Binary protocol over serial (little-endian 32-bit addresses and values):
//! Write request: 9 bytes [0x80, addr_le_u32, value_le_u32]
//! Read  request: 5 bytes [0x00, addr_le_u32]
//! Response:      4 bytes [value_le_u32]  (for reads)
//!
//! Key addresses (firmware version 3):
//!   LaserMode[0..8]:     0..7
//!   LaserDuration[0..8]: 8..15
//!   LaserSequence[0..8]: 16..23
//!   TTL[0..4]:           24..27
//!   Servo[0..7]:         28..34
//!   PWM[0..5]:           35..39
//!   CamSyncMode:         40
//!   CamTriggerStart:     41
//!   CamPulse:            42
//!   CamReadout:          43
//!   CamExposure:         44
//!   LaserDelay:          45
//!   AnalogInput[0..8]:   46..53
//!   Version:             200
//!   ID:                  201
//!
//! Devices exported:
//! - `MicroFPGA-Hub`    — Hub
//! - `Camera Trigger`   — Generic
//! - `Laser Trigger`    — Generic
//! - `Analog Input`     — Generic
//! - `PWM`              — Generic
//! - `TTL`              — Generic
//! - `Servos`           — Generic

pub mod analog_input;
pub mod camera_trigger;
pub mod hub;
pub mod laser_trigger;
pub mod pwm;
pub mod servo;
pub mod ttl;

pub use analog_input::AnalogInput;
pub use camera_trigger::CameraTrigger;
pub use hub::MicroFpgaHub;
pub use laser_trigger::LaserTrigger;
pub use pwm::FpgaPwm;
pub use servo::FpgaServo;
pub use ttl::FpgaTtl;

use crate::error::{MmError, MmResult};
use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::transport::Transport;
use crate::types::DeviceType;

pub const DEVICE_NAME_HUB: &str = "MicroFPGA-Hub";
pub const DEVICE_NAME_CAM_TRIG: &str = "Camera Trigger";
pub const DEVICE_NAME_LASER_TRIG: &str = "Laser Trigger";
pub const DEVICE_NAME_ANALOG: &str = "Analog Input";
pub const DEVICE_NAME_PWM: &str = "PWM";
pub const DEVICE_NAME_TTL: &str = "TTL";
pub const DEVICE_NAME_SERVO: &str = "Servos";

// Address constants matching firmware version 3
pub const ADDR_VERSION: u32 = 200;
pub const ADDR_ID: u32 = 201;

pub const OFFSET_LASER_MODE: u32 = 0;
pub const MAX_LASERS: u32 = 8;
pub const MAX_ANALOG_INPUT: u32 = 8;
pub const OFFSET_LASER_DURATION: u32 = OFFSET_LASER_MODE + MAX_LASERS;
pub const OFFSET_LASER_SEQUENCE: u32 = OFFSET_LASER_DURATION + MAX_LASERS;
pub const MAX_TTL: u32 = 4;
pub const OFFSET_TTL: u32 = OFFSET_LASER_SEQUENCE + MAX_LASERS;
pub const MAX_SERVOS: u32 = 7;
pub const OFFSET_SERVO: u32 = OFFSET_TTL + MAX_TTL;
pub const MAX_PWM: u32 = 5;
pub const OFFSET_PWM: u32 = OFFSET_SERVO + MAX_SERVOS;
pub const OFFSET_CAM_SYNC_MODE: u32 = OFFSET_PWM + MAX_PWM;
pub const OFFSET_CAM_TRIGGER_START: u32 = OFFSET_CAM_SYNC_MODE + 1;
pub const OFFSET_CAM_PULSE: u32 = OFFSET_CAM_TRIGGER_START + 1;
pub const OFFSET_CAM_READOUT: u32 = OFFSET_CAM_PULSE + 1;
pub const OFFSET_CAM_EXPOSURE: u32 = OFFSET_CAM_READOUT + 1;
pub const OFFSET_LASER_DELAY: u32 = OFFSET_CAM_EXPOSURE + 1;
pub const OFFSET_ANALOG_INPUT: u32 = OFFSET_LASER_DELAY + 1;

pub const FIRMWARE_VERSION: u32 = 3;

// Known board IDs
pub const ID_AU: u32 = 79;
pub const ID_AUP: u32 = 80;
pub const ID_CU: u32 = 29;
pub const ID_MOJO: u32 = 12;

pub(crate) fn read_register(t: &mut dyn Transport, addr: u32) -> MmResult<u32> {
    let req = [
        0x00u8,
        (addr & 0xFF) as u8,
        ((addr >> 8) & 0xFF) as u8,
        ((addr >> 16) & 0xFF) as u8,
        ((addr >> 24) & 0xFF) as u8,
    ];
    t.send_bytes(&req)?;
    let raw = t.receive_bytes(4)?;
    if raw.len() < 4 {
        return Err(MmError::SerialInvalidResponse);
    }
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

pub(crate) fn write_register(t: &mut dyn Transport, addr: u32, value: u32) -> MmResult<()> {
    let bytes = [
        0x80u8,
        (addr & 0xFF) as u8,
        ((addr >> 8) & 0xFF) as u8,
        ((addr >> 16) & 0xFF) as u8,
        ((addr >> 24) & 0xFF) as u8,
        (value & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        ((value >> 16) & 0xFF) as u8,
        ((value >> 24) & 0xFF) as u8,
    ];
    t.send_bytes(&bytes)
}

static DEVICE_LIST: &[DeviceInfo] = &[
    DeviceInfo {
        name: DEVICE_NAME_HUB,
        description: "MicroFPGA Hub (required)",
        device_type: DeviceType::Hub,
    },
    DeviceInfo {
        name: DEVICE_NAME_CAM_TRIG,
        description: "Camera Trigger",
        device_type: DeviceType::Generic,
    },
    DeviceInfo {
        name: DEVICE_NAME_LASER_TRIG,
        description: "Laser Trigger",
        device_type: DeviceType::Generic,
    },
    DeviceInfo {
        name: DEVICE_NAME_ANALOG,
        description: "Analog Input",
        device_type: DeviceType::Generic,
    },
    DeviceInfo {
        name: DEVICE_NAME_PWM,
        description: "PWM Output",
        device_type: DeviceType::Generic,
    },
    DeviceInfo {
        name: DEVICE_NAME_TTL,
        description: "TTL Output",
        device_type: DeviceType::Generic,
    },
    DeviceInfo {
        name: DEVICE_NAME_SERVO,
        description: "Servos",
        device_type: DeviceType::Generic,
    },
];

pub struct MicroFpgaAdapter;

impl AdapterModule for MicroFpgaAdapter {
    fn module_name(&self) -> &'static str {
        "microfpga"
    }
    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }
    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_HUB => Some(AnyDevice::Hub(Box::new(MicroFpgaHub::new()))),
            DEVICE_NAME_CAM_TRIG => Some(AnyDevice::Generic(Box::new(CameraTrigger::new()))),
            DEVICE_NAME_LASER_TRIG => Some(AnyDevice::Generic(Box::new(LaserTrigger::new()))),
            DEVICE_NAME_ANALOG => Some(AnyDevice::Generic(Box::new(AnalogInput::new()))),
            DEVICE_NAME_PWM => Some(AnyDevice::Generic(Box::new(FpgaPwm::new()))),
            DEVICE_NAME_TTL => Some(AnyDevice::Generic(Box::new(FpgaTtl::new()))),
            DEVICE_NAME_SERVO => Some(AnyDevice::Generic(Box::new(FpgaServo::new()))),
            _ => None,
        }
    }
}
