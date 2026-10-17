use crate::{
    cd::extract_bios,
    covers::save_cover,
    report,
    source::{safe_relative, Source},
    BiosConfig, Reporter,
};
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::{fs, path::Path};

use m2_publish::console::{ConsoleVariant, SYSTEM_PROFILE};

#[derive(Debug, Serialize)]
pub struct NewLibraryResult {
    pub root: String,
    pub games: usize,
    pub bios: Option<BiosConfig>,
    pub warnings: Vec<String>,
    pub source_schema: u32,
}

fn psb(source: &Source, name: &str) -> Result<m2_psb::Value> {
    let bytes = source.read(name, 128 * 1024 * 1024)?;
    let bytes = if bytes.starts_with(b"mzs\0") {
        m2_mzs::unpack_default(
            &bytes,
            Path::new(name).file_name().unwrap().to_str().unwrap(),
        )?
    } else {
        bytes
    };
    Ok(m2_psb::read(&bytes)?)
}

/// Complete the older BACKUP/game first-run layout without exporting folder
/// packs. Existing runtime files and original templates are preserved.
pub fn prepare_existing_library_runtime(
    input: &Path,
    library: &Path,
    progress: Reporter<'_>,
) -> Result<m2_publish::UsbPreparation> {
    let game = m2_publish::source_library::game_root(library)
        .context("Choose a USB folder with a library subdirectory")?;
    let source = Source::open(input)?;
    let identity = source
        .read(SYSTEM_PROFILE, 1024 * 1024)
        .context("Original system profile missing from BACKUP/game")?;
    let console = ConsoleVariant::detect(Some(&identity), |name| source.size(name).is_some())?;
    let templates = console.templates();
    for name in templates.iter().copied().chain([
        "m2engage",
        "libopus.so.0",
        "version",
        "shutdown.png",
        "system/script/init.nut.m",
        SYSTEM_PROFILE,
    ]) {
        ensure!(source.size(name).is_some(), "BACKUP/game is missing {name}");
    }
    let files: Vec<_> = source
        .names()
        .filter(|name| {
            !name.starts_with("system/roms/")
                && !name.starts_with("save/")
                && !name.starts_with("alldata.")
                && !name.starts_with("lost+found/")
        })
        .cloned()
        .collect();
    for (index, name) in files.iter().enumerate() {
        report(
            progress,
            format!("Preparing console resources: {name}"),
            Some((index as u64, files.len() as u64)),
        );
        copy_missing_resource(&source, name, &game)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(game.join("m2engage"), fs::Permissions::from_mode(0o755))?;
    }
    for name in templates
        .iter()
        .copied()
        .chain(m2_publish::fonts::RESOURCES.iter().copied())
        .chain([SYSTEM_PROFILE])
    {
        copy_missing_resource(&source, name, &library.join("templates"))?;
    }
    report(progress, "Preparing the console runtime…", None);
    Ok(m2_publish::source_library::prepare(library, &library.join("templates"))?.usb)
}

fn copy_missing_resource(source: &Source, name: &str, root: &Path) -> Result<()> {
    let relative = safe_relative(name)?;
    let mut directory = root.to_path_buf();
    for component in std::iter::once(None).chain(relative.parent().unwrap().components().map(Some))
    {
        if let Some(component) = component {
            directory.push(component);
        }
        match fs::symlink_metadata(&directory) {
            Ok(meta) => ensure!(
                meta.file_type().is_dir(),
                "Expected a regular directory: {}",
                directory.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir_all(&directory)?
            }
            Err(error) => return Err(error.into()),
        }
    }
    let destination = root.join(relative);
    match fs::symlink_metadata(&destination) {
        Ok(meta) => ensure!(
            meta.file_type().is_file(),
            "Expected a regular resource: {}",
            destination.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut temporary = tempfile::NamedTempFile::new_in(destination.parent().unwrap())?;
            let copied = std::io::copy(&mut source.reader(name)?, &mut temporary)?;
            ensure!(
                source.size(name) == Some(copied),
                "Incomplete original console resource: {name}"
            );
            temporary.as_file().sync_all()?;
            temporary
                .persist_noclobber(&destination)
                .map_err(|error| error.error)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

const LIBRARY_ENTRIES: [&str; 3] = ["game", "library", "source.json"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LibraryDestination {
    NewDirectory,
    ExistingDirectory,
}

fn require_absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => anyhow::bail!(
            "Destination already contains {}; existing files were kept. Choose a folder without game, library or source.json.",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Cannot inspect {}", path.display())),
    }
}

fn library_destination(destination: &Path) -> Result<LibraryDestination> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) => {
            ensure!(
                metadata.file_type().is_dir(),
                "Choose a regular destination directory: {}",
                destination.display()
            );
            for name in LIBRARY_ENTRIES {
                require_absent(&destination.join(name))?;
            }
            Ok(LibraryDestination::ExistingDirectory)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = destination
                .parent()
                .context("Choose a destination directory")?;
            ensure!(parent.is_dir(), "Destination parent does not exist");
            Ok(LibraryDestination::NewDirectory)
        }
        Err(error) => {
            Err(error).with_context(|| format!("Cannot inspect {}", destination.display()))
        }
    }
}

