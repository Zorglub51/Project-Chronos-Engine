//! Desktop import pipeline. Sources are read-only; outputs are staged before commit.
mod cd;
mod covers;
mod source;
mod stock;

pub use cd::{bios_status, configure_bios, import_rom, BiosConfig, BiosStatus, ImportResult};
use serde::Serialize;
pub use stock::{create_library, NewLibraryResult};

#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub message: String,
    pub completed: Option<u64>,
    pub total: Option<u64>,
}
pub type Reporter<'a> = &'a dyn Fn(Progress);
pub(crate) fn report(
    progress: Reporter<'_>,
    message: impl Into<String>,
    counts: Option<(u64, u64)>,
) {
    progress(Progress {
        message: message.into(),
        completed: counts.map(|x| x.0),
        total: counts.map(|x| x.1),
    });
}
