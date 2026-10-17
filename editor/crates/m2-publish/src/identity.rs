//! Stable game identities belong to game.json; saves remain beside that file.
//! Directory names, menu positions and lineups are not game identities.
use serde_json::Value;
use std::{collections::{BTreeMap, BTreeSet}, fs, io::{Read, Write}, path::{Path, PathBuf}};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameIdentity {
    pub game_id: String,
    pub native_region_tag: String,
}

pub fn valid_game_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

pub fn valid_native_tag(value: &str) -> bool {
    !value.is_empty() && value.len() <= 14 && value.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
}

pub fn new_game_identity() -> GameIdentity {
    let game_id = uuid::Uuid::new_v4().simple().to_string();
    // Twelve bytes leave room for the native E/S1/S2 alternate suffixes.
    let native_region_tag = format!("G{}", &game_id[..11]);
    GameIdentity { game_id, native_region_tag }
}

/// Replace JSON atomically in the same directory. Serialization, staged
/// write/sync and rename failures leave the original intact, including on
/// Windows. The committed directory entry is synced on Unix as well.
pub fn atomic_write_json(path: &Path, value: &Value) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| format!("JSON path has no parent: {}", path.display()))?;
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!("Refusing to replace symlink {}", path.display()));
    }
    let mut output = tempfile::NamedTempFile::new_in(parent).map_err(|e| format!("Prepare {}: {e}", path.display()))?;
    serde_json::to_writer_pretty(&mut output, value).map_err(|e| format!("Serialize {}: {e}", path.display()))?;
    output.write_all(b"\n").map_err(|e| format!("Write {}: {e}", path.display()))?;
    output.as_file().sync_all().map_err(|e| format!("Sync {}: {e}", path.display()))?;
    output.persist(path).map_err(|e| format!("Replace {}: {e}", path.display()))?;
    #[cfg(unix)]
    fs::File::open(parent).and_then(|directory| directory.sync_all())
        .map_err(|e| format!("Sync JSON directory {}: {e}", parent.display()))?;
    Ok(())
}

fn read_json(path: &Path) -> Result<Value, String> {
    if !fs::symlink_metadata(path).map_err(|e| format!("Inspect {}: {e}", path.display()))?.file_type().is_file() {
        return Err(format!("Expected a regular game.json: {}", path.display()));
    }
    serde_json::from_slice(&fs::read(path).map_err(|e| format!("Read {}: {e}", path.display()))?)
        .map_err(|e| format!("Invalid {}: {e}", path.display()))
}

/// Trust a native state tag only after its layout, header and metadata digest
/// have been verified. Unknown files are preserved, never guessed or retagged.
fn state_tags(game: &Path) -> Result<(BTreeSet<String>, bool), String> {
    let mut tags = BTreeSet::new();
    let mut any_states = false;
    for slot in 0..4 {
        let path = game.join(format!("saves/state_{slot}.bin"));
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("Inspect {}: {e}", path.display())),
        };
        if !metadata.file_type().is_file() { return Err(format!("Expected a regular save file: {}", path.display())); }
        if metadata.len() == 0 { continue; }
        any_states = true;
        let meta = game.join(format!("saves/state_{slot}_meta.bin"));
        if let Ok(state) = crate::save_format::validate_state_pair(&path, &meta) {
            tags.insert(state.region_tag);
        }
    }
    Ok((tags, any_states))
}

