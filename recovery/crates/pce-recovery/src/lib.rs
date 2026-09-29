//! Post-recovery (in-recovery-image) tooling: TCP partition transfer + SSH
//! ad-hoc commands against the PCE Mini's recovery initrd at `169.254.13.37`.

pub mod image;
pub mod nc;
pub mod partitions;
pub mod ssh;

pub use nc::{dump_partition, restore_partition, NcError};
pub use partitions::{Partition, FULL, PARTITIONS};
pub use ssh::{exec as ssh_exec, ExecOutput, SshError};

/// IP the recovery initrd brings up on its USB-RNDIS gadget. Hardcoded in
/// `nand_dump/Program.cs:15` and matches the Linux network helper.
pub const PEER_IP: [u8; 4] = [169, 254, 13, 37];
