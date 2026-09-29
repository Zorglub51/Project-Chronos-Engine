//! NAND/eMMC partition table for the PC Engine Mini.
//!
//! Hardcoded from `nand_dump/Program.cs:20-30`. The partition layout is
//! fixed by the factory firmware and identical across all units; we treat
//! it as a constant rather than discovering it via GPT (which would also
//! work, but only after the device is up).

#[derive(Debug, Clone, Copy)]
pub struct Partition {
    /// Number after `mmcblk0p` — e.g. `1` for `/dev/mmcblk0p1`.
    pub id: u8,
    /// Path on the device, used as the TFTP filename.
    pub device_path: &'static str,
    /// Short human-readable role (factory layout).
    pub role: &'static str,
    /// Size in 512-byte sectors.
    pub sectors: u64,
}

impl Partition {
    pub const SECTOR_SIZE: u64 = 512;

    pub fn size_bytes(&self) -> u64 {
        self.sectors * Self::SECTOR_SIZE
    }
}

/// Per-partition layout. Sizes verified against `nand_dump/Program.cs:20-30`.
/// p3 and p4 are intentionally absent (extended-partition table slots in the
/// factory layout — no payload).
pub const PARTITIONS: &[Partition] = &[
    Partition {
        id: 1,
        device_path: "/dev/mmcblk0p1",
        role: "System",
        sectors: 1_347_584,
    }, // 658 MiB
    Partition {
        id: 2,
        device_path: "/dev/mmcblk0p2",
        role: "System",
        sectors: 16_384,
    }, //   8 MiB
    Partition {
        id: 5,
        device_path: "/dev/mmcblk0p5",
        role: "System",
        sectors: 4_096,
    }, //   2 MiB
    Partition {
        id: 6,
        device_path: "/dev/mmcblk0p6",
        role: "Kernel",
        sectors: 16_384,
    }, //   8 MiB
    Partition {
        id: 7,
        device_path: "/dev/mmcblk0p7",
        role: "Linux",
        sectors: 204_800,
    }, // 100 MiB
    Partition {
        id: 8,
        device_path: "/dev/mmcblk0p8",
        role: "Saves",
        sectors: 1_376_256,
    }, // 672 MiB
    Partition {
        id: 9,
        device_path: "/dev/mmcblk0p9",
        role: "Games",
        sectors: 4_587_520,
    }, // 2.19 GiB
    Partition {
        id: 10,
        device_path: "/dev/mmcblk0p10",
        role: "System",
        sectors: 8_192,
    }, //   4 MiB
];

/// Whole NAND device (3.64 GiB total per nand_dump). The TFTP filename is
/// the bare block device.
pub const FULL: Partition = Partition {
    id: 0,
    device_path: "/dev/mmcblk0",
    role: "Full NAND",
    // 3,909,091,328 / 512 — matches nand_dump's hardcoded total.
    sectors: 7_634_944,
};

/// Look up a partition by id. `0` resolves to the whole-NAND `FULL` block
/// device — matches nand_dump's `full` / `fullrestore` semantics with a
/// single uniform interface.
pub fn lookup(id: u8) -> Option<&'static Partition> {
    if id == 0 {
        Some(&FULL)
    } else {
        PARTITIONS.iter().find(|p| p.id == id)
    }
}

#[deprecated(note = "use lookup(id) — also resolves 0 → FULL")]
pub fn by_id(id: u8) -> Option<&'static Partition> {
    lookup(id)
}
