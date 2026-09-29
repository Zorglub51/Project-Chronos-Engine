use thiserror::Error;

#[derive(Debug, Error)]
pub enum FelError {
    #[error("no FEL device found (VID:PID 1f3a:efe8)")]
    NotFound,

    #[error("USB transfer error: {0}")]
    Usb(String),

    #[error(
        "device returned bad AWUS status: signature={signature:x?} csw_status={csw_status:#x}"
    )]
    BadStatus { signature: [u8; 4], csw_status: u8 },

    #[error("short transfer: expected {expected} bytes, got {actual}")]
    ShortTransfer { expected: usize, actual: usize },

    #[error("device returned unexpected version signature: {0:x?}")]
    BadVersion([u8; 8]),

    #[error("operation requires {expected:?} state, device is {actual:?}")]
    WrongState {
        expected: crate::device::DeviceState,
        actual: crate::device::DeviceState,
    },
}

pub type Result<T> = std::result::Result<T, FelError>;
