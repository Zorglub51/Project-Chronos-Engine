// M2 PSB binary format. Used by PC Engine Mini and other M2 consoles to ship
// structured game-config data (motion, title list, ROM mapping, etc.).
//
// This crate reads and writes PSB files. Writing comes in a later phase.

mod reader;
mod trie;
mod value;
mod writer;

pub use reader::{read, Reader};
pub use value::{Stream, Value};
pub use writer::write;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("not a PSB file (bad magic)")]
    BadMagic,
    #[error("input shorter than expected at offset {0}")]
    Truncated(usize),
    #[error("unknown PSB token type {ty} at offset {pos}")]
    UnknownType { ty: u8, pos: usize },
    #[error("invalid uint type {0}")]
    InvalidUintType(u8),
    #[error("string contains invalid utf-8")]
    InvalidUtf8,
}
