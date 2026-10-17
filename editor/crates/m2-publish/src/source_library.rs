//! Lifecycle for a source-driven library. The editor prepares reusable assets;
//! menu metadata is read directly from game.json/gamelist.json by the console.
use crate::{console::ConsoleVariant, Error, SyncOptions, SyncReport, UsbPreparation};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const MARKER: &str = ".chronos-library-v2";
const BOOTSTRAP_ROM: &str = "CHRONOS_EMPTY.pce.m";

fn bootstrap_rom() -> Vec<u8> {
    let mut bytes = vec![0xea; 8192];
    bytes[..4].copy_from_slice(&[0x78, 0xd8, 0x80, 0xfe]);
    for offset in (0x1ff6..=0x1ffe).step_by(2) {
        bytes[offset..offset + 2].copy_from_slice(&0xe000u16.to_le_bytes());
    }
    bytes
}

pub(crate) fn is_bootstrap_rom(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == BOOTSTRAP_ROM)
        && fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file())
        && fs::read(path).is_ok_and(|bytes| {
            m2_mzs::unpack_default(&bytes, BOOTSTRAP_ROM).is_ok_and(|raw| raw == bootstrap_rom())
        })
}

fn prepare_bootstrap_rom(library: &Path) -> Result<(), Error> {
    use std::io::Write;
    let pool = crate::rom_store::checked_pool(library)?;
    let mut source = tempfile::Builder::new()
        .suffix(".pce")
        .tempfile_in(library)?;
    source.write_all(&bootstrap_rom())?;
    crate::rom_store::store(source.path(), &pool, BOOTSTRAP_ROM).map_err(|error| {
        Error::Library(format!(
            "Cannot prepare the empty-menu bootstrap ROM; existing files were kept: {error}"
        ))
    })?;
    Ok(())
}

pub struct SourceLibraryReport {
    pub usb: UsbPreparation,
    pub migration: SyncReport,
}

pub fn enabled(library: &Path) -> bool {
    fs::read(library.join(MARKER)).is_ok_and(|bytes| bytes == b"2\n")
}

pub fn game_root(library: &Path) -> Option<PathBuf> {
    if library.file_name()? != "library" {
        return None;
    }
    Some(library.parent()?.join("game"))
}

/// Bare legacy editing trees remain usable. A complete canonical key can be
/// upgraded automatically without guessing where its stock runtime lives.
pub fn can_prepare(library: &Path) -> bool {
    enabled(library) || game_root(library).is_some_and(|game| game.join("m2engage").is_file())
}

fn checked_directory(path: &Path) -> Result<(), Error> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if !meta.file_type().is_dir() {
            return Err(Error::Library(format!(
                "Expected a regular directory: {}",
                path.display()
            )));
        }
    }
    fs::create_dir_all(path)?;
    Ok(())
}

