//! Commit a catalogue deletion before removing any game files. Interrupted
//! cleanup can leave unused files, but never an active row with a missing game.
use crate::{DualLibrary, GameListEntry, Library};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

const SORT_FIELDS: [&str; 5] = ["sor_date", "sor_demo", "sor_genr", "sor_name", "sor_pnum"];
const DELETION_MARKER: &str = ".chronos-delete-pending";
const MARKER_VERSION: &[u8] = b"1\n";

#[derive(Debug, Serialize)]
pub struct DeleteResult {
    pub library: DualLibrary,
    pub source: Library,
    pub warning: Option<String>,
}

fn basename(name: &str) -> Result<(), String> {
    let mut parts = Path::new(name).components();
    if name.contains(['/', '\\', ':'])
        || !matches!(parts.next(), Some(Component::Normal(_)))
        || parts.next().is_some()
    {
        return Err(format!("Invalid library directory name: {name}"));
    }
    Ok(())
}

fn directory(path: &Path) -> Result<(), String> {
    if !fs::symlink_metadata(path)
        .map_err(|e| format!("Inspect {}: {e}", path.display()))?
        .file_type()
        .is_dir()
    {
        return Err(format!("Expected a regular directory: {}", path.display()));
    }
    Ok(())
}

fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    if !fs::symlink_metadata(path)
        .map_err(|e| format!("Inspect {}: {e}", path.display()))?
        .file_type()
        .is_file()
    {
        return Err(format!("Expected a regular file: {}", path.display()));
    }
    fs::read(path).map_err(|e| format!("Read {}: {e}", path.display()))
}

/// A committed deletion may leave files behind if Windows refuses a rename or
/// the application stops during cleanup. The marker follows the directory and
/// is active only after its parent catalogue stops listing it. Before commit,
/// including after an interrupted preflight, the entry remains editable.
pub(crate) fn ensure_not_deleted(directory: &Path) -> Result<(), String> {
    for entry in directory.ancestors() {
        let marker = entry.join(DELETION_MARKER);
        match fs::symlink_metadata(&marker) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Inspect deletion marker {}: {error}",
                    marker.display()
                ))
            }
            Ok(_) => {
                if read_file(&marker)? != MARKER_VERSION {
                    return Err(format!(
                        "Invalid deletion marker in {}; reopen the library before saving",
                        entry.display()
                    ));
                }
                let parent = entry
                    .parent()
                    .ok_or("Deleted entry has no parent catalogue")?;
                let rows: Vec<Value> =
                    serde_json::from_slice(&read_file(&parent.join("gamelist.json"))?)
                        .map_err(|e| format!("Invalid parent catalogue: {e}"))?;
                let name = entry
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or("Invalid deleted entry name")?;
                if !rows.iter().any(|row| row["folder"].as_str() == Some(name)) {
                    return Err(format!("Library entry {} was deleted. Reopen the library before saving; deleted files will not be restored.", entry.display()));
                }
            }
        }
        if matches!(
            entry.file_name().and_then(|s| s.to_str()),
            Some("jp" | "us")
        ) {
            break;
        }
    }
    Ok(())
}

fn container(root: &Path, name: &str) -> Result<PathBuf, String> {
    let parts: Vec<_> = name.split('/').collect();
    if !(1..=2).contains(&parts.len()) || !matches!(parts[0], "jp" | "us") {
        return Err(format!("Invalid library lineup or folder: {name}"));
    }
    let mut path = root.to_path_buf();
    for part in parts {
        basename(part)?;
        path.push(part);
        directory(&path)?;
    }
    if path.join("game.json").exists() {
        return Err("A game cannot contain another game".into());
    }
    Ok(path)
}

/// Consider only primary ROM selections from the entry explicitly deleted.
/// Other metadata, BIOS files and unreferenced pool contents are never swept.
fn primary_roms(path: &Path, out: &mut BTreeSet<String>) -> Result<(), String> {
    for entry in fs::read_dir(path).map_err(|e| format!("Read {}: {e}", path.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            return Err(format!("Linked library entry: {}", entry.path().display()));
        }
        if kind.is_dir() {
            primary_roms(&entry.path(), out)?;
        } else if entry.file_name() == "game.json" {
            let game: Value = serde_json::from_slice(&read_file(&entry.path())?)
                .map_err(|e| format!("Invalid {}: {e}", entry.path().display()))?;
            if let Some(name) = game["rom"]["rom"].as_str().filter(|s| !s.is_empty()) {
                out.insert(m2_publish::rom_store::console_name(name).map_err(|e| e.to_string())?);
            }
        }
    }
    Ok(())
}