/// The console stores the owner beside the save files. An older editor may
/// remove game_id from game.json; that must not assign the saves a new owner.
fn saved_game_id(game: &Path) -> Result<Option<String>, String> {
    let saves = game.join("saves");
    let directory = match fs::symlink_metadata(&saves) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Inspect save directory {}: {error}", saves.display())),
    };
    // Check the parent before inspecting its child: symlink_metadata on the
    // marker alone would still follow a symlink in the saves component.
    if !directory.file_type().is_dir() {
        return Err(format!("Expected a regular save directory, not a symlink or file: {}. Game identities and saves were not changed.", saves.display()));
    }
    let marker = saves.join(".chronos-game-id");
    let metadata = match fs::symlink_metadata(&marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Inspect save owner marker {}: {error}", marker.display())),
    };
    if !metadata.file_type().is_file() {
        return Err(format!("Expected a regular save owner marker, not a symlink or directory: {}. Game identities and saves were not changed.", marker.display()));
    }
    let invalid = || format!("Invalid save owner marker {}; expected 32 lowercase hexadecimal characters followed by a newline. Game identities and saves were not changed.", marker.display());
    if metadata.len() != 33 { return Err(invalid()); }
    let mut bytes = Vec::with_capacity(34);
    fs::File::open(&marker)
        .map_err(|error| format!("Read save owner marker {}: {error}", marker.display()))?
        .take(34).read_to_end(&mut bytes)
        .map_err(|error| format!("Read save owner marker {}: {error}", marker.display()))?;
    if bytes.len() != 33 || bytes[32] != b'\n' { return Err(invalid()); }
    let id = std::str::from_utf8(&bytes[..32]).map_err(|_| invalid())?;
    if !valid_game_id(id) { return Err(invalid()); }
    Ok(Some(id.to_owned()))
}

fn plan_identity(path: &Path, data: &mut Value) -> Result<GameIdentity, String> {
    if !data.is_object() { return Err(format!("Expected a game object: {}", path.display())); }
    let game = path.parent().ok_or("game.json has no directory")?;
    let saved_id = saved_game_id(game)?;
    let game_id = match data.get("game_id") {
        None | Some(Value::Null) => saved_id.clone().unwrap_or_else(|| new_game_identity().game_id),
        Some(Value::String(id)) if valid_game_id(id) => {
            if saved_id.as_ref().is_some_and(|owner| owner != id) {
                return Err(format!("game_id in {} does not match its save owner marker {}. Game identities and saves were not changed.", path.display(), game.join("saves/.chronos-game-id").display()));
            }
            id.clone()
        },
        _ => return Err(format!("Invalid game_id in {}; expected 32 lowercase hexadecimal characters. Existing identities are never replaced automatically.", path.display())),
    };
    let explicit = match data.get("native_region_tag") {
        None | Some(Value::Null) => None,
        Some(Value::String(tag)) if valid_native_tag(tag) => Some(tag.clone()),
        _ => return Err(format!("Invalid native_region_tag in {}; expected 1–14 ASCII letters, numbers, '_' or '-'.", path.display())),
    };
    // Native payloads are checked during the first legacy migration and by
    // save import/publication. Ordinary title edits and library opens must
    // not repeatedly hash every multi-megabyte state in the collection.
    if data.get("game_id").and_then(Value::as_str).is_some() {
        if let Some(native_region_tag) = explicit.clone() {
            return Ok(GameIdentity { game_id, native_region_tag });
        }
    }
    let (tags, any_states) = state_tags(game)?;
    if let Some(tag) = tags.iter().find(|tag| !valid_native_tag(tag)) {
        return Err(format!("Validated legacy save tag '{tag}' in {} cannot be used as a new native identifier. Save migration is required; the original data and metadata were not modified.", game.display()));
    }
    let alternate_tags: BTreeSet<String> = ["regionTagE", "regionTagS1", "regionTagS2"].iter()
        .filter_map(|key| data["rom_alt"][*key].as_str())
        .filter(|tag| valid_native_tag(tag)).map(str::to_owned).collect();
    let legacy = data["rom"]["regionTag"].as_str().filter(|tag| !tag.is_empty())
        .map(str::to_owned).unwrap_or_else(|| game.file_name().unwrap_or_default().to_string_lossy().into_owned());
    let known_tag = |tag: &str| tag == legacy || alternate_tags.contains(tag);
    if tags.len() > 1 && !tags.iter().all(|tag| known_tag(tag)) {
        return Err(format!("Save files in {} contain different native game tags; migrate these saves explicitly before changing the game identity.", game.display()));
    }
    if let Some(expected) = &explicit {
        if let Some(saved) = tags.iter().find(|tag| *tag != expected && !alternate_tags.contains(*tag)) {
            return Err(format!("Native tag '{expected}' in {} conflicts with save tag '{saved}'. Save migration is required; no saves were modified.", path.display()));
        }
    }
    let native_region_tag = if let Some(tag) = explicit { tag }
        else if valid_native_tag(&legacy) && tags.iter().all(|tag| known_tag(tag)) { legacy }
        else if let Some(tag) = tags.iter().find(|tag| valid_native_tag(tag) && !alternate_tags.contains(*tag)).cloned() { tag }
        else if valid_native_tag(&legacy) { legacy }
        else if any_states {
            return Err(format!("Legacy native tag '{legacy}' in {} is too long or unsafe, and its saves cannot be safely retagged automatically. Save migration is required; no saves were modified.", path.display()));
        } else { format!("G{}", &game_id[..11]) };
    data["game_id"] = Value::String(game_id.clone());
    data["native_region_tag"] = Value::String(native_region_tag.clone());
    Ok(GameIdentity { game_id, native_region_tag })
}

