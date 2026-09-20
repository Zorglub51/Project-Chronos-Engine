use crate::{report, Reporter};
use anyhow::{bail, ensure, Context, Result};
use pcd_core::{create_pecd, read_bios, CreateOptions, PcdArchive};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BiosConfig {
    pub super_path: String,
    pub system_path: String,
    pub source: String,
}
#[derive(Serialize)]
pub struct BiosStatus {
    pub ready: bool,
    pub message: String,
}

fn validate(bytes: &[u8], label: &str) -> Result<()> {
    ensure!(
        bytes.len() == 262144,
        "{label}: expected a 256 KiB BIOS, found {} bytes",
        bytes.len()
    );
    ensure!(
        !bytes.iter().all(|b| *b == 0) && !bytes.iter().all(|b| *b == 255),
        "{label}: BIOS is empty"
    );
    Ok(())
}

pub(crate) fn store_bios(
    super_bios: &[u8],
    system: &[u8],
    source: &str,
    cache: &Path,
) -> Result<BiosConfig> {
    validate(super_bios, "Super System Card")?;
    validate(system, "System Card")?;
    let mut hash = Sha256::new();
    hash.update(super_bios);
    hash.update(system);
    let destination = cache.join(format!("{:x}", hash.finalize()));
    fs::create_dir_all(cache)?;
    if !destination.exists() {
        let temp = tempfile::tempdir_in(cache)?;
        fs::write(temp.path().join("super.pce"), super_bios)?;
        fs::write(temp.path().join("system.pce"), system)?;
        fs::rename(temp.path(), &destination)?;
    }
    ensure!(
        fs::read(destination.join("super.pce"))? == super_bios
            && fs::read(destination.join("system.pce"))? == system,
        "Cached BIOS files are damaged: {}",
        destination.display()
    );
    Ok(BiosConfig {
        super_path: destination.join("super.pce").to_string_lossy().into(),
        system_path: destination.join("system.pce").to_string_lossy().into(),
        source: source.into(),
    })
}

pub(crate) fn extract_bios(reader: impl Read, source: &str, cache: &Path) -> Result<BiosConfig> {
    let bios = read_bios(reader).context("Cannot read BIOS sections from this PCD")?;
    store_bios(
        bios.bios
            .as_deref()
            .context("PCD has no Super System Card BIOS (bios section)")?,
        bios.bios2
            .as_deref()
            .context("PCD has no System Card BIOS (bio2 section)")?,
        source,
        cache,
    )
}

pub fn configure_bios(
    pcd: Option<&Path>,
    super_path: Option<&Path>,
    system_path: Option<&Path>,
    cache: &Path,
) -> Result<BiosConfig> {
    if let Some(pcd) = pcd {
        return extract_bios(
            File::open(pcd).with_context(|| format!("Open {}", pcd.display()))?,
            &pcd.to_string_lossy(),
            cache,
        );
    }
    let super_path = super_path.context("Select a Super System Card BIOS")?;
    let system_path = system_path.context("Select a System Card BIOS")?;
    store_bios(
        &read_bios_file(super_path, "Super System Card")?,
        &read_bios_file(system_path, "System Card")?,
        "Selected BIOS files",
        cache,
    )
}
fn read_bios_file(path: &Path, label: &str) -> Result<Vec<u8>> {
    ensure!(
        fs::metadata(path)
            .with_context(|| format!("{label}: {}", path.display()))?
            .len()
            == 262144,
        "{label}: expected a 256 KiB BIOS"
    );
    let bytes = fs::read(path)?;
    validate(&bytes, label)?;
    Ok(bytes)
}
pub fn bios_status(config: Option<&BiosConfig>) -> BiosStatus {
    let result = (|| -> Result<()> {
        let config =
            config.context("No BIOS configured. Extract them from an original PCD in Settings.")?;
        read_bios_file(Path::new(&config.super_path), "Super System Card")?;
        read_bios_file(Path::new(&config.system_path), "System Card")?;
        Ok(())
    })();
    BiosStatus {
        ready: result.is_ok(),
        message: result
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap_or_else(|| "Both BIOS files are ready for CD conversion.".into()),
    }
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub filename: String,
    pub converted: bool,
    pub cd: bool,
}

