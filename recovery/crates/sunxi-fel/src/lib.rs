//! Pure-Rust port of the Allwinner FEL (USB recovery) protocol.
//!
//! Targets the PC Engine Mini (Allwinner R16, USB VID:PID 1f3a:efe8) on Linux and macOS,
//! using `nusb` so there is no libusb dependency.

pub mod device;
pub mod error;
pub mod protocol;

pub use device::{
    detect_state, is_present, wait_for_device, wait_for_state, DeviceState, FelDevice, FEL_PID,
    FEL_VID,
};
pub use error::{FelError, Result};
pub use protocol::FelVersion;