/// Attach identity before moving or renaming the directory; old native tags
/// are captured from the save payload or original metadata/directory name.
pub fn ensure_game_identity(path: &Path) -> Result<GameIdentity, String> {
    let original = read_json(path)?;
    let mut data = original.clone();
    let identity = plan_identity(path, &mut data)?;
    if data != original { atomic_write_json(path, &data)?; }
    Ok(identity)
}

/// Normalize an in-memory game before its first atomic save. Callers retain
/// arbitrary JSON fields; this only adds/validates the two identity fields.
pub fn normalize_game_json(path: &Path, data: &mut Value) -> Result<GameIdentity, String> {
    plan_identity(path, data)
}

fn game_paths(directory: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if !directory.exists() { return Ok(()); }
    if !fs::symlink_metadata(directory).map_err(|e| e.to_string())?.file_type().is_dir() {
        return Err(format!("Expected a regular library directory: {}", directory.display()));
    }
    for entry in fs::read_dir(directory).map_err(|e| format!("Read {}: {e}", directory.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() { return Err(format!("Symlinks are not supported in the game library: {}", entry.path().display())); }
        if kind.is_dir() { game_paths(&entry.path(), out)?; }
        else if entry.file_name() == "game.json" { out.push(entry.path()); }
    }
    Ok(())
}

/// Scan authoritative game.json files, including games currently omitted
/// from a gamelist. Validate the full plan before persisting any migration.
pub fn migrate_library(root: &Path) -> Result<usize, String> {
    let mut paths = Vec::new();
    for lineup in ["jp", "us"] { game_paths(&root.join(lineup), &mut paths)?; }
    paths.sort();
    let mut ids = BTreeMap::new();
    let mut tags = BTreeMap::new();
    let mut changes = Vec::new();
    for path in paths {
        let original = read_json(&path)?;
        if original["rom"]["arch"].as_str() == Some("folder") { continue; }
        let mut data = original.clone();
        let identity = plan_identity(&path, &mut data)?;
        if let Some(previous) = ids.insert(identity.game_id.clone(), path.clone()) {
            return Err(format!("Duplicate game_id '{}' in {} and {}. Copying a game requires a new identity; existing saves and IDs were not changed.", identity.game_id, previous.display(), path.display()));
        }
        let key = (path.parent().unwrap().parent().unwrap().to_path_buf(), identity.native_region_tag.clone());
        if let Some(previous) = tags.insert(key, path.clone()) {
            return Err(format!("Native game tag '{}' collides in {} and {}. These games need an explicit save/tag migration before sharing a menu; no identities or saves were changed.", identity.native_region_tag, previous.display(), path.display()));
        }
        if data != original { changes.push((path, data)); }
    }
    for (path, data) in &changes { atomic_write_json(path, data)?; }
    Ok(changes.len())
}

/// A move must not introduce a duplicate native tag in the destination menu.
pub fn validate_destination(source: &Path, destination: &Path) -> Result<(), String> {
    let identity = ensure_game_identity(&source.join("game.json"))?;
    if !destination.exists() { return Ok(()); }
    for entry in fs::read_dir(destination).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() || entry.path() == source { continue; }
        let game_json = entry.path().join("game.json");
        if !game_json.is_file() { continue; }
        let data = read_json(&game_json)?;
        if data["rom"]["arch"].as_str() == Some("folder") { continue; }
        let mut normalized = data;
        let other = plan_identity(&game_json, &mut normalized)?;
        if identity.game_id == other.game_id || identity.native_region_tag == other.native_region_tag {
            return Err(format!("Cannot move this game into {}: identity or native tag '{}' conflicts with {}. No game files were moved; explicit save/tag migration is required.", destination.display(), identity.native_region_tag, entry.path().display()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn game(root: &Path, name: &str, data: Value) -> PathBuf {
        let directory = root.join(name); fs::create_dir_all(&directory).unwrap();
        atomic_write_json(&directory.join("game.json"), &data).unwrap(); directory
    }
    #[test]
    fn migration_captures_old_folder_name_before_rename_and_is_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let directory = game(root.path(), "jp/GAME000", serde_json::json!({"rom":{"arch":"tg16"},"display":{"name":"Test"},"unknown":{"keep":7}}));
        assert_eq!(migrate_library(root.path()).unwrap(), 1);
        let first = read_json(&directory.join("game.json")).unwrap();
        assert!(valid_game_id(first["game_id"].as_str().unwrap()));
        assert_eq!(first["native_region_tag"], "GAME000"); assert_eq!(first["unknown"]["keep"], 7);
        fs::create_dir(root.path().join("us")).unwrap();
        fs::rename(directory, root.path().join("us/GAME000-2")).unwrap();
        assert_eq!(migrate_library(root.path()).unwrap(), 0);
        assert_eq!(read_json(&root.path().join("us/GAME000-2/game.json")).unwrap(), first);
    }
    #[test]
    fn safe_native_save_tag_takes_precedence_without_modifying_any_payload() {
        let root = tempfile::tempdir().unwrap();
        let directory = game(root.path(), "jp/VERY_LONG_LEGACY_FOLDER", serde_json::json!({"rom":{"regionTag":"VERY_LONG_LEGACY_REGION"}}));
        fs::create_dir(directory.join("saves")).unwrap();
        let path = directory.join("saves/state_2.bin");
        crate::save_format::tests::fixture_pair(&path, &directory.join("saves/state_2_meta.bin"), "PAYLOAD", 12, 7);
        let before = fs::read(&path).unwrap();
        let identity = ensure_game_identity(&directory.join("game.json")).unwrap();
        assert_eq!(identity.native_region_tag, "PAYLOAD"); assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(read_json(&directory.join("game.json")).unwrap()["rom"]["regionTag"], "VERY_LONG_LEGACY_REGION");
    }
    #[test]
    fn unsafe_legacy_tags_without_states_get_short_tags_but_unknown_states_block_migration() {
        let root = tempfile::tempdir().unwrap();
        let directory = game(root.path(), "jp/very-long-game-directory", serde_json::json!({"rom":{}}));
        let identity = ensure_game_identity(&directory.join("game.json")).unwrap();
        assert!(identity.native_region_tag.starts_with('G')); assert_eq!(identity.native_region_tag.len(), 12);
        let other = game(root.path(), "us/another-long-game-directory", serde_json::json!({"rom":{}}));
        fs::create_dir(other.join("saves")).unwrap(); fs::write(other.join("saves/state_0.bin"), b"unknown state").unwrap();
        let before = fs::read(other.join("game.json")).unwrap();
        assert!(ensure_game_identity(&other.join("game.json")).unwrap_err().contains("Save migration is required"));
        assert_eq!(fs::read(other.join("game.json")).unwrap(), before);
    }
    #[test]
    fn collisions_are_detected_before_any_migration_and_move_preserves_source_files() {
        let root = tempfile::tempdir().unwrap();
        let source = game(root.path(), "jp/GAME001", serde_json::json!({"rom":{"regionTag":"SAME"}}));
        game(root.path(), "jp/GAME002", serde_json::json!({"rom":{"regionTag":"SAME"}}));
        assert!(migrate_library(root.path()).unwrap_err().contains("collides"));
        assert!(read_json(&source.join("game.json")).unwrap().get("game_id").is_none());
        game(root.path(), "us/GAME003", serde_json::json!({"rom":{"regionTag":"SAME"}}));
        fs::write(source.join("sram.bin"), b"memory").unwrap();
        assert!(validate_destination(&source, &root.path().join("us")).unwrap_err().contains("Cannot move"));
        assert_eq!(fs::read(source.join("sram.bin")).unwrap(), b"memory");
    }
    #[test]
    fn duplicate_stable_ids_are_errors_even_in_different_folders() {
        let root = tempfile::tempdir().unwrap();
        for name in ["jp/GAME001", "us/FOLDER/GAME002"] {
            game(root.path(), name, serde_json::json!({"game_id":"0123456789abcdef0123456789abcdef","rom":{}}));
        }
        assert!(migrate_library(root.path()).unwrap_err().contains("Duplicate game_id"));
    }
    #[test]
    fn primary_and_declared_alternate_save_tags_keep_the_original_primary_identity() {
        let root = tempfile::tempdir().unwrap();
        let directory = game(root.path(), "jp/GAME001", serde_json::json!({"rom":{"regionTag":"PRIMARY"},"rom_alt":{"regionTagE":"PRIMARYE","regionTagS1":"PRIMARYS1"}}));
        for (slot, tag) in [(0,"PRIMARY"),(1,"PRIMARYE"),(3,"PRIMARYS1")] {
            crate::save_format::tests::fixture_pair(&directory.join(format!("saves/state_{slot}.bin")), &directory.join(format!("saves/state_{slot}_meta.bin")), tag, 12, slot);
        }
        assert_eq!(ensure_game_identity(&directory.join("game.json")).unwrap().native_region_tag, "PRIMARY");
        assert_eq!(ensure_game_identity(&directory.join("game.json")).unwrap().native_region_tag, "PRIMARY");
    }
    #[test]
    fn validated_unsafe_payload_tags_are_never_adopted_as_new_native_identifiers() {
        let root = tempfile::tempdir().unwrap();
        let directory = game(root.path(), "jp/legacy-directory-with-a-long-tag", serde_json::json!({"rom":{"regionTag":"too-long-for-native-tag"}}));
        let data = directory.join("saves/state_0.bin");
        let meta = directory.join("saves/state_0_meta.bin");
        crate::save_format::tests::fixture_pair(&data, &meta, "旧 NAME", 12, 2);
        let before_data = fs::read(&data).unwrap();
        let before_json = fs::read(directory.join("game.json")).unwrap();
        assert!(ensure_game_identity(&directory.join("game.json")).unwrap_err().contains("Save migration is required"));
        assert_eq!(fs::read(data).unwrap(), before_data);
        assert_eq!(fs::read(directory.join("game.json")).unwrap(), before_json);
    }

    #[test]
    fn missing_or_null_id_recovers_the_save_owner_without_changing_sram_or_four_states() {
        let root = tempfile::tempdir().unwrap();
        let owner = "0123456789abcdef0123456789abcdef";
        for (name, null_id) in [("jp/GAME_MOVED", false), ("us/GAME_MOVED", true)] {
            let mut json = serde_json::json!({"rom":{"regionTag":"ORIGINAL"},"unknown":{"keep":7}});
            if null_id { json["game_id"] = Value::Null; }
            let directory = game(root.path(), name, json);
            fs::create_dir(directory.join("saves")).unwrap();
            let marker = directory.join("saves/.chronos-game-id");
            fs::write(&marker, format!("{owner}\n")).unwrap();
            fs::write(directory.join("sram.bin"), b"existing persistent game memory").unwrap();
            fs::write(directory.join("saves/locks.bin"), [0b1010]).unwrap();
            for slot in 0..4 {
                crate::save_format::tests::fixture_pair(
                    &directory.join(format!("saves/state_{slot}.bin")),
                    &directory.join(format!("saves/state_{slot}_meta.bin")),
                    "ORIGINAL", 12, slot,
                );
            }
            let mut files = vec![directory.join("sram.bin"), marker, directory.join("saves/locks.bin")];
            for slot in 0..4 {
                files.push(directory.join(format!("saves/state_{slot}.bin")));
                files.push(directory.join(format!("saves/state_{slot}_meta.bin")));
            }
            let before: Vec<_> = files.iter().map(|path| fs::read(path).unwrap()).collect();
            let path = directory.join("game.json");
            let recovered = ensure_game_identity(&path).unwrap();
            assert_eq!(recovered.game_id, owner);
            assert_eq!(recovered.native_region_tag, "ORIGINAL");
            let saved = read_json(&path).unwrap();
            assert_eq!(saved["game_id"], owner);
            assert_eq!(saved["native_region_tag"], "ORIGINAL");
            assert_eq!(saved["rom"]["regionTag"], "ORIGINAL");
            assert_eq!(saved["unknown"]["keep"], 7);
            assert_eq!(ensure_game_identity(&path).unwrap(), recovered);
            for (file, bytes) in files.iter().zip(before) {
                assert_eq!(fs::read(file).unwrap(), bytes, "Save content changed: {}", file.display());
            }
        }
    }

    #[test]
    fn malformed_owner_markers_block_identity_changes_even_on_the_normal_fast_path() {
        let root = tempfile::tempdir().unwrap();
        for (index, contents) in [
            "", "0123456789abcdef0123456789abcdef", "0123456789abcdef0123456789abcdef\r\n",
            "0123456789abcdef0123456789abcdeF\n", "0123456789abcdef0123456789abcdef\nextra",
        ].into_iter().enumerate() {
            let directory = game(root.path(), &format!("jp/GAME{index}"), serde_json::json!({
                "game_id":"0123456789abcdef0123456789abcdef", "native_region_tag":"ORIGINAL", "rom":{},
            }));
            fs::create_dir(directory.join("saves")).unwrap();
            let marker = directory.join("saves/.chronos-game-id");
            fs::write(&marker, contents).unwrap();
            let path = directory.join("game.json");
            let before = fs::read(&path).unwrap();
            let mut in_memory = read_json(&path).unwrap();
            let original = in_memory.clone();
            assert!(normalize_game_json(&path, &mut in_memory).unwrap_err().contains("Invalid save owner marker"));
            assert_eq!(in_memory, original);
            assert!(ensure_game_identity(&path).unwrap_err().contains("Invalid save owner marker"));
            assert_eq!(fs::read(&path).unwrap(), before);
            assert_eq!(fs::read(&marker).unwrap(), contents.as_bytes());
        }
        let directory = game(root.path(), "us/GAME001", serde_json::json!({"rom":{}}));
        let marker = directory.join("saves/.chronos-game-id");
        fs::create_dir_all(&marker).unwrap();
        let before = fs::read(directory.join("game.json")).unwrap();
        assert!(ensure_game_identity(&directory.join("game.json")).unwrap_err().contains("Expected a regular save owner marker"));
        assert!(marker.is_dir());
        assert_eq!(fs::read(directory.join("game.json")).unwrap(), before);
    }

    #[test]
    fn explicit_identity_mismatching_its_save_owner_is_rejected_without_mutation() {
        let root = tempfile::tempdir().unwrap();
        let directory = game(root.path(), "jp/GAME001", serde_json::json!({
            "game_id":"0123456789abcdef0123456789abcdef", "native_region_tag":"ORIGINAL", "rom":{},
        }));
        fs::create_dir(directory.join("saves")).unwrap();
        let marker = directory.join("saves/.chronos-game-id");
        let marker_bytes = b"fedcba9876543210fedcba9876543210\n";
        fs::write(&marker, marker_bytes).unwrap();
        fs::write(directory.join("sram.bin"), b"saved memory").unwrap();
        let path = directory.join("game.json");
        let before = fs::read(&path).unwrap();
        let mut in_memory = read_json(&path).unwrap();
        let original = in_memory.clone();
        assert!(normalize_game_json(&path, &mut in_memory).unwrap_err().contains("does not match its save owner marker"));
        assert_eq!(in_memory, original);
        assert!(ensure_game_identity(&path).unwrap_err().contains("does not match its save owner marker"));
        assert_eq!(fs::read(path).unwrap(), before);
        assert_eq!(fs::read(marker).unwrap(), marker_bytes);
        assert_eq!(fs::read(directory.join("sram.bin")).unwrap(), b"saved memory");
    }

    #[test]
    fn missing_owner_marker_retains_legacy_identity_generation_without_creating_one() {
        let root = tempfile::tempdir().unwrap();
        for (name, has_saves) in [("jp/GAME001", false), ("us/GAME002", true)] {
            let directory = game(root.path(), name, serde_json::json!({"rom":{"regionTag":"LEGACY"}}));
            if has_saves {
                fs::create_dir(directory.join("saves")).unwrap();
                fs::write(directory.join("sram.bin"), b"legacy memory").unwrap();
            }
            let identity = ensure_game_identity(&directory.join("game.json")).unwrap();
            assert!(valid_game_id(&identity.game_id));
            assert_eq!(identity.native_region_tag, "LEGACY");
            assert!(!directory.join("saves/.chronos-game-id").exists());
            if has_saves { assert_eq!(fs::read(directory.join("sram.bin")).unwrap(), b"legacy memory"); }
        }
    }

    #[test]
    #[cfg(unix)]
    fn standalone_identity_recovery_refuses_symlink_markers_and_symlink_save_directories() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("outside-saves");
        fs::create_dir(&outside).unwrap();
        let owner = outside.join(".chronos-game-id");
        let marker_bytes = b"0123456789abcdef0123456789abcdef\n";
        fs::write(&owner, marker_bytes).unwrap();
        for (name, link_directory, dangling) in [
            ("jp/GAME001", false, false), ("jp/GAME002", true, false),
            ("us/GAME003", false, true), ("us/GAME004", true, true),
        ] {
            let directory = game(root.path(), name, serde_json::json!({"rom":{}}));
            let target = if dangling { root.path().join("missing-owner") } else if link_directory { outside.clone() } else { owner.clone() };
            if link_directory { symlink(&target, directory.join("saves")).unwrap(); }
            else {
                fs::create_dir(directory.join("saves")).unwrap();
                symlink(&target, directory.join("saves/.chronos-game-id")).unwrap();
            }
            let path = directory.join("game.json");
            let before = fs::read(&path).unwrap();
            let error = ensure_game_identity(&path).unwrap_err();
            assert!(error.contains(if link_directory { "regular save directory" } else { "regular save owner marker" }), "{error}");
            assert_eq!(fs::read(path).unwrap(), before);
            assert_eq!(fs::read(&owner).unwrap(), marker_bytes);
        }
    }
}