// Unlike fs::rename, these operations cannot replace a file or even an empty
// directory created between our validation and the actual rename.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn rename_without_replacement(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let source = CString::new(source.as_os_str().as_bytes())?;
    let destination = CString::new(destination.as_os_str().as_bytes())?;
    #[cfg(target_os = "macos")]
    let result = {
        extern "C" {
            fn renamex_np(
                from: *const std::ffi::c_char,
                to: *const std::ffi::c_char,
                flags: u32,
            ) -> i32;
        }
        // RENAME_EXCL, available on supported macOS releases.
        unsafe { renamex_np(source.as_ptr(), destination.as_ptr(), 0x0000_0004) }
    };
    #[cfg(target_os = "linux")]
    let result = {
        extern "C" {
            fn renameat2(
                from_dir: i32,
                from: *const std::ffi::c_char,
                to_dir: i32,
                to: *const std::ffi::c_char,
                flags: u32,
            ) -> i32;
        }
        // AT_FDCWD and RENAME_NOREPLACE. Unsupported filesystems fail closed.
        unsafe { renameat2(-100, source.as_ptr(), -100, destination.as_ptr(), 1) }
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "windows")]
fn rename_without_replacement(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let wide = |path: &Path| -> std::io::Result<Vec<u16>> {
        let mut encoded: Vec<u16> = path.as_os_str().encode_wide().collect();
        if encoded.contains(&0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Path contains NUL",
            ));
        }
        encoded.push(0);
        Ok(encoded)
    };
    // Match std's support for long paths (canonicalize supplies the extended
    // Windows prefix), while the not-yet-existing leaf remains untouched.
    let source = wide(&fs::canonicalize(source)?)?;
    let parent = destination.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Destination has no parent",
        )
    })?;
    let name = destination.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Destination has no filename",
        )
    })?;
    let destination = wide(&fs::canonicalize(parent)?.join(name))?;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    // No MOVEFILE_REPLACE_EXISTING or cross-volume copy fallback.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) } != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn rename_without_replacement(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Safe library installation is unavailable on this platform",
    ))
}

#[derive(Debug)]
struct IncompleteLibraryRollback(String);

impl std::fmt::Display for IncompleteLibraryRollback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for IncompleteLibraryRollback {}

fn finish_staged_library(
    staged: &Path,
    destination: &Path,
    expected: LibraryDestination,
) -> Result<()> {
    finish_staged_library_with(staged, destination, expected, rename_without_replacement)
}