/// Write alongside the game in a temporary file; never replace an existing ROM.
pub fn import_rom(
    source: &Path,
    destination: &Path,
    bios: Option<&BiosConfig>,
    progress: Reporter<'_>,
) -> Result<ImportResult> {
    ensure!(
        source.is_file(),
        "ROM file does not exist: {}",
        source.display()
    );
    let extension = source
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    ensure!(
        ["cue", "pcd", "pce", "sgx", "bin"].contains(&extension.as_str()),
        "Unsupported ROM format: {extension}"
    );
    if extension == "bin" {
        bail!("For a CD game, select the .cue file describing all tracks, or an existing .pcd. Rename a raw HuCard ROM to .pce before importing it.");
    }
    let converted = extension == "cue";
    let cd = converted || extension == "pcd";
    if converted {
        let status = bios_status(bios);
        ensure!(status.ready, "{}", status.message);
        // Fail with the missing track's filename before creating output files.
        let cue = pcd_core::cue::CueSheet::parse_file(source).context("Invalid CUE sheet")?;
        for file in cue.files {
            let track = source.parent().unwrap().join(&file.filename);
            ensure!(
                track.is_file(),
                "CUE track is missing: {}. Keep all BIN files beside the CUE.",
                track.display()
            );
        }
    } else if cd {
        PcdArchive::open(source).context("Invalid or truncated PCD archive")?;
    }
    fs::create_dir_all(destination)?;
    let name = source.file_name().context("ROM has no filename")?;
    let mut output = destination.join(name);
    if converted {
        output.set_extension("pcd");
    }
    if !converted && output.exists() && fs::canonicalize(source)? == fs::canonicalize(&output)? {
        return Ok(ImportResult {
            filename: name.to_string_lossy().into(),
            converted,
            cd,
        });
    }
    let stem = output.file_stem().unwrap().to_string_lossy().to_string();
    let ext = output.extension().unwrap().to_string_lossy().to_string();
    let mut suffix = 2;
    while output.exists() {
        output = destination.join(format!("{stem} ({suffix}).{ext}"));
        suffix += 1;
    }
    let mut temporary = tempfile::NamedTempFile::new_in(destination)?;
    if converted {
        let bios = bios.unwrap();
        report(progress, "Reading CD track layout…", None);
        let callback = |message: &str| {
            let label = if message.starts_with("encoding data") {
                "Compressing data tracks…"
            } else if message.starts_with("encoded") && message.contains("data") {
                "Data tracks compressed"
            } else if message.starts_with("encoding audio") {
                "Converting audio tracks…"
            } else if message.starts_with("encoded") && message.contains("audio") {
                "Audio tracks converted"
            } else if message.starts_with("writing") {
                "Writing PCD archive…"
            } else if message == "done" {
                "Checking PCD archive…"
            } else {
                "CD track layout checked"
            };
            report(progress, label, None);
        };
        create_pecd(CreateOptions {
            cue_paths: &[source.to_path_buf()],
            out_path: temporary.path(),
            super_bios: Path::new(&bios.super_path),
            syscard: Path::new(&bios.system_path),
            arch_key: 0x4348524f,
            lz_level: pcd_core::create::LzLevel::Default,
            progress: Some(&callback),
        })
        .context("CD conversion failed")?;
        PcdArchive::open(temporary.path()).context("Converted PCD failed verification")?;
    } else {
        let mut input = File::open(source)?;
        let total = input.metadata()?.len();
        let mut bytes = vec![0; 1024 * 1024];
        let mut copied = 0;
        loop {
            let count = input.read(&mut bytes)?;
            if count == 0 {
                break;
            }
            temporary.write_all(&bytes[..count])?;
            copied += count as u64;
            report(progress, "Copying ROM…", Some((copied, total)));
        }
    }
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(&output)
        .with_context(|| format!("Save imported ROM to {}", output.display()))?;
    report(progress, "Import complete", Some((1, 1)));
    Ok(ImportResult {
        filename: output.file_name().unwrap().to_string_lossy().into(),
        converted,
        cd,
    })
}
