//! A game move commits both catalogues before any derived-resource scan.
//! The complete directory moves once; identities, artwork and saves stay together.
use crate::{DualLibrary, GameListEntry, Library};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

const SORT_FIELDS: [&str; 5] = ["sor_date", "sor_demo", "sor_genr", "sor_name", "sor_pnum"];

#[derive(Debug, Serialize)]
pub struct MoveGameResult {
    pub folder_name: String,
    pub library: DualLibrary,
    pub source: Library,
    pub warning: Option<String>,
}

fn basename(name: &str) -> Result<(), String> {
    let mut parts = Path::new(name).components();
    if name.contains(['/', '\\'])
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
        return Err(format!(
            "Expected a regular library directory: {}",
            path.display()
        ));
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

struct Index {
    path: PathBuf,
    original: Vec<u8>,
    rows: Vec<Value>,
}

impl Index {
    fn read(base: &Path) -> Result<Self, String> {
        let path = base.join("gamelist.json");
        let original = read_file(&path)?;
        let rows: Vec<Value> = serde_json::from_slice(&original)
            .map_err(|e| format!("Invalid {}: {e}", path.display()))?;
        let mut names = std::collections::BTreeSet::new();
        for row in &rows {
            let sort: GameListEntry = serde_json::from_value(row.clone())
                .map_err(|e| format!("Invalid entry in {}: {e}", path.display()))?;
            basename(&sort.folder)?;
            if !names.insert(sort.folder.clone()) {
                return Err(format!(
                    "Duplicate entry '{}' in {}",
                    sort.folder,
                    path.display()
                ));
            }
            directory(&base.join(&sort.folder))?;
        }
        Ok(Self {
            path,
            original,
            rows,
        })
    }
}

/// Only ordering is accepted from the UI. Every row and its unknown fields
/// come from disk; a stale UI cannot add, omit or duplicate another game.
fn sorted_rows(rows: Vec<Value>, plan: &[GameListEntry]) -> Result<Vec<Value>, String> {
    if rows.len() != plan.len() {
        return Err("The game list changed; reopen the library before moving this game".into());
    }
    let mut by_name: BTreeMap<_, _> = rows
        .into_iter()
        .map(|row| (row["folder"].as_str().unwrap().to_owned(), row))
        .collect();
    plan.iter()
        .map(|sort| {
            let mut row = by_name.remove(&sort.folder).ok_or_else(|| {
                "The game list changed; reopen the library before moving this game".to_string()
            })?;
            let new_sort = serde_json::to_value(sort).map_err(|e| e.to_string())?;
            for field in SORT_FIELDS {
                row[field] = new_sort[field].clone();
            }
            Ok(row)
        })
        .collect()
}

fn json_bytes(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn perform_move(
    root: &Path,
    src: &str,
    name: &str,
    dest: &str,
    source_sort: &[GameListEntry],
    destination_sort: &[GameListEntry],
) -> Result<MoveGameResult, String> {
    perform_with(
        root,
        src,
        name,
        dest,
        source_sort,
        destination_sort,
        &m2_publish::save_format::atomic_write,
        &|root| m2_publish::source_library::metadata_changed(root).map_err(|e| e.to_string()),
    )
}

#[cfg(test)]
#[path = "library_move_tests.rs"]
mod tests;

fn perform_with(
    root: &Path,
    src: &str,
    name: &str,
    dest: &str,
    source_sort: &[GameListEntry],
    destination_sort: &[GameListEntry],
    write: &impl Fn(&Path, &[u8]) -> Result<(), String>,
    refresh: &impl Fn(&Path) -> Result<(), String>,
) -> Result<MoveGameResult, String> {
    let _cover_update = crate::import_commands::lock_cover_update()?;
    directory(root)?;
    basename(name)?;
    let source_base = container(root, src)?;
    let dest_base = container(root, dest)?;
    if source_base == dest_base {
        return Err("Choose a different destination for this game".into());
    }
    let source_path = source_base.join(name);
    directory(&source_path)?;
    let mut source_index = Index::read(&source_base)?;
    let mut dest_index = Index::read(&dest_base)?;
    let position = source_index
        .rows
        .iter()
        .position(|row| row["folder"] == name)
        .ok_or("The source game list no longer contains this game; reopen the library")?;
    let mut moved_row = source_index.rows.remove(position);
    let source_rows = sorted_rows(std::mem::take(&mut source_index.rows), source_sort)?;
    let (moved_sort, existing_sort) = destination_sort
        .split_last()
        .ok_or("The destination game list is missing the moved game")?;
    if moved_sort.folder != name {
        return Err("The destination game list does not match the moved game".into());
    }
    let mut dest_rows = sorted_rows(std::mem::take(&mut dest_index.rows), existing_sort)?;

    // Capture legacy native tags before suffixing a conflicting directory name.
    // This migration validates all identities before persisting any additions.
    m2_publish::identity::migrate_library(root)?;
    m2_publish::identity::validate_destination(&source_path, &dest_base)?;
    let original_game = read_file(&source_path.join("game.json"))?;
    let mut game: Value =
        serde_json::from_slice(&original_game).map_err(|e| format!("Invalid game.json: {e}"))?;
    if !game["rom"].is_object() || game["rom"]["arch"] == "folder" {
        return Err("Only games can be moved".into());
    }
    let mut final_name = name.to_owned();
    for suffix in 2u64.. {
        match fs::symlink_metadata(dest_base.join(&final_name)) {
            Ok(_) => final_name = format!("{name}-{suffix}"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(format!("Inspect destination: {e}")),
        }
    }
    let destination_path = dest_base.join(&final_name);
    moved_row["folder"] = Value::String(final_name.clone());
    let sort_value = serde_json::to_value(moved_sort).map_err(|e| e.to_string())?;
    for field in SORT_FIELDS {
        moved_row[field] = sort_value[field].clone();
    }
    dest_rows.push(moved_row);
    game["rom"]["country"] = Value::String(dest.split('/').next().unwrap().to_owned());
    // Serialize all updates before the directory move. The two raw catalogues
    // retain unknown row fields, and game.json retains all emulator settings.
    let new_game = json_bytes(&game)?;
    let new_source = json_bytes(&source_rows)?;
    let new_dest = json_bytes(&dest_rows)?;
    fs::rename(&source_path, &destination_path)
        .map_err(|e| format!("Failed to move game folder: {e}"))?;
    let commit = (|| {
        write(&destination_path.join("game.json"), &new_game)?;
        write(&dest_index.path, &new_dest)?;
        write(&source_index.path, &new_source)
    })();
    if let Err(error) = commit {
        // Even a sync failure may follow a successful replacement. Restore all
        // three original byte streams rather than guessing which write failed.
        let mut failures = Vec::new();
        if let Err(e) = m2_publish::save_format::atomic_write(
            &destination_path.join("game.json"),
            &original_game,
        ) {
            failures.push(e);
        }
        if let Err(e) = fs::rename(&destination_path, &source_path) {
            failures.push(e.to_string());
        }
        for index in [&dest_index, &source_index] {
            if let Err(e) = m2_publish::save_format::atomic_write(&index.path, &index.original) {
                failures.push(e);
            }
        }
        return if failures.is_empty() {
            Err(format!("Game move rolled back: {error}"))
        } else {
            Err(format!("Game move interrupted: {error}. Recovery could not finish: {}. Check {} and {} before retrying; game files were not deleted.", failures.join("; "), source_path.display(), destination_path.display()))
        };
    }

    // Both lists now refer to real directories. A derived-font failure must
    // report a warning, not invite a retry of an already committed move.
    let warning = refresh(root)
        .err()
        .map(|e| format!("Console fonts could not be updated: {e}"));
    Ok(MoveGameResult {
        folder_name: final_name,
        source: crate::load_lineup(&source_base),
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
        warning,
    })
}
