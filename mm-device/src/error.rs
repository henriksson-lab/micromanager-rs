use thiserror::Error;

pub type MmResult<T> = Result<T, MmError>;

/// Error codes mirroring MMDeviceConstants.h
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MmError {
    #[error("generic error")]
    Err,
    #[error("invalid property")]
    InvalidProperty,
    #[error("invalid property value")]
    InvalidPropertyValue,
    #[error("duplicate property")]
    DuplicateProperty,
    #[error("invalid property type")]
    InvalidPropertyType,
    #[error("native module failed")]
    NativeModuleFailed,
    #[error("unsupported data format")]
    UnsupportedDataFormat,
    #[error("internal inconsistency")]
    InternalInconsistency,
    #[error("not supported")]
    NotSupported,
    #[error("unknown label: {0}")]
    UnknownLabel(String),
    #[error("unsupported command")]
    UnsupportedCommand,
    #[error("unknown position")]
    UnknownPosition,
    #[error("no callback registered")]
    NoCallbackRegistered,
    #[error("serial command failed")]
    SerialCommandFailed,
    #[error("serial buffer overrun")]
    SerialBufferOverrun,
    #[error("serial invalid response")]
    SerialInvalidResponse,
    #[error("serial timeout")]
    SerialTimeout,
    #[error("self reference")]
    SelfReference,
    #[error("no property data")]
    NoPropertyData,
    #[error("duplicate label")]
    DuplicateLabel,
    #[error("invalid input param")]
    InvalidInputParam,
    #[error("buffer overflow")]
    BufferOverflow,
    #[error("nonexistent channel")]
    NonexistentChannel,
    #[error("invalid property limits")]
    InvalidPropertyLimits,
    #[error("snap image failed")]
    SnapImageFailed,
    #[error("image params failed")]
    ImageParamsFailed,
    #[error("focus stage undefined")]
    CoreFocusStageUndef,
    #[error("exposure failed")]
    CoreExposureFailed,
    #[error("config failed")]
    CoreConfigFailed,
    #[error("camera busy acquiring")]
    CameraBusyAcquiring,
    #[error("incompatible image")]
    IncompatibleImage,
    #[error("cannot set property")]
    CanNotSetProperty,
    #[error("channel presets failed")]
    CoreChannelPresetsFailed,
    #[error("locally defined error: {0}")]
    LocallyDefined(String),
    #[error("not connected")]
    NotConnected,
    #[error("comm hub missing")]
    CommHubMissing,
    #[error("duplicate library")]
    DuplicateLibrary,
    #[error("property not sequenceable")]
    PropertyNotSequenceable,
    #[error("sequence too large")]
    SequenceTooLarge,
    #[error("out of memory")]
    OutOfMemory,
    #[error("not yet implemented")]
    NotYetImplemented,
    #[error("pump is running")]
    PumpIsRunning,
    #[error("device not found: {0}")]
    DeviceNotFound(String),
    #[error("wrong device type")]
    WrongDeviceType,
}