fn preserve_stock_templates(
    library: &Path,
    stock: &Path,
    game: &Path,
    console: ConsoleVariant,
) -> Result<(), Error> {
    let baseline = library.join("templates");
    checked_directory(&baseline)?;
    for relative in console
        .templates()
        .into_iter()
        .chain([crate::console::SYSTEM_PROFILE])
    {
        let destination = baseline.join(relative);
        let mut parent = baseline.clone();
        for component in Path::new(relative).parent().unwrap().components() {
            parent.push(component);
            checked_directory(&parent)?;
        }
        match fs::symlink_metadata(&destination) {
            Ok(meta) if meta.file_type().is_file() => continue,
            Ok(_) => {
                return Err(Error::Library(format!(
                    "Expected a regular original template: {}",
                    destination.display()
                )))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let mut source = stock.join(relative);
        if relative == crate::console::SYSTEM_PROFILE && !source.is_file() {
            source = game.join(relative);
        }
        // A complete legacy template set can identify its package without a
        // system profile. Preserve that valid layout rather than invent one.
        if relative == crate::console::SYSTEM_PROFILE && !source.exists() {
            continue;
        }
        if !fs::symlink_metadata(&source)?.file_type().is_file() {
            return Err(Error::Library(format!(
                "Expected a regular original template: {}",
                source.display()
            )));
        }
        crate::save_format::atomic_write(&destination, &fs::read(source)?)
            .map_err(Error::Library)?;
    }
    if ConsoleVariant::from_directory(&baseline)? != console {
        return Err(Error::Library(
            "Existing original library templates belong to a different console package".into(),
        ));
    }
    Ok(())
}

pub fn prepare(library: &Path, stock: &Path) -> Result<SourceLibraryReport, Error> {
    let game = game_root(library).ok_or_else(|| {
        Error::Library("Source libraries need the USB layout <root>/library and <root>/game".into())
    })?;
    checked_directory(library)?;
    checked_directory(&game)?;
    let console = ConsoleVariant::from_directory(stock)?;
    let missing: Vec<_> = crate::usb::ORIGINAL_FILES
        .iter()
        .copied()
        .filter(|relative| !game.join(relative).is_file())
        .collect();
    if !missing.is_empty() {
        return Err(Error::Library(format!("Original console runtime is incomplete: {}. Restore these files in {} from the console dump before enabling a source library.", missing.join(", "), game.display())));
    }
    let original = ConsoleVariant::from_directory(&game)?;
    if original != console {
        return Err(Error::Library(format!(
            "Console resources ({}) do not match library templates ({})",
            original.id(),
            console.id()
        )));
    }
    preserve_stock_templates(library, stock, &game, console)?;
    crate::identity::migrate_library(library).map_err(Error::Library)?;
    let published = library.join("published");
    let migration = if !enabled(library) && published.join("folders").is_dir() {
        crate::sync_library_from_published(&SyncOptions {
            library_root: library.to_path_buf(),
            published_root: published.clone(),
        })?
    } else {
        SyncReport::default()
    };
    crate::rom_store::migrate(library)?;
    checked_directory(&published)?;
    checked_directory(&published.join("roms"))?;
    checked_directory(&published.join("save"))?;
    prepare_bootstrap_rom(library)?;
    crate::publish_pipeline::prepare_native_working_save_source(library)?;
    let model = crate::library::load(library)?;
    crate::publish_pipeline::prepare_per_game_save_store(&model)?;
    crate::runtime_assets::prepare_library_runtime_assets(library, &game, Some(stock))?;
    let usb = crate::usb::prepare_source_runtime(library, console)?;
    // Both opt-ins are written last. Original folder PSBs/save archives stay
    // on disk for recovery; the source runtime no longer consumes them.
    crate::save_format::atomic_write(&published.join(".per-game-saves-v2"), b"2\n")
        .map_err(Error::Library)?;
    crate::save_format::atomic_write(&library.join(MARKER), b"2\n").map_err(Error::Library)?;
    Ok(SourceLibraryReport { usb, migration })
}

/// Metadata edits need no catalogue export. Maintain bitmap glyphs only when
/// the library uses the source runtime; legacy editing trees stay independent.
pub fn metadata_changed(library: &Path) -> Result<(), Error> {
    if !enabled(library) {
        return Ok(());
    }
    let game = game_root(library)
        .ok_or_else(|| Error::Library("Source library runtime is missing".into()))?;
    crate::runtime_assets::prepare_runtime_fonts(library, &game, Some(&library.join("templates")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_original_runtime_never_enables_source_mode_or_creates_catalogue_packs() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("library");
        fs::create_dir(&library).unwrap();
        assert!(!can_prepare(&library));
        assert!(prepare(&library, &library.join("templates")).is_err());
        assert!(!enabled(&library));
        assert!(!library.join("published/folders").exists());
    }
    #[test]
    fn source_marker_is_explicit_and_bare_editing_trees_do_not_acquire_a_runtime() {
        let root = tempfile::tempdir().unwrap();
        assert!(game_root(root.path()).is_none());
        assert!(!can_prepare(root.path()));
        assert!(!enabled(root.path()));
        fs::write(root.path().join(MARKER), b"1\n").unwrap();
        assert!(!enabled(root.path()));
        fs::write(root.path().join(MARKER), b"2\n").unwrap();
        assert!(enabled(root.path()));
        assert!(metadata_changed(root.path()).is_err());
    }
    #[test]
    fn compatibility_working_store_is_seeded_once_without_catalogues_or_save_overwrites() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("library");
        fs::create_dir(&library).unwrap();
        crate::publish_pipeline::prepare_native_working_save_source(&library).unwrap();
        let data = library.join("published/save/data_008_0000.bin");
        let meta = library.join("published/save/meta_008_0000.bin");
        assert_eq!(fs::metadata(&data).unwrap().len(), 1_291_396);
        crate::save_format::validate_data_digest(&data, &meta).unwrap();
        let original_meta = fs::read(&meta).unwrap();
        fs::write(&data, b"existing live settings and SRAM").unwrap();
        crate::publish_pipeline::prepare_native_working_save_source(&library).unwrap();
        assert_eq!(fs::read(&data).unwrap(), b"existing live settings and SRAM");
        assert_eq!(fs::read(&meta).unwrap(), original_meta);
        assert!(library.join("published/roms").is_dir());
        assert!(!library.join("published/folders").exists());
    }
    #[test]
    fn external_stock_templates_survive_reopening_and_are_never_replaced() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("library");
        let stock = root.path().join("original");
        let game = root.path().join("game");
        for relative in ConsoleVariant::Japan.templates() {
            let path = stock.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"original template").unwrap();
        }
        preserve_stock_templates(&library, &stock, &game, ConsoleVariant::Japan).unwrap();
        assert_eq!(
            ConsoleVariant::from_directory(&library.join("templates")).unwrap(),
            ConsoleVariant::Japan
        );
        for relative in ConsoleVariant::Japan.templates() {
            fs::write(stock.join(relative), b"different external resource").unwrap();
        }
        preserve_stock_templates(&library, &stock, &game, ConsoleVariant::Japan).unwrap();
        for relative in ConsoleVariant::Japan.templates() {
            assert_eq!(
                fs::read(library.join("templates").join(relative)).unwrap(),
                b"original template"
            );
        }
    }
    #[test]
    fn empty_menu_rom_has_safe_vectors_and_never_replaces_foreign_content() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("library");
        fs::create_dir(&library).unwrap();
        prepare_bootstrap_rom(&library).unwrap();
        let path = library.join("published/roms").join(BOOTSTRAP_ROM);
        let first = fs::read(&path).unwrap();
        let raw = m2_mzs::unpack_default(&first, BOOTSTRAP_ROM).unwrap();
        assert_eq!(raw.len(), 8192);
        assert_eq!(&raw[..4], &[0x78, 0xd8, 0x80, 0xfe]);
        for offset in (0x1ff6..=0x1ffe).step_by(2) {
            assert_eq!(&raw[offset..offset + 2], &[0x00, 0xe0]);
        }
        assert!(is_bootstrap_rom(&path));
        prepare_bootstrap_rom(&library).unwrap();
        assert_eq!(fs::read(&path).unwrap(), first);
        crate::rom_store::queue_cleanup(&library, BOOTSTRAP_ROM).unwrap();
        crate::rom_store::cleanup(&library).unwrap();
        assert_eq!(fs::read(&path).unwrap(), first);
        fs::write(&path, b"foreign user ROM").unwrap();
        assert!(!is_bootstrap_rom(&path));
        assert!(prepare_bootstrap_rom(&library).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"foreign user ROM");
    }
}