fn finish_staged_library_with(
    staged: &Path,
    destination: &Path,
    expected: LibraryDestination,
    mut move_entry: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> Result<()> {
    for name in LIBRARY_ENTRIES {
        let path = staged.join(name);
        let kind = fs::symlink_metadata(&path)?.file_type();
        ensure!(
            if name == "source.json" {
                kind.is_file()
            } else {
                kind.is_dir()
            },
            "Incomplete staged library: {}",
            path.display()
        );
    }
    ensure!(
        library_destination(destination)? == expected,
        "Destination changed while preparing the library; existing files were kept"
    );
    if expected == LibraryDestination::NewDirectory {
        return move_entry(staged, destination)
            .context("Cannot finish creating the library; existing files were kept");
    }
    let mut moved = Vec::new();
    let result: Result<()> = (|| {
        for name in LIBRARY_ENTRIES {
            let target = destination.join(name);
            require_absent(&target)?;
            move_entry(&staged.join(name), &target)
                .with_context(|| format!("Cannot install {}", target.display()))?;
            moved.push(name);
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut failures = Vec::new();
        for name in moved.into_iter().rev() {
            if let Err(rollback) = move_entry(&destination.join(name), &staged.join(name)) {
                failures.push(format!("{name}: {rollback}"));
            }
        }
        if !failures.is_empty() {
            return Err(IncompleteLibraryRollback(format!(
                "{error:#}; rollback could not finish ({}). Remaining prepared files are retained at {}. Do not remove this recovery directory.",
                failures.join("; "), staged.display()
            )).into());
        }
        return Err(error.context("Library installation rolled back; existing files were kept"));
    }
    Ok(())
}

pub fn create_library(
    input: &Path,
    destination: &Path,
    include_games: bool,
    bios_cache: &Path,
    progress: Reporter<'_>,
) -> Result<NewLibraryResult> {
    let destination_kind = library_destination(destination)?;
    let staging_parent = if destination_kind == LibraryDestination::ExistingDirectory {
        destination
    } else {
        destination
            .parent()
            .context("Choose a destination directory")?
    };
    report(progress, "Reading console dump…", None);
    let source = Source::open(input)?;
    let identity = source
        .read(SYSTEM_PROFILE, 1024 * 1024)
        .context("Original system profile missing from the console dump")?;
    let console = ConsoleVariant::detect(Some(&identity), |name| source.size(name).is_some())?;
    let templates = console.templates();
    // Check the full set up front, before a destination is visible.
    for name in templates.iter().copied().chain([
        "m2engage",
        "libopus.so.0",
        "version",
        "shutdown.png",
        "system/script/init.nut.m",
        "system/config/system_prof.psb.m",
    ]) {
        ensure!(
            source.size(name).is_some(),
            "The dump is missing {name}. Select the original P9, not only alldata.bin."
        );
    }
    let temporary = tempfile::Builder::new()
        .prefix(".chronos-library-")
        .tempdir_in(staging_parent)?;
    let root = temporary.path();
    let game = root.join("game");
    let library = root.join("library");
    let files = source
        .names()
        .filter(|n| {
            !n.starts_with("system/roms/")
                && !n.starts_with("save/")
                && !n.starts_with("alldata.")
                && !n.starts_with("lost+found/")
        })
        .cloned()
        .collect::<Vec<_>>();
    for (i, name) in files.iter().enumerate() {
        report(
            progress,
            format!("Preparing menu resources: {name}"),
            Some((i as u64, files.len() as u64)),
        );
        source.copy(name, &game.join(safe_relative(name)?))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(game.join("m2engage"), fs::Permissions::from_mode(0o755))?;
    }
    for path in [
        game.join("save"),
        game.join("system/roms"),
        library.join("published"),
    ] {
        fs::create_dir_all(path)?;
    }
    for name in templates
        .iter()
        .copied()
        .chain(m2_publish::fonts::RESOURCES.iter().copied())
        .chain([SYSTEM_PROFILE])
    {
        source.copy(name, &library.join("templates").join(name))?;
    }
    let mut warnings = Vec::new();
    let mut bios = None;
    report(progress, "Extracting BIOS from an original CD game…", None);
    for name in source
        .names()
        .filter(|n| n.starts_with("system/roms/") && n.to_ascii_lowercase().ends_with(".pcd"))
    {
        match extract_bios(
            source.reader(name)?,
            &format!("{}: {name}", input.display()),
            bios_cache,
        ) {
            Ok(found) => {
                bios = Some(found);
                break;
            }
            Err(e) => {
                warnings.push(format!("{name}: {e:#}"));
            }
        }
    }
    if bios.is_none() {
        warnings.push(
            "No compatible BIOS pair found. Configure BIOS in Settings before importing a CUE."
                .into(),
        );
    }
    let mut games = 0;
    if include_games {
        games = extract_games(&source, &library, &templates, progress)?;
    } else {
        for lineup in ["jp", "us"] {
            fs::create_dir_all(library.join(lineup))?;
            fs::write(library.join(lineup).join("gamelist.json"), "[]\n")?;
        }
    }
    report(progress, "Preparing the console runtime…", None);
    let runtime = m2_publish::source_library::prepare(&library, &library.join("templates"))?;
    warnings.extend(runtime.usb.missing_original_files);
    fs::write(
        root.join("source.json"),
        serde_json::to_vec_pretty(
            &json!({"source":input,"console_package":console.id(),"include_original_games":include_games,"games":games,"warnings":warnings}),
        )?,
    )?;
    if let Err(error) = finish_staged_library(root, destination, destination_kind) {
        if error.is::<IncompleteLibraryRollback>() {
            // Never let TempDir delete the remaining half of a recovery set.
            let _ = temporary.keep();
        }
        return Err(error);
    }
    report(progress, "Library created", Some((1, 1)));
    Ok(NewLibraryResult {
        root: destination.to_string_lossy().into(),
        games,
        bios,
        warnings,
        source_schema: 2,
    })
}

fn extract_games(
    source: &Source,
    library: &Path,
    templates: &[&str; 4],
    progress: Reporter<'_>,
) -> Result<usize> {
    let menu = psb(source, templates[1])?.to_json();
    let items = menu
        .get("items")
        .or_else(|| menu.get("root").and_then(|r| r.get("items")))
        .and_then(Value::as_array)
        .context("Original menu has no game list")?;
    ensure!(
        items.len() >= 200,
        "Unsupported original menu: expected the JP/US lineups"
    );
    let profiles = psb(source, templates[0])?.to_json();
    let versions = profiles
        .pointer("/root/m2epi/version")
        .and_then(Value::as_object)
        .context("Original ROM profiles not found")?;
    let mut count = 0;
    for (lineup, start, motion) in [("jp", 0, templates[2]), ("us", 50, templates[3])] {
        let covers = psb(source, motion)?;
        let dir = library.join(lineup);
        fs::create_dir_all(&dir)?;
        let mut list = Vec::new();
        for (slot, item) in items.iter().enumerate().skip(start).take(50) {
            let tag = item["regionTag"]
                .as_str()
                .context("Game has no regionTag")?;
            if tag == "DUMMY" {
                continue;
            }
            ensure!(
                safe_relative(tag)?.components().count() == 1 && !tag.starts_with("FOLDER"),
                "Invalid game identifier: {tag}"
            );
            report(
                progress,
                format!("Importing original game: {tag}"),
                Some(((slot - start) as u64, 50)),
            );
            let profile = versions
                .get(tag)
                .with_context(|| format!("No ROM profile for {tag}"))?;
            let original = profile["rom"]
                .as_str()
                .context("Game has no ROM filename")?;
            let filename = Path::new(original)
                .file_name()
                .and_then(|n| n.to_str())
                .context("Invalid ROM filename")?;
            safe_relative(filename)?;
            let dest = dir.join(tag);
            fs::create_dir(&dest)?;
            let rom_name = format!("system/roms/{filename}");
            let pool = m2_publish::rom_store::checked_pool(library)?;
            let console_name = m2_publish::rom_store::console_name(filename)?;
            if !pool.join(&console_name).exists() {
                if source.size(&rom_name).is_some() {
                    if console_name == filename {
                        source.copy(&rom_name, &pool.join(filename))?;
                    } else {
                        let bytes = source.read(&rom_name, 32 * 1024 * 1024)?;
                        fs::write(
                            pool.join(&console_name),
                            m2_mzs::pack_default(&bytes, &console_name)?,
                        )?;
                    }
                } else {
                    // Keep the original encrypted HuCard archive unchanged.
                    source.copy(&format!("{rom_name}.m"), &pool.join(&console_name))?;
                }
            }
            let (width, height) = save_cover(
                &covers,
                item["image"].as_u64().unwrap_or(slot as u64),
                &dest.join("cover.png"),
            )
            .with_context(|| format!("Extract cover for {tag}"))?;
            let english = items
                .iter()
                .skip(start + 100)
                .take(50)
                .find(|i| i["regionTag"].as_str() == Some(tag))
                .unwrap_or(item);
            let mut display = serde_json::Map::new();
            for key in [
                "name",
                "tname",
                "titlebar",
                "ccolor",
                "csize",
                "players",
                "demo_time",
                "action",
                "copy",
                "desc",
            ] {
                display.insert(
                    key.into(),
                    item.get(key).cloned().unwrap_or_else(|| {
                        if ["name", "tname", "copy", "desc"].contains(&key) {
                            json!("")
                        } else if key == "action" {
                            json!("Boot")
                        } else {
                            json!(0)
                        }
                    }),
                );
            }
            display.insert(
                "name_eng".into(),
                english.get("tname").cloned().unwrap_or_else(|| json!("")),
            );
            let mut rom = profile.as_object().context("Invalid ROM profile")?.clone();
            rom.insert("rom".into(), json!(filename));
            rom.insert("regionTag".into(), json!(tag));
            let mut data = json!({"display":display,"rom":rom,"cover":"cover.png","emulator":{
                "EmuOfsX":item.get("EmuOfsX").cloned().unwrap_or(json!(0)),
                "EmuOfsY":item.get("EmuOfsY").cloned().unwrap_or(json!(0)),
                "ScreenMode":item.get("ScreenMode").cloned().unwrap_or(json!(0))}});
            if (width, height) != (240, 240) {
                data["cover_size"] =
                    json!({"width":width,"height":height,"originX":width/2,"originY":height/2});
            }
            fs::write(dest.join("game.json"), serde_json::to_vec_pretty(&data)?)?;
            let mut entry = json!({"folder":tag});
            for key in ["sor_date", "sor_demo", "sor_genr", "sor_name", "sor_pnum"] {
                entry[key] = item.get(key).cloned().unwrap_or(json!(0));
            }
            list.push(entry);
            count += 1;
        }
        fs::write(dir.join("gamelist.json"), serde_json::to_vec_pretty(&list)?)?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged_library(root: &Path) {
        fs::create_dir_all(root.join("game")).unwrap();
        fs::create_dir_all(root.join("library/jp")).unwrap();
        fs::write(root.join("game/m2engage"), b"prepared engine").unwrap();
        fs::write(root.join("library/jp/gamelist.json"), b"[]\n").unwrap();
        fs::write(root.join("source.json"), b"{\"games\":0}\n").unwrap();
    }

    fn assert_staged_library(root: &Path) {
        assert_eq!(
            fs::read(root.join("game/m2engage")).unwrap(),
            b"prepared engine"
        );
        assert_eq!(
            fs::read(root.join("library/jp/gamelist.json")).unwrap(),
            b"[]\n"
        );
        assert_eq!(
            fs::read(root.join("source.json")).unwrap(),
            b"{\"games\":0}\n"
        );
    }

    fn payload_entry_count(path: &Path) -> usize {
        // macOS may create AppleDouble metadata companions on FAT32.
        fs::read_dir(path)
            .unwrap()
            .filter(|entry| {
                !entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("._")
            })
            .count()
    }

    #[test]
    fn existing_destination_installs_directly_and_preserves_unrelated_files() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("personal.txt"), b"personal data").unwrap();
        fs::write(root.path().join(".hidden"), b"hidden data").unwrap();
        let staging = tempfile::tempdir_in(root.path()).unwrap();
        staged_library(staging.path());
        let mode = library_destination(root.path()).unwrap();
        assert_eq!(mode, LibraryDestination::ExistingDirectory);
        finish_staged_library(staging.path(), root.path(), mode).unwrap();
        drop(staging);
        assert_staged_library(root.path());
        assert_eq!(
            fs::read(root.path().join("personal.txt")).unwrap(),
            b"personal data"
        );
        assert_eq!(
            fs::read(root.path().join(".hidden")).unwrap(),
            b"hidden data"
        );
        assert!(!root.path().join("Chronos").exists());
        assert_eq!(payload_entry_count(root.path()), 5);
    }

    #[test]
    fn new_destination_keeps_the_whole_directory_commit() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("new-library");
        let staging = tempfile::tempdir_in(root.path()).unwrap();
        staged_library(staging.path());
        let mode = library_destination(&destination).unwrap();
        assert_eq!(mode, LibraryDestination::NewDirectory);
        finish_staged_library(staging.path(), &destination, mode).unwrap();
        assert!(!staging.path().exists());
        assert_staged_library(&destination);
    }

    #[test]
    fn existing_reserved_names_refuse_commit_without_touching_files() {
        for name in LIBRARY_ENTRIES {
            for directory in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let destination = root.path().join("usb");
                fs::create_dir(&destination).unwrap();
                let conflict = destination.join(name);
                if directory {
                    fs::create_dir(&conflict).unwrap();
                } else {
                    fs::write(&conflict, b"existing user data").unwrap();
                }
                let staging = tempfile::tempdir_in(root.path()).unwrap();
                staged_library(staging.path());
                assert!(library_destination(&destination).is_err());
                assert!(finish_staged_library(
                    staging.path(),
                    &destination,
                    LibraryDestination::ExistingDirectory
                )
                .is_err());
                assert_staged_library(staging.path());
                assert_eq!(payload_entry_count(&destination), 1);
                if directory {
                    assert!(conflict.is_dir());
                } else {
                    assert_eq!(fs::read(&conflict).unwrap(), b"existing user data");
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn destination_and_reserved_dangling_symlinks_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("usb");
        fs::create_dir(&destination).unwrap();
        for name in LIBRARY_ENTRIES {
            let conflict = destination.join(name);
            std::os::unix::fs::symlink(root.path().join("missing"), &conflict).unwrap();
            assert!(!conflict.exists());
            assert!(library_destination(&destination).is_err());
            assert!(fs::symlink_metadata(&conflict)
                .unwrap()
                .file_type()
                .is_symlink());
            fs::remove_file(conflict).unwrap();
        }
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&destination, &alias).unwrap();
        assert!(library_destination(&alias).is_err());
        let broken = root.path().join("broken");
        std::os::unix::fs::symlink(root.path().join("missing"), &broken).unwrap();
        assert!(library_destination(&broken).is_err());
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
    }

    #[test]
    fn late_conflict_cannot_overwrite_an_empty_directory_and_rolls_back() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("usb");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("personal.txt"), b"keep").unwrap();
        let staging = tempfile::tempdir_in(&destination).unwrap();
        staged_library(staging.path());
        let result = finish_staged_library_with(
            staging.path(),
            &destination,
            LibraryDestination::ExistingDirectory,
            |from, to| {
                if from == staging.path().join("library") {
                    // Appears after require_absent: plain fs::rename would replace
                    // this other operation's empty directory on Unix.
                    fs::create_dir(to)?;
                }
                rename_without_replacement(from, to)
            },
        );
        assert!(result.is_err());
        assert!(!destination.join("game").exists());
        assert!(!destination.join("source.json").exists());
        assert!(destination.join("library").is_dir());
        assert_eq!(
            fs::read_dir(destination.join("library")).unwrap().count(),
            0
        );
        assert_eq!(fs::read(destination.join("personal.txt")).unwrap(), b"keep");
        assert_staged_library(staging.path());
    }

    #[test]
    fn failed_final_rename_restores_every_previously_installed_entry() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("usb");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("personal.txt"), b"keep").unwrap();
        let staging = tempfile::tempdir_in(&destination).unwrap();
        staged_library(staging.path());
        let error = finish_staged_library_with(
            staging.path(),
            &destination,
            LibraryDestination::ExistingDirectory,
            |from, to| {
                if from == staging.path().join("source.json") {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "injected final rename failure",
                    ));
                }
                rename_without_replacement(from, to)
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("rolled back"));
        for name in LIBRARY_ENTRIES {
            assert!(!destination.join(name).exists());
        }
        assert_eq!(fs::read(destination.join("personal.txt")).unwrap(), b"keep");
        assert_staged_library(staging.path());
    }

    #[test]
    fn incomplete_rollback_requests_retention_of_recovery_files() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("usb");
        fs::create_dir(&destination).unwrap();
        let staging = tempfile::tempdir_in(&destination).unwrap();
        staged_library(staging.path());
        let error = finish_staged_library_with(
            staging.path(),
            &destination,
            LibraryDestination::ExistingDirectory,
            |from, to| {
                if from == staging.path().join("source.json") || from == destination.join("library")
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "injected storage failure",
                    ));
                }
                rename_without_replacement(from, to)
            },
        )
        .unwrap_err();
        assert!(error.is::<IncompleteLibraryRollback>());
        assert!(error
            .to_string()
            .contains(&staging.path().display().to_string()));
        assert!(staging.path().join("game/m2engage").is_file());
        assert!(staging.path().join("source.json").is_file());
        assert!(destination.join("library/jp/gamelist.json").is_file());
    }

    #[test]
    fn a_new_destination_created_at_rename_time_is_not_replaced() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("new-library");
        let staging = tempfile::tempdir_in(root.path()).unwrap();
        staged_library(staging.path());
        let result = finish_staged_library_with(
            staging.path(),
            &destination,
            LibraryDestination::NewDirectory,
            |from, to| {
                fs::create_dir(to)?;
                rename_without_replacement(from, to)
            },
        );
        assert!(result.is_err());
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        assert_staged_library(staging.path());
    }

    #[test]
    fn incomplete_staging_never_exposes_a_partial_library() {
        let root = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir_in(root.path()).unwrap();
        staged_library(staging.path());
        fs::remove_file(staging.path().join("source.json")).unwrap();
        assert!(finish_staged_library(
            staging.path(),
            root.path(),
            LibraryDestination::ExistingDirectory
        )
        .is_err());
        for name in LIBRARY_ENTRIES {
            assert!(!root.path().join(name).exists());
        }
    }

    #[test]
    fn first_run_resource_copy_is_atomic_and_preserves_existing_files() {
        let root = tempfile::tempdir().unwrap();
        let backup = root.path().join("backup");
        fs::create_dir_all(backup.join("system/script")).unwrap();
        fs::write(backup.join("m2engage"), b"original engine").unwrap();
        fs::write(backup.join("system/script/init.nut.m"), b"original script").unwrap();
        fs::write(backup.join("version"), b"original version").unwrap();
        let source = Source::open(&backup).unwrap();
        let game = root.path().join("game");
        copy_missing_resource(&source, "system/script/init.nut.m", &game).unwrap();
        assert_eq!(
            fs::read(game.join("system/script/init.nut.m")).unwrap(),
            b"original script"
        );
        fs::write(backup.join("system/script/init.nut.m"), b"another script").unwrap();
        copy_missing_resource(&source, "system/script/init.nut.m", &game).unwrap();
        assert_eq!(
            fs::read(game.join("system/script/init.nut.m")).unwrap(),
            b"original script"
        );
        assert_eq!(fs::read_dir(game.join("system/script")).unwrap().count(), 1);
        fs::write(backup.join("version"), b"short").unwrap();
        assert!(copy_missing_resource(&source, "version", &game).is_err());
        assert!(!game.join("version").exists());
    }

    #[test]
    fn incomplete_first_run_dump_is_rejected_before_creating_runtime_resources() {
        let root = tempfile::tempdir().unwrap();
        let backup = root.path().join("backup");
        fs::create_dir(&backup).unwrap();
        fs::write(backup.join("m2engage"), b"original engine").unwrap();
        let library = root.path().join("library");
        assert!(prepare_existing_library_runtime(&backup, &library, &|_| {}).is_err());
        assert!(!library.exists());
        assert!(!root.path().join("game").exists());
    }

    #[cfg(unix)]
    #[test]
    fn first_run_resource_copy_refuses_linked_destination_directories() {
        let root = tempfile::tempdir().unwrap();
        let backup = root.path().join("backup");
        let game = root.path().join("game");
        let external = root.path().join("external");
        fs::create_dir_all(backup.join("system/script")).unwrap();
        fs::write(backup.join("m2engage"), b"engine").unwrap();
        fs::write(backup.join("system/script/init.nut.m"), b"script").unwrap();
        fs::create_dir(&game).unwrap();
        fs::create_dir(&external).unwrap();
        std::os::unix::fs::symlink(&external, game.join("system")).unwrap();
        let source = Source::open(&backup).unwrap();
        assert!(copy_missing_resource(&source, "system/script/init.nut.m", &game).is_err());
        assert_eq!(fs::read_dir(&external).unwrap().count(), 0);
    }
}
