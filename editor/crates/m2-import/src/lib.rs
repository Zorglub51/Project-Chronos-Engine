//! Desktop import pipeline. Sources are read-only; outputs are staged before commit.
mod cd;
mod covers;
mod replacement;
mod source;
mod stock;

pub use cd::{bios_status, configure_bios, import_rom, BiosConfig, BiosStatus, ImportResult};
pub use replacement::remove_replaced_rom;
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

/// Library imports go directly into the shared console-ready pool.
pub fn import_library_rom(source: &std::path::Path, game_dir: &std::path::Path,
    bios: Option<&BiosConfig>, progress: Reporter<'_>) -> anyhow::Result<ImportResult> {
    let Some(root) = m2_publish::rom_store::library_root(game_dir) else {
        return import_rom(source, game_dir, bios, progress);
    };
    let pool = m2_publish::rom_store::checked_pool(&root)?;
    let filename = source.file_name().and_then(|n| n.to_str()).ok_or_else(|| anyhow::anyhow!("Invalid ROM filename"))?;
    let ext = source.extension().and_then(|n| n.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case("pce") || ext.eq_ignore_ascii_case("sgx") {
        report(progress, "Preparing HuCard for the console…", None);
        let mut name = filename.to_owned();
        let mut suffix = 2;
        loop {
            match m2_publish::rom_store::store(source, &pool, &name) {
                Ok(path) => return Ok(ImportResult { filename: path.file_name().unwrap().to_string_lossy().into(), converted: false, cd: false }),
                Err(m2_publish::Error::Library(message)) if message.starts_with("ROM filename collision:") => {
                    name = format!("{} ({suffix}).{ext}", source.file_stem().unwrap().to_string_lossy());
                    suffix += 1;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    import_rom(source, &pool, bios, progress)
}