pub fn perform_delete(
    root: &Path,
    lineup: &str,
    name: &str,
    is_folder: bool,
    remaining_sort: &[GameListEntry],
) -> Result<DeleteResult, String> {
    perform_with(
        root,
        lineup,
        name,
        is_folder,
        remaining_sort,
        &m2_publish::save_format::atomic_write,
        &|root| m2_publish::source_library::metadata_changed(root).map_err(|e| e.to_string()),
    )
}

fn perform_with(
    root: &Path,
    lineup: &str,
    name: &str,
    is_folder: bool,
    remaining_sort: &[GameListEntry],
    write: &impl Fn(&Path, &[u8]) -> Result<(), String>,
    refresh: &impl Fn(&Path) -> Result<(), String>,
) -> Result<DeleteResult, String> {
    perform_with_io(
        root,
        lineup,
        name,
        is_folder,
        remaining_sort,
        write,
        &m2_publish::save_format::atomic_write,
        refresh,
    )
}

fn perform_with_io(
    root: &Path,
    lineup: &str,
    name: &str,
    is_folder: bool,
    remaining_sort: &[GameListEntry],
    write: &impl Fn(&Path, &[u8]) -> Result<(), String>,
    write_marker: &impl Fn(&Path, &[u8]) -> Result<(), String>,
    refresh: &impl Fn(&Path) -> Result<(), String>,
) -> Result<DeleteResult, String> {
    let _cover_update = crate::import_commands::lock_cover_update()?;
    directory(root)?;
    basename(name)?;
    let base = container(root, lineup)?;
    if is_folder != name.starts_with("FOLDER") || (is_folder && lineup.contains('/')) {
        return Err("The selected entry is not the expected game or folder".into());
    }
    let target = base.join(name);
    directory(&target)?;
    if !is_folder {
        let game: Value = serde_json::from_slice(&read_file(&target.join("game.json"))?)
            .map_err(|e| format!("Invalid game.json: {e}"))?;
        if !game["rom"].is_object() || game["rom"]["arch"] == "folder" {
            return Err("The selected entry is not a game".into());
        }
    }

    let index_path = base.join("gamelist.json");
    let original_index = read_file(&index_path)?;
    let rows: Vec<Value> = serde_json::from_slice(&original_index)
        .map_err(|e| format!("Invalid {}: {e}", index_path.display()))?;
    let mut remaining = BTreeMap::new();
    for row in rows {
        let sort: GameListEntry = serde_json::from_value(row.clone())
            .map_err(|e| format!("Invalid game list entry: {e}"))?;
        basename(&sort.folder)?;
        directory(&base.join(&sort.folder))?;
        if remaining.insert(sort.folder, row).is_some() {
            return Err("The game list contains a duplicate entry".into());
        }
    }
    if remaining.remove(name).is_none() || remaining.len() != remaining_sort.len() {
        return Err("The game list changed; reopen the library before deleting this entry".into());
    }
    let new_rows = remaining_sort
        .iter()
        .map(|sort| {
            let mut row = remaining.remove(&sort.folder).ok_or_else(|| {
                "The game list changed; reopen the library before deleting this entry".to_string()
            })?;
            let values = serde_json::to_value(sort).map_err(|e| e.to_string())?;
            for field in SORT_FIELDS {
                row[field] = values[field].clone();
            }
            Ok(row)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut new_index = serde_json::to_vec_pretty(&new_rows).map_err(|e| e.to_string())?;
    new_index.push(b'\n');
    let mut roms = BTreeSet::new();
    primary_roms(&target, &mut roms)?;

    // Persist before committing the index, so even a restart immediately after
    // commit cannot let an old window restore a directory left by failed cleanup.
    let marker = target.join(DELETION_MARKER);
    let previous_marker = match fs::symlink_metadata(&marker) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Inspect deletion marker: {error}")),
        Ok(_) => {
            let bytes = read_file(&marker)?;
            if bytes != MARKER_VERSION {
                return Err(format!(
                    "Invalid deletion marker in {}; game files were kept",
                    target.display()
                ));
            }
            Some(bytes)
        }
    };
    write_marker(&marker, MARKER_VERSION).map_err(|e| {
        format!("Cannot prepare deletion marker; game and save data were kept: {e}")
    })?;

    // Commit first. A crash before this point leaves the directory and saves
    // untouched; a crash afterwards can only leave unreferenced game files.
    if let Err(error) = write(&index_path, &new_index) {
        // An atomic writer may fail after replacement, so restore its original
        // bytes instead of assuming the list is intact. No game file moved.
        let restored = m2_publish::save_format::atomic_write(&index_path, &original_index);
        if previous_marker.is_none() && restored.is_ok() {
            // Harmless if removal fails: the restored row keeps this marker
            // inactive, and a later successful delete can reuse it.
            let _ = fs::remove_file(&marker);
        }
        return match restored {
            Ok(()) => Err(format!(
                "Deletion rolled back; game files and saves were kept: {error}"
            )),
            Err(recovery) => Err(format!("Deletion interrupted: {error}. The game list could not be restored: {recovery}. Game files and saves remain unchanged in {}.", target.display())),
        };
    }

    // The catalogue is committed. Derived cleanup failures are warnings and
    // must not invite a second deletion against an already updated library.
    let mut warnings = Vec::new();
    let remove_files = (|| {
        // Same filesystem, outside jp/us: reference scans cannot mistake a
        // partially removed directory for a second live game.
        let quarantine = tempfile::Builder::new()
            .prefix(".chronos-delete-")
            .tempdir_in(root)
            .map_err(|e| format!("Deleted game files remain in {}: {e}", target.display()))?
            .keep();
        if let Err(e) = fs::rename(&target, quarantine.join(name)) {
            let _ = fs::remove_dir(&quarantine);
            return Err(format!(
                "Deleted game files remain in {}: {e}",
                target.display()
            ));
        }
        fs::remove_dir_all(&quarantine)
            .map_err(|e| format!("Some deleted files remain in {}: {e}", quarantine.display()))
    })();
    if let Err(e) = remove_files {
        warnings.push(e);
    }
    for name in roms {
        if let Err(e) = m2_publish::rom_store::queue_cleanup(root, &name) {
            warnings.push(format!("ROM cleanup could not be queued for {name}: {e}"));
        }
    }
    if let Err(e) = m2_publish::rom_store::cleanup(root) {
        warnings.push(format!("Unused ROMs could not be removed: {e}"));
    }
    if let Err(e) = refresh(root) {
        warnings.push(format!("Console fonts could not be updated: {e}"));
    }
    Ok(DeleteResult {
        library: DualLibrary {
            jp: crate::load_lineup(&root.join("jp")),
            us: crate::load_lineup(&root.join("us")),
            path: root.to_string_lossy().into_owned(),
            source_schema: if m2_publish::source_library::enabled(root) {
                2
            } else {
                1
            },
        },
        source: crate::load_lineup(&base),
        warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    struct Fixture(tempfile::TempDir);

    impl Fixture {
        fn new() -> Self {
            let fixture = Self(tempfile::tempdir().unwrap());
            for lineup in ["jp", "us"] {
                fs::create_dir(fixture.root().join(lineup)).unwrap();
                put_json(
                    &fixture.root().join(lineup).join("gamelist.json"),
                    &json!([]),
                );
            }
            fs::write(
                fixture.root().join(m2_publish::source_library::MARKER),
                b"2\n",
            )
            .unwrap();
            fixture
        }

        fn root(&self) -> &Path {
            self.0.path()
        }

        fn append(&self, base: &Path, name: &str) {
            let index = base.join("gamelist.json");
            let mut rows = read_json(&index).as_array().unwrap().clone();
            rows.push(
                json!({"folder":name,"sor_date":rows.len(),"sor_demo":rows.len(),
                "sor_name":rows.len(),"sor_genr":rows.len(),"sor_pnum":rows.len(),
                "futureRow":{"keep":[name,7]}}),
            );
            put_json(&index, &json!(rows));
        }

        fn folder(&self, name: &str) {
            let path = self.root().join(name);
            if path.exists() {
                return;
            }
            fs::create_dir(&path).unwrap();
            put_json(
                &path.join("folder.json"),
                &json!({"name":"Folder","future":true}),
            );
            put_json(&path.join("gamelist.json"), &json!([]));
            self.append(
                path.parent().unwrap(),
                path.file_name().unwrap().to_str().unwrap(),
            );
        }

        fn game(&self, base: &str, name: &str, rom: &str) -> PathBuf {
            if base.contains('/') {
                self.folder(base);
            }
            let base = self.root().join(base);
            let path = base.join(name);
            fs::create_dir(&path).unwrap();
            put_json(
                &path.join("game.json"),
                &json!({
                    "display":{"name":name,"tname":name,"name_eng":name,"titlebar":9},
                    "rom":{"arch":if rom.ends_with(".pcd") { "tg16cd" } else { "tg16" },
                        "country":"jp","rom":rom}, "cover":"cover.png", "future":{"keep":true}
                }),
            );
            fs::write(path.join("cover.png"), b"opaque artwork").unwrap();
            fs::write(path.join("sram.bin"), vec![0xa7; 8448]).unwrap();
            fs::create_dir(path.join("runtime")).unwrap();
            fs::write(path.join("runtime/assets.json"), b"cached artwork").unwrap();
            fs::create_dir(path.join("saves")).unwrap();
            for slot in 0..4 {
                fs::write(path.join(format!("saves/state_{slot}.bin")), [slot; 257]).unwrap();
                fs::write(
                    path.join(format!("saves/state_{slot}_meta.bin")),
                    [slot + 1; 17],
                )
                .unwrap();
            }
            let pool = m2_publish::rom_store::checked_pool(self.root()).unwrap();
            let name_in_pool = m2_publish::rom_store::console_name(rom).unwrap();
            fs::write(pool.join(name_in_pool), format!("synthetic ROM: {rom}")).unwrap();
            self.append(&base, name);
            path
        }

        fn remaining(&self, base: &str, deleted: &str) -> Vec<GameListEntry> {
            let rows: Vec<GameListEntry> =
                serde_json::from_value(read_json(&self.root().join(base).join("gamelist.json")))
                    .unwrap();
            rows.into_iter()
                .filter(|row| row.folder != deleted)
                .enumerate()
                .map(|(index, mut row)| {
                    row.sor_date = index as u32;
                    row.sor_demo = index as u32;
                    row.sor_genr = index as u32;
                    row.sor_name = index as u32;
                    row.sor_pnum = index as u32;
                    row
                })
                .collect()
        }

        fn delete(&self, base: &str, name: &str, folder: bool) -> DeleteResult {
            perform_with(
                self.root(),
                base,
                name,
                folder,
                &self.remaining(base, name),
                &m2_publish::save_format::atomic_write,
                &|root| {
                    m2_publish::load_library(root)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                },
            )
            .unwrap()
        }
    }

    fn put_json(path: &Path, value: &Value) {
        fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn collect(root: &Path, current: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(current).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    collect(root, &entry.path(), out);
                } else {
                    out.insert(
                        entry.path().strip_prefix(root).unwrap().to_owned(),
                        fs::read(entry.path()).unwrap(),
                    );
                }
            }
        }
        let mut out = BTreeMap::new();
        collect(root, root, &mut out);
        out
    }

    #[test]
    fn deleting_cd_and_hucard_commits_catalogue_and_removes_only_selected_game_and_rom() {
        for (base, rom) in [
            ("jp", "card.pce"),
            ("us", "card.pce.m"),
            ("jp/FOLDER00", "disc.pcd"),
        ] {
            let fixture = Fixture::new();
            let deleted = fixture.game(base, "GAME04", rom);
            let survivor = fixture.game(base, "GAME33", "keep.pcd");
            let survivor_before = snapshot(&survivor);
            let pool = fixture.root().join("published/roms");
            fs::write(pool.join("unmanaged.pcd"), b"user file").unwrap();
            fs::write(pool.join("syscard.pce.m"), b"BIOS").unwrap();
            let result = fixture.delete(base, "GAME04", false);
            assert!(result.warning.is_none(), "{:?}", result.warning);
            assert!(!deleted.exists());
            assert!(!pool
                .join(m2_publish::rom_store::console_name(rom).unwrap())
                .exists());
            assert!(pool.join("keep.pcd").is_file());
            assert!(pool.join("unmanaged.pcd").is_file());
            assert!(pool.join("syscard.pce.m").is_file());
            assert_eq!(snapshot(&survivor), survivor_before);
            assert_eq!(result.source.games.len(), 1);
            assert_eq!(result.source.games[0].folder, "GAME33");
            let rows = read_json(&fixture.root().join(base).join("gamelist.json"));
            assert_eq!(rows.as_array().unwrap().len(), 1);
            assert_eq!(rows[0]["folder"], "GAME33");
            assert_eq!(rows[0]["futureRow"], json!({"keep":["GAME33",7]}));
            for field in SORT_FIELDS {
                assert_eq!(rows[0][field], 0);
            }
            assert!(!fs::read_dir(fixture.root()).unwrap().any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".chronos-delete-")));
        }
    }

    #[test]
    fn shared_rom_stays_until_last_reference_in_another_lineup_folder_is_deleted() {
        let fixture = Fixture::new();
        fixture.game("jp", "GAME00", "shared.pcd");
        fixture.game("us/FOLDER00", "GAME99", "shared.pcd");
        assert!(fixture.delete("jp", "GAME00", false).warning.is_none());
        assert!(fixture.root().join("published/roms/shared.pcd").is_file());
        assert!(fixture.delete("us", "FOLDER00", true).warning.is_none());
        assert!(!fixture.root().join("published/roms/shared.pcd").exists());
        assert!(!fixture.root().join("us/FOLDER00").exists());
        assert_eq!(
            read_json(&fixture.root().join("us/gamelist.json")),
            json!([])
        );
    }

    #[test]
    fn folder_deletion_removes_all_children_and_only_unshared_roms() {
        let fixture = Fixture::new();
        fixture.game("jp/FOLDER00", "GAME00", "card.pce");
        fixture.game("jp/FOLDER00", "GAME01", "shared.pcd");
        let remaining = fixture.game("us", "GAME25", "shared.pcd");
        let before = snapshot(&remaining);
        let result = fixture.delete("jp", "FOLDER00", true);
        assert!(result.warning.is_none());
        assert!(result.library.jp.games.is_empty());
        assert!(!fixture.root().join("jp/FOLDER00").exists());
        assert!(!fixture.root().join("published/roms/card.pce.m").exists());
        assert!(fixture.root().join("published/roms/shared.pcd").is_file());
        assert_eq!(snapshot(&remaining), before);
    }

    #[test]
    fn shared_file_referenced_as_bios_or_alternate_rom_is_preserved() {
        let fixture = Fixture::new();
        fixture.game("jp", "GAME00", "shared.pce");
        let other = fixture.game("us", "GAME22", "other.pcd");
        let mut game = read_json(&other.join("game.json"));
        game["rom"]["tg16cd_systemcard"] = json!("SHARED.PCE.M");
        game["rom_alt"] = json!({"rom":"shared.pce"});
        put_json(&other.join("game.json"), &game);
        assert!(fixture.delete("jp", "GAME00", false).warning.is_none());
        assert!(fixture.root().join("published/roms/shared.pce.m").is_file());
    }

    #[test]
    fn failed_index_commit_restores_directory_catalogue_all_saves_and_roms() {
        for replace_before_error in [false, true] {
            let fixture = Fixture::new();
            fixture.game("jp/FOLDER00", "GAME00", "card.pce");
            fixture.game("jp/FOLDER00", "GAME01", "disc.pcd");
            let before = snapshot(fixture.root());
            let refreshed = Cell::new(false);
            let result = perform_with(
                fixture.root(),
                "jp/FOLDER00",
                "GAME00",
                false,
                &fixture.remaining("jp/FOLDER00", "GAME00"),
                &|path, bytes| {
                    // The commit writer runs before any directory mutation.
                    assert!(fixture
                        .root()
                        .join("jp/FOLDER00/GAME00/saves/state_3.bin")
                        .is_file());
                    assert!(fixture.root().join("jp/FOLDER00/GAME00/sram.bin").is_file());
                    if replace_before_error {
                        m2_publish::save_format::atomic_write(path, bytes)?;
                    }
                    Err("injected index failure".into())
                },
                &|_| {
                    refreshed.set(true);
                    Ok(())
                },
            );
            assert!(result.unwrap_err().contains("rolled back"));
            assert_eq!(snapshot(fixture.root()), before);
            assert!(!refreshed.get());
        }
    }

    #[test]
    fn font_refresh_failure_is_warning_after_committed_deletion() {
        let fixture = Fixture::new();
        fixture.game("jp", "GAME00", "disc.pcd");
        let result = perform_with(
            fixture.root(),
            "jp",
            "GAME00",
            false,
            &[],
            &m2_publish::save_format::atomic_write,
            &|_| Err("injected font failure".into()),
        )
        .unwrap();
        assert!(result.warning.unwrap().contains("injected font failure"));
        assert!(result.source.games.is_empty());
        assert!(!fixture.root().join("jp/GAME00").exists());
        assert!(!fixture.root().join("published/roms/disc.pcd").exists());
    }

    #[test]
    fn cleanup_failure_is_warning_and_retains_retry_candidate_after_commit() {
        let fixture = Fixture::new();
        fixture.game("jp", "GAME00", "disc.pcd");
        let unexpected = fixture.root().join("published/roms/disc.pcd");
        fs::remove_file(&unexpected).unwrap();
        fs::create_dir(&unexpected).unwrap();
        let result = fixture.delete("jp", "GAME00", false);
        assert!(result
            .warning
            .unwrap()
            .contains("Unused ROMs could not be removed"));
        assert!(result.source.games.is_empty());
        assert!(!fixture.root().join("jp/GAME00").exists());
        assert!(unexpected.is_dir());
        assert_eq!(
            read_json(&fixture.root().join(".rom-cleanup.json")),
            json!(["disc.pcd"])
        );
    }

    #[test]
    fn marker_failure_before_catalogue_commit_keeps_index_rom_and_save_data() {
        let fixture = Fixture::new();
        fixture.game("jp", "GAME00", "disc.pcd");
        let before = snapshot(fixture.root());
        let index_written = Cell::new(false);
        let result = perform_with_io(
            fixture.root(),
            "jp",
            "GAME00",
            false,
            &[],
            &|_, _| {
                index_written.set(true);
                Ok(())
            },
            &|_, _| Err("injected marker write failure".into()),
            &|_| Ok(()),
        );
        assert!(result
            .unwrap_err()
            .contains("injected marker write failure"));
        assert!(!index_written.get());
        assert_eq!(snapshot(fixture.root()), before);
    }

    #[test]
    fn persisted_marker_blocks_resurrection_after_interrupted_cleanup_and_allows_name_reuse() {
        let fixture = Fixture::new();
        fs::remove_file(fixture.root().join(m2_publish::source_library::MARKER)).unwrap();
        let target = fixture.game("jp", "GAME00", "disc.pcd");
        fixture.game("jp", "GAME33", "keep.pcd");
        let stale = crate::load_lineup(&fixture.root().join("jp"));
        // This is the exact durable state after catalogue commit but before
        // cleanup, including when Windows refuses to rename the directory.
        fs::write(target.join(DELETION_MARKER), MARKER_VERSION).unwrap();
        put_json(
            &fixture.root().join("jp/gamelist.json"),
            &serde_json::to_value(fixture.remaining("jp", "GAME00")).unwrap(),
        );
        let after_commit = snapshot(fixture.root());
        assert!(crate::save_lineup(&fixture.root().join("jp"), &stale)
            .unwrap_err()
            .contains("was deleted"));
        assert!(
            crate::save_game_json(&target.join("game.json"), &stale.games[0].game)
                .unwrap_err()
                .contains("was deleted")
        );
        assert_eq!(snapshot(fixture.root()), after_commit);

        // Explicit creation still requires a fresh directory. Removing the
        // orphan and creating a new game under the same name is supported.
        fs::remove_dir_all(&target).unwrap();
        let new_game = crate::create_game(
            fixture.root().to_string_lossy().into_owned(),
            "jp".into(),
            "GAME00".into(),
        )
        .unwrap();
        let mut current = crate::load_lineup(&fixture.root().join("jp"));
        current.games.push(new_game);
        crate::save_lineup(&fixture.root().join("jp"), &current).unwrap();
        assert!(!target.join(DELETION_MARKER).exists());
        assert_eq!(
            crate::load_lineup(&fixture.root().join("jp")).games.len(),
            2
        );
    }

    #[test]
    fn deleted_folder_marker_blocks_parent_and_direct_empty_or_nonempty_saves() {
        let fixture = Fixture::new();
        let game_dir = fixture.game("jp/FOLDER00", "GAME00", "disc.pcd");
        let root_snapshot = crate::load_lineup(&fixture.root().join("jp"));
        let folder_snapshot = crate::load_lineup(&fixture.root().join("jp/FOLDER00"));
        let folder = fixture.root().join("jp/FOLDER00");
        fs::write(folder.join(DELETION_MARKER), MARKER_VERSION).unwrap();
        put_json(&fixture.root().join("jp/gamelist.json"), &json!([]));
        let before = snapshot(fixture.root());
        let empty = Library {
            games: vec![],
            path: folder.to_string_lossy().into_owned(),
        };
        for (path, library) in [
            (fixture.root().join("jp"), &root_snapshot),
            (folder.clone(), &folder_snapshot),
            (folder.clone(), &empty),
        ] {
            assert!(crate::save_lineup(&path, library)
                .unwrap_err()
                .contains("was deleted"));
        }
        assert!(
            crate::save_game_json(&game_dir.join("game.json"), &folder_snapshot.games[0].game)
                .unwrap_err()
                .contains("was deleted")
        );
        assert_eq!(snapshot(fixture.root()), before);
    }

    #[test]
    fn stale_empty_folder_save_never_recreates_physically_removed_directory() {
        let fixture = Fixture::new();
        fixture.folder("jp/FOLDER00");
        let folder = fixture.root().join("jp/FOLDER00");
        let stale = crate::load_lineup(&folder);
        assert!(stale.games.is_empty());
        fixture.delete("jp", "FOLDER00", true);
        assert!(!folder.exists());
        assert!(crate::save_lineup(&folder, &stale)
            .unwrap_err()
            .contains("no folder was recreated"));
        assert!(!folder.exists());

        // New root lineups retain their supported empty-library behavior.
        fs::remove_dir_all(fixture.root().join("us")).unwrap();
        crate::save_lineup(
            &fixture.root().join("us"),
            &Library {
                games: vec![],
                path: String::new(),
            },
        )
        .unwrap();
        assert_eq!(
            read_json(&fixture.root().join("us/gamelist.json")),
            json!([])
        );
    }

    #[test]
    fn marker_before_catalogue_commit_remains_inactive_and_does_not_block_edits() {
        let fixture = Fixture::new();
        fs::remove_file(fixture.root().join(m2_publish::source_library::MARKER)).unwrap();
        let game = fixture.game("jp", "GAME00", "disc.pcd");
        fs::write(game.join(DELETION_MARKER), MARKER_VERSION).unwrap();
        ensure_not_deleted(&game).unwrap();
        let mut current = crate::load_lineup(&fixture.root().join("jp"));
        current.games[0].game.display.name_eng = "Still editable".into();
        crate::save_lineup(&fixture.root().join("jp"), &current).unwrap();
        assert_eq!(
            read_json(&game.join("game.json"))["display"]["name_eng"],
            "Still editable"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_locked_game_after_commit_cannot_be_restored_by_an_old_window() {
        use std::{cell::RefCell, os::windows::fs::OpenOptionsExt};
        let fixture = Fixture::new();
        let target = fixture.game("jp", "GAME00", "disc.pcd");
        let stale = crate::load_lineup(&fixture.root().join("jp"));
        let locked = RefCell::new(None);
        let result = perform_with(
            fixture.root(),
            "jp",
            "GAME00",
            false,
            &[],
            &|path, bytes| {
                m2_publish::save_format::atomic_write(path, bytes)?;
                // Simulate antivirus/another application taking an exclusive
                // handle after preflight but before postcommit directory rename.
                *locked.borrow_mut() = Some(
                    fs::OpenOptions::new()
                        .read(true)
                        .share_mode(0)
                        .open(target.join("game.json"))
                        .unwrap(),
                );
                Ok(())
            },
            &|_| Ok(()),
        )
        .unwrap();
        assert!(result
            .warning
            .unwrap()
            .contains("Deleted game files remain"));
        assert!(target.is_dir());
        assert!(result.source.games.is_empty());
        assert!(crate::save_lineup(&fixture.root().join("jp"), &stale)
            .unwrap_err()
            .contains("was deleted"));
        drop(locked.into_inner());
        assert_eq!(
            read_json(&fixture.root().join("jp/gamelist.json")),
            json!([])
        );
        assert!(ensure_not_deleted(&target)
            .unwrap_err()
            .contains("was deleted"));
    }

    #[test]
    fn stale_or_unsafe_plans_fail_before_any_write() {
        for (base, name) in [
            ("jp/..", "GAME00"),
            ("../jp", "GAME00"),
            ("jp", "../GAME00"),
            ("jp", "GAME00/"),
            ("jp", "C:GAME00"),
            ("jp", "missing"),
        ] {
            let fixture = Fixture::new();
            fixture.game("jp", "GAME00", "disc.pcd");
            let before = snapshot(fixture.root());
            assert!(perform_with(
                fixture.root(),
                base,
                name,
                false,
                &[],
                &m2_publish::save_format::atomic_write,
                &|_| Ok(())
            )
            .is_err());
            assert_eq!(snapshot(fixture.root()), before);
        }
        let fixture = Fixture::new();
        fixture.game("jp", "GAME00", "disc.pcd");
        fixture.game("jp", "GAME01", "card.pce");
        let before = snapshot(fixture.root());
        for plan in [
            vec![],
            vec![GameListEntry {
                folder: "GAME00".into(),
                sor_date: 0,
                sor_demo: 0,
                sor_genr: 0,
                sor_name: 0,
                sor_pnum: 0,
            }],
        ] {
            assert!(perform_with(
                fixture.root(),
                "jp",
                "GAME00",
                false,
                &plan,
                &m2_publish::save_format::atomic_write,
                &|_| Ok(())
            )
            .is_err());
            assert_eq!(snapshot(fixture.root()), before);
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_entry_or_child_is_never_followed_or_deleted() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let game = fixture.game("jp", "GAME00", "disc.pcd");
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("important"), b"keep me").unwrap();
        symlink(outside.path(), game.join("external")).unwrap();
        assert!(perform_with(
            fixture.root(),
            "jp",
            "GAME00",
            false,
            &[],
            &m2_publish::save_format::atomic_write,
            &|_| Ok(())
        )
        .is_err());
        assert!(game.join("game.json").is_file());
        assert_eq!(
            fs::read(outside.path().join("important")).unwrap(),
            b"keep me"
        );
        fs::remove_file(game.join("external")).unwrap();
        fs::rename(&game, outside.path().join("game")).unwrap();
        symlink(outside.path().join("game"), &game).unwrap();
        assert!(perform_with(
            fixture.root(),
            "jp",
            "GAME00",
            false,
            &[],
            &m2_publish::save_format::atomic_write,
            &|_| Ok(())
        )
        .is_err());
        assert!(outside.path().join("game/game.json").is_file());
    }

    #[test]
    fn stale_save_never_recreates_deleted_entry_or_partially_updates_survivors() {
        let fixture = Fixture::new();
        let survivor = fixture.game("jp", "GAME00", "keep.pcd");
        fixture.game("jp", "GAME01", "deleted.pcd");
        let mut stale = crate::load_lineup(&fixture.root().join("jp"));
        stale.games[0].game.display.name_eng = "old pending edit".into();
        fixture.delete("jp", "GAME01", false);
        let before = snapshot(&survivor);
        let index = fs::read(fixture.root().join("jp/gamelist.json")).unwrap();
        let error = crate::save_lineup(&fixture.root().join("jp"), &stale).unwrap_err();
        assert!(error.contains("Reopen the library"));
        assert_eq!(snapshot(&survivor), before);
        assert_eq!(
            fs::read(fixture.root().join("jp/gamelist.json")).unwrap(),
            index
        );
        assert!(!fixture.root().join("jp/GAME01").exists());
    }

    #[test]
    fn save_started_during_deletion_cannot_restore_old_catalogue_after_commit() {
        use std::{cell::RefCell, sync::mpsc, thread, time::Duration};
        let fixture = Fixture::new();
        fs::remove_file(fixture.root().join(m2_publish::source_library::MARKER)).unwrap();
        fixture.game("jp", "GAME00", "keep.pcd");
        fixture.game("jp", "GAME01", "deleted.pcd");
        let stale = crate::load_lineup(&fixture.root().join("jp"));
        let base = fixture.root().join("jp");
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = RefCell::new(None);
        let result = perform_with(
            fixture.root(),
            "jp",
            "GAME01",
            false,
            &fixture.remaining("jp", "GAME01"),
            &|path, bytes| {
                let stale = stale.clone();
                let base = base.clone();
                let started = started_tx.clone();
                let done = done_tx.clone();
                *worker.borrow_mut() = Some(thread::spawn(move || {
                    started.send(()).unwrap();
                    let result = crate::save_lineup(&base, &stale);
                    done.send(()).unwrap();
                    result
                }));
                started_rx.recv().unwrap();
                // The writer is in the delete transaction. An overlapping
                // save must wait for its mutex, rather than pass preflight.
                assert!(matches!(
                    done_rx.recv_timeout(Duration::from_millis(50)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ));
                m2_publish::save_format::atomic_write(path, bytes)
            },
            &|_| Ok(()),
        )
        .unwrap();
        assert!(result.warning.is_none());
        let error = worker.into_inner().unwrap().join().unwrap().unwrap_err();
        assert!(error.contains("Reopen the library"));
        assert_eq!(
            read_json(&fixture.root().join("jp/gamelist.json"))
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(!fixture.root().join("jp/GAME01").exists());
        assert!(fixture.root().join("jp/GAME00/game.json").is_file());
    }
}
