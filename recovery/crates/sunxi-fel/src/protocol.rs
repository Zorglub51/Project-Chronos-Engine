//! Wire-format types for the Allwinner FEL protocol.
//!
//! Two layers:
//! 1. AWUC envelope (host→device 28 bytes / device→host AWUS status 13 bytes),
//!    a SCSI-CBW-style wrapper that carries an inner FEL request and frames
//!    the bulk transfer that follows.
//! 2. FEL request (16 bytes), the actual FEL command (version, read, write,
//!    exec) sent as the payload of a `0x12` AWUC.
//!
//! Reference: linux-sunxi/sunxi-tools `fel_lib.c`.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub const AWUC_MAGIC: &[u8; 8] = b"AWUC\0\0\0\0";
pub const AWUS_MAGIC: &[u8; 4] = b"AWUS";

pub const AW_USB_READ: u16 = 0x11;
pub const AW_USB_WRITE: u16 = 0x12;

pub const AW_FEL_VERSION: u32 = 0x001;
pub const AW_FEL_WRITE: u32 = 0x101;
pub const AW_FEL_EXEC: u32 = 0x102;
pub const AW_FEL_READ: u32 = 0x103;

/// Outer envelope. All multi-byte fields little-endian.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct AwUsbRequest {
    pub signature: [u8; 8], // "AWUC\0\0\0\0"
    pub length: u32,        // payload length
    pub unknown1: u32,      // 0x0c000000
    pub request: u16,       // AW_USB_READ / AW_USB_WRITE
    pub length2: u32,       // == length
    pub pad: [u8; 10],
}

impl AwUsbRequest {
    pub fn new(request: u16, length: u32) -> Self {
        Self {
            signature: *AWUC_MAGIC,
            length,
            unknown1: 0x0c00_0000,
            request,
            length2: length,
            pad: [0; 10],
        }
    }
}

/// Status returned by the device after each AWUC transaction (13 bytes total).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct AwUsbStatus {
    pub signature: [u8; 4], // "AWUS"
    pub tag: u32,
    pub residue: u32,
    pub csw_status: u8,
}

/// Inner FEL command, payload of a `0x12` AWUC.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct FelRequest {
    pub request: u32,
    pub address: u32,
    pub length: u32,
    pub pad: u32,
}

impl FelRequest {
    pub fn new(request: u32, address: u32, length: u32) -> Self {
        Self {
            request,
            address,
            length,
            pad: 0,
        }
    }
}

/// Look up a human-readable name for a 16-bit Allwinner SoC code (the meaningful
/// middle two bytes of `FelVersion::soc_id`). List is partial — covers the parts
/// you'd realistically meet with sunxi-fel. PCE Mini = R16 = `0x1667`.
pub fn soc_name(code: u16) -> Option<&'static str> {
    Some(match code {
        0x1623 => "A10",
        0x1625 => "A13",
        0x1633 => "A31",
        0x1639 => "A80",
        0x1650 => "A23",
        0x1651 => "A20",
        0x1667 => "A33 / R16",
        0x1673 => "A83T",
        0x1680 => "H3",
        0x1681 => "V3s",
        0x1689 => "A64",
        0x1701 => "R40",
        0x1708 => "T7",
        0x1718 => "H5",
        0x1719 => "A63",
        0x1728 => "H6",
        0x1755 => "H616",
        _ => return None,
    })
}

/// 32-byte response to AW_FEL_VERSION. Field layout per upstream `fel_lib.c`.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct FelVersion {
    pub signature: [u8; 8], // "AWUSBFEX"
    pub soc_id: u32,        // e.g. 0x1667 for R16 (PCE Mini)
    pub unknown_0a: u32,
    pub protocol: u32,
    pub unknown_12: u8,
    pub unknown_13: u8,
    pub board: u8,
    pub unknown_15: u8,
    pub reserved: [u8; 8],
}

impl FelVersion {
    /// The meaningful 16-bit SoC code: bits 8..24 of `soc_id`. The boot ROM
    /// reports the SoC ID as `0x00xxxx00` where `xxxx` is the part code.
    pub fn soc_code(&self) -> u16 {
        ((self.soc_id >> 8) & 0xffff) as u16
    }

    /// Human-readable SoC name, if known. See `soc_name`.
    pub fn soc_name(&self) -> Option<&'static str> {
        soc_name(self.soc_code())
    }
}
