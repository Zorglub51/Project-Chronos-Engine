//! Validated native save files and recoverable data/metadata replacement.
//! Native slots stay 0..3; the UI displays them as 1..4.
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

pub const STATE_TAG_OFFSET: u64 = 0x50;
pub const STATE_TAG_BYTES: usize = 15;
pub const HUCARD_STATE_SIZE: u64 = 895_848;
pub const CD_STATE_SIZE: u64 = 2_206_568;

#[derive(Debug, Clone)]
pub struct StateInfo {
    pub region_tag: String,
    pub segment_type: u16,
    pub digest: [u8; 16],
}

fn regular_file(path: &Path) -> Result<fs::Metadata, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|e| format!("Inspect {}: {e}", path.display()))?;
    if !metadata.file_type().is_file() {
        return Err(format!("Expected a regular file: {}", path.display()));
    }
    Ok(metadata)
}

pub fn file_digest(path: &Path) -> Result<[u8; 16], String> {
    regular_file(path)?;
    let mut file = File::open(path).map_err(|e| format!("Read {}: {e}", path.display()))?;
    let mut hash = Md5::new();
    let mut buffer = [0; 65_536];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|e| format!("Read {}: {e}", path.display()))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash.finalize().into())
}

pub fn digest_hex(digest: &[u8; 16]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() < STATE_TAG_BYTES
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// Read only the verified native layouts, never infer a format from a tag.
pub fn inspect_state(data: &Path) -> Result<(String, u16), String> {
    let size = regular_file(data)?.len();
    let segment = match size {
        HUCARD_STATE_SIZE => 12,
        CD_STATE_SIZE => 11,
        _ => {
            return Err(format!(
                "Unrecognized native save size in {}",
                data.display()
            ))
        }
    };
    let mut file = File::open(data).map_err(|e| format!("Read {}: {e}", data.display()))?;
    let mut header = [0; STATE_TAG_OFFSET as usize + STATE_TAG_BYTES];
    file.read_exact(&mut header)
        .map_err(|e| format!("Read {}: {e}", data.display()))?;
    if u32::from_le_bytes(header[..4].try_into().unwrap()) != 0x0001_0010
        || u32::from_le_bytes(header[4..8].try_into().unwrap()) == 0
    {
        return Err(format!(
            "Unrecognized native save header in {}",
            data.display()
        ));
    }
    let bytes = &header[STATE_TAG_OFFSET as usize..];
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| format!("Unterminated native game tag in {}", data.display()))?;
    let tag = std::str::from_utf8(&bytes[..end])
        .map_err(|_| format!("Invalid native game tag in {}", data.display()))?;
    // Existing native tags can contain UTF-8 or spaces. Preserve a proven
    // bounded tag as-is; newly assigned tags use the stricter ASCII policy.
    if tag.is_empty()
        || tag.chars().any(char::is_control)
        || bytes[end..].iter().any(|byte| *byte != 0)
    {
        return Err(format!("Invalid native game tag in {}", data.display()));
    }
    Ok((tag.to_owned(), segment))
}

fn read_u32(bytes: &[u8], at: usize) -> Result<usize, String> {
    let raw = bytes.get(at..at + 4).ok_or("Truncated save metadata")?;
    Ok(u32::from_le_bytes(raw.try_into().unwrap()) as usize)
}

fn single_uint(bytes: &[u8], at: usize) -> Result<usize, String> {
    let mut at = at;
    let width = *bytes.get(at).ok_or("Truncated save metadata array")?;
    if !(13..=16).contains(&width) {
        return Err("Invalid save metadata array".into());
    }
    let width = (width - 12) as usize;
    at += 1;
    let raw = bytes
        .get(at..at + width)
        .ok_or("Truncated save metadata array")?;
    let mut count = 0;
    for (shift, byte) in raw.iter().enumerate() {
        count |= (*byte as usize) << (shift * 8);
    }
    if count != 1 {
        return Err("Save metadata must contain exactly one digest stream".into());
    }
    at += width;
    let width = *bytes.get(at).ok_or("Truncated save metadata array")?;
    if width == 0 {
        return Ok(0);
    }
    if !(13..=16).contains(&width) {
        return Err("Invalid save metadata value".into());
    }
    let width = (width - 12) as usize;
    at += 1;
    let raw = bytes
        .get(at..at + width)
        .ok_or("Truncated save metadata value")?;
    let mut value = 0;
    for (shift, byte) in raw.iter().enumerate() {
        value |= (*byte as usize) << (shift * 8);
    }
    Ok(value)
}

/// Preserve the metadata representation; its digest offset is not constant.
fn metadata_digest_offset(bytes: &[u8]) -> Result<usize, String> {
    if bytes.len() < 44
        || bytes.len() > 4096
        || &bytes[..4] != b"PSB\0"
        || !matches!(u16::from_le_bytes(bytes[4..6].try_into().unwrap()), 2 | 3)
        || bytes[6..8] != [0, 0]
    {
        return Err("Unrecognized native save metadata".into());
    }
    // Metadata fields stay opaque. Validate bounded native tables directly,
    // avoiding an unbounded generic PSB allocation from damaged counts.
    let header_size = if bytes[4] == 3 { 44 } else { 40 };
    for field in (8..40).step_by(4) {
        let offset = read_u32(bytes, field)?;
        if offset < header_size || offset >= bytes.len() {
            return Err("Invalid native metadata header offset".into());
        }
    }
    if bytes[read_u32(bytes, 36)?] != 33 {
        return Err("Native save metadata root is not an object".into());
    }
    if bytes[4] == 3 {
        let (mut a, mut b) = (1u32, 0u32);
        for byte in &bytes[8..40] {
            a = (a + u32::from(*byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        if ((b << 16) | a) != read_u32(bytes, 40)? as u32 {
            return Err("Native metadata header checksum mismatch".into());
        }
    }
    let relative = single_uint(bytes, read_u32(bytes, 24)?)?;
    let length = single_uint(bytes, read_u32(bytes, 28)?)?;
    let base = read_u32(bytes, 32)?;
    let at = base
        .checked_add(relative)
        .ok_or("Invalid save digest offset")?;
    if base < 44 || length != 16 || at.checked_add(16).is_none_or(|end| end > bytes.len()) {
        return Err("Invalid save digest stream".into());
    }
    Ok(at)
}

pub fn validate_data_digest(data: &Path, meta: &Path) -> Result<[u8; 16], String> {
    if regular_file(meta)?.len() > 4096 {
        return Err("Native save metadata is too large".into());
    }
    let bytes = fs::read(meta).map_err(|e| format!("Read {}: {e}", meta.display()))?;
    let at = metadata_digest_offset(&bytes)?;
    let digest = file_digest(data)?;
    if bytes[at..at + 16] != digest {
        return Err(format!(
            "Save data/metadata digest mismatch: {}",
            data.display()
        ));
    }
    Ok(digest)
}

pub fn validate_state_pair(data: &Path, meta: &Path) -> Result<StateInfo, String> {
    let (region_tag, segment_type) = inspect_state(data)?;
    let digest = validate_data_digest(data, meta)?;
    Ok(StateInfo {
        region_tag,
        segment_type,
        digest,
    })
}

fn sync_directory(parent: &Path) -> Result<(), String> {
    #[cfg(unix)]
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|e| format!("Sync {}: {e}", parent.display()))?;
    #[cfg(not(unix))]
    let _ = parent;
    Ok(())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Save path has no parent")?;
    if fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!(
            "Refusing to write through save-directory symlink {}",
            parent.display()
        ));
    }
    fs::create_dir_all(parent).map_err(|e| format!("Create {}: {e}", parent.display()))?;
    if fs::symlink_metadata(path).is_ok_and(|m| !m.file_type().is_file()) {
        return Err(format!("Refusing to replace non-file {}", path.display()));
    }
    let mut output = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    output.write_all(bytes).map_err(|e| e.to_string())?;
    output.as_file().sync_all().map_err(|e| e.to_string())?;
    output
        .persist(path)
        .map_err(|e| format!("Replace {}: {e}", path.display()))?;
    sync_directory(parent)
}

#[derive(Debug, Serialize, Deserialize)]
struct PairJournal {
    version: u32,
    data: String,
    meta: String,
    staged_data: String,
    staged_meta: String,
    old_data: Option<String>,
    old_meta: Option<String>,
    new_data: String,
    new_meta: String,
}

fn basename(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or("Invalid save filename".into())
}

fn pair_parent(data: &Path, meta: &Path) -> Result<PathBuf, String> {
    let parent = data.parent().ok_or("Save path has no parent")?;
    if meta.parent() != Some(parent) {
        return Err("Save data and metadata must share a directory".into());
    }
    if fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!(
            "Refusing to migrate through save-directory symlink {}",
            parent.display()
        ));
    }
    Ok(parent.to_path_buf())
}

fn journal_path(data: &Path) -> Result<PathBuf, String> {
    Ok(data.with_file_name(format!(".{}.chronos-migration.json", basename(data)?)))
}

fn optional_digest(path: &Path) -> Result<Option<String>, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => file_digest(path).map(|digest| Some(digest_hex(&digest))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Inspect {}: {e}", path.display())),
    }
}

fn journal_member(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err("Invalid migration journal filename".into());
    }
    Ok(parent.join(name))
}

fn backup_original(path: &Path) -> Result<(), String> {
    if path.parent().is_some_and(|parent| {
        fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink())
    }) {
        return Err(format!(
            "Refusing to back up through save-directory symlink {}",
            path.display()
        ));
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("Inspect {}: {e}", path.display())),
    };
    if !metadata.file_type().is_file() {
        return Err(format!("Refusing to back up non-file {}", path.display()));
    }
    if metadata.len() == 0 {
        return Ok(());
    }
    let backup = path.with_file_name(format!("{}.pre-chronos-v2", basename(path)?));
    let mut output = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            regular_file(&backup)?;
            return Ok(());
        }
        Err(e) => return Err(format!("Back up {}: {e}", path.display())),
    };
    let result = File::open(path)
        .and_then(|mut input| std::io::copy(&mut input, &mut output))
        .and_then(|_| output.sync_all());
    if let Err(error) = result {
        drop(output);
        let _ = fs::remove_file(&backup);
        return Err(format!("Back up {}: {error}", path.display()));
    }
    Ok(())
}

/// Preserve a pre-migration file once, including parent-directory durability.
pub fn retain_original(path: &Path) -> Result<(), String> {
    backup_original(path)?;
    if let Some(parent) = path.parent().filter(|parent| parent.exists()) {
        sync_directory(parent)?;
    }
    Ok(())
}

fn install_member(
    target: &Path,
    staged: &Path,
    old: &Option<String>,
    new: &str,
) -> Result<(), String> {
    let current = optional_digest(target)?;
    if current.as_deref() == Some(new) {
        if optional_digest(staged)?.is_some() {
            fs::remove_file(staged).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    if current != *old {
        return Err(format!(
            "{} changed during save migration; retained originals and staged files",
            target.display()
        ));
    }
    if optional_digest(staged)?.as_deref() != Some(new) {
        return Err(format!(
            "Missing or damaged staged save {}; retained originals",
            staged.display()
        ));
    }
    fs::rename(staged, target).map_err(|e| format!("Replace {}: {e}", target.display()))
}

/// Finish an interrupted pair replacement; refuse unrelated concurrent edits.
pub fn recover_state_pair(data: &Path, meta: &Path) -> Result<(), String> {
    let path = journal_path(data)?;
    if !path.exists() {
        return Ok(());
    }
    if regular_file(&path)?.len() > 4096 {
        return Err("Migration journal is too large".into());
    }
    let journal: PairJournal = serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Invalid migration journal {}: {e}", path.display()))?;
    if journal.version != 1 || journal.data != basename(data)? || journal.meta != basename(meta)? {
        return Err("Migration journal does not match the requested save pair".into());
    }
    let parent = pair_parent(data, meta)?;
    let staged_data = journal_member(&parent, &journal.staged_data)?;
    let staged_meta = journal_member(&parent, &journal.staged_meta)?;
    if !journal.staged_data.starts_with(".chronos-state-")
        || !journal.staged_meta.starts_with(".chronos-meta-")
    {
        return Err("Migration journal does not reference owned staged files".into());
    }
    // Check both destinations before changing either one.
    for (target, old, new) in [
        (data, &journal.old_data, &journal.new_data),
        (meta, &journal.old_meta, &journal.new_meta),
    ] {
        let current = optional_digest(target)?;
        if current != *old && current.as_deref() != Some(new.as_str()) {
            return Err(format!(
                "{} changed during save migration; retained originals",
                target.display()
            ));
        }
    }
    // Prove the completed pair before moving anything, including recovery
    // after only one member has reached its final pathname.
    let prospective_data = if optional_digest(data)?.as_deref() == Some(&journal.new_data) {
        data
    } else {
        &staged_data
    };
    let prospective_meta = if optional_digest(meta)?.as_deref() == Some(&journal.new_meta) {
        meta
    } else {
        &staged_meta
    };
    validate_state_pair(prospective_data, prospective_meta)?;
    install_member(data, &staged_data, &journal.old_data, &journal.new_data)?;
    install_member(meta, &staged_meta, &journal.old_meta, &journal.new_meta)?;
    validate_state_pair(data, meta)?;
    sync_directory(&parent)?;
    fs::remove_file(path).map_err(|e| e.to_string())?;
    sync_directory(&parent)
}

fn prepare_pair(
    src_data: &Path,
    src_meta: &Path,
    dst_data: &Path,
    dst_meta: &Path,
    new_tag: &str,
) -> Result<(), String> {
    let info = validate_state_pair(src_data, src_meta)?;
    if info.region_tag != new_tag && !valid_tag(new_tag) {
        return Err("Native game tag must contain 1–14 safe ASCII characters".into());
    }
    let parent = pair_parent(dst_data, dst_meta)?;
    fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let old_data = optional_digest(dst_data)?;
    let old_meta = optional_digest(dst_meta)?;
    let mut data = tempfile::Builder::new()
        .prefix(".chronos-state-")
        .tempfile_in(&parent)
        .map_err(|e| e.to_string())?;
    let mut input = File::open(src_data).map_err(|e| e.to_string())?;
    std::io::copy(&mut input, &mut data).map_err(|e| e.to_string())?;
    if file_digest(data.path())? != info.digest {
        return Err("Source save changed while it was being copied; original retained".into());
    }
    if info.region_tag != new_tag {
        let mut tag = [0; STATE_TAG_BYTES];
        tag[..new_tag.len()].copy_from_slice(new_tag.as_bytes());
        data.seek(SeekFrom::Start(STATE_TAG_OFFSET))
            .map_err(|e| e.to_string())?;
        data.write_all(&tag).map_err(|e| e.to_string())?;
    }
    data.as_file().sync_all().map_err(|e| e.to_string())?;
    let new_data = file_digest(data.path())?;
    let mut metadata = fs::read(src_meta).map_err(|e| e.to_string())?;
    let at = metadata_digest_offset(&metadata)?;
    if metadata[at..at + 16] != info.digest {
        return Err(
            "Source metadata changed while the save was being copied; original retained".into(),
        );
    }
    metadata[at..at + 16].copy_from_slice(&new_data);
    let mut meta = tempfile::Builder::new()
        .prefix(".chronos-meta-")
        .tempfile_in(&parent)
        .map_err(|e| e.to_string())?;
    meta.write_all(&metadata).map_err(|e| e.to_string())?;
    meta.as_file().sync_all().map_err(|e| e.to_string())?;
    validate_state_pair(data.path(), meta.path())?;
    backup_original(dst_data)?;
    backup_original(dst_meta)?;
    sync_directory(&parent)?;
    // Retain staged files once a durable journal describes their exact bytes.
    let staged_data = basename(data.path())?;
    let staged_meta = basename(meta.path())?;
    let journal = PairJournal {
        version: 1,
        data: basename(dst_data)?,
        meta: basename(dst_meta)?,
        staged_data,
        staged_meta,
        old_data,
        old_meta,
        new_data: digest_hex(&new_data),
        new_meta: digest_hex(&file_digest(meta.path())?),
    };
    let (_, data_path) = data.keep().map_err(|e| e.to_string())?;
    let (_, meta_path) = match meta.keep() {
        Ok(kept) => kept,
        Err(error) => {
            let _ = fs::remove_file(data_path);
            return Err(error.to_string());
        }
    };
    let journal_file = journal_path(dst_data)?;
    let journal_bytes = serde_json::to_vec(&journal).map_err(|e| e.to_string())?;
    if let Err(error) = atomic_write(&journal_file, &journal_bytes) {
        cleanup_unpublished_stages(&journal_file, &journal_bytes, &data_path, &meta_path);
        return Err(error);
    }
    Ok(())
}

fn cleanup_unpublished_stages(journal: &Path, expected: &[u8], data: &Path, meta: &Path) {
    // Persist can succeed before parent-directory fsync fails. Its journal
    // already owns these stages, so keep them available for recovery.
    let installed = match fs::symlink_metadata(journal) {
        Ok(m) if m.file_type().is_file() && m.len() == expected.len() as u64 => fs::read(journal)
            .map(|bytes| bytes == expected)
            .unwrap_or(true),
        Ok(_) => false,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
    };
    if !installed {
        let _ = fs::remove_file(data);
        let _ = fs::remove_file(meta);
    }
}

/// Retag only a proven native pair. Originals survive in .pre-chronos-v2 files.
pub fn retag_state_pair(
    data: &Path,
    meta: &Path,
    old_tag: &str,
    new_tag: &str,
) -> Result<bool, String> {
    recover_state_pair(data, meta)?;
    let info = validate_state_pair(data, meta)?;
    if info.region_tag == new_tag {
        return Ok(false);
    }
    if info.region_tag != old_tag {
        return Err(format!(
            "Save belongs to '{}', expected '{old_tag}'",
            info.region_tag
        ));
    }
    prepare_pair(data, meta, data, meta, new_tag)?;
    recover_state_pair(data, meta)?;
    Ok(true)
}

/// Import a validated pair without touching its source. Caller decides freshness.
pub fn import_state_pair(
    src_data: &Path,
    src_meta: &Path,
    dst_data: &Path,
    dst_meta: &Path,
    expected_tag: &str,
    destination_tag: &str,
) -> Result<bool, String> {
    recover_state_pair(dst_data, dst_meta)?;
    let info = validate_state_pair(src_data, src_meta)?;
    if info.region_tag != expected_tag {
        return Err(format!(
            "Save belongs to '{}', expected '{expected_tag}'",
            info.region_tag
        ));
    }
    if dst_data.is_file() && dst_meta.is_file() && regular_file(dst_data)?.len() != 0 {
        let previous = validate_state_pair(dst_data, dst_meta)?;
        if previous.digest == info.digest && previous.region_tag == destination_tag {
            return Ok(false);
        }
    }
    prepare_pair(src_data, src_meta, dst_data, dst_meta, destination_tag)?;
    recover_state_pair(dst_data, dst_meta)?;
    Ok(true)
}

/// Validate every canonical slot before any primary-tag migration. Alternate
/// ROM tags belong to this game but keep their original native payload tag.
pub fn validate_canonical_states(
    saves: &Path,
    native_tag: &str,
    legacy_primary_tag: Option<&str>,
    alternate_tags: &[&str],
    expected_segment: u16,
) -> Result<usize, String> {
    if !valid_tag(native_tag) || !matches!(expected_segment, 11 | 12) {
        return Err("Invalid canonical game save identity or layout".into());
    }
    if fs::symlink_metadata(saves).is_ok_and(|m| !m.file_type().is_dir()) {
        return Err(format!(
            "Expected a regular save directory: {}",
            saves.display()
        ));
    }
    let mut retags = Vec::new();
    for slot in 0..4 {
        let data = saves.join(format!("state_{slot}.bin"));
        let meta = saves.join(format!("state_{slot}_meta.bin"));
        recover_state_pair(&data, &meta)?;
        let size = |path: &Path| -> Result<Option<u64>, String> {
            match fs::symlink_metadata(path) {
                Ok(m) if m.file_type().is_file() => Ok(Some(m.len())),
                Ok(_) => Err(format!("Expected a regular save file: {}", path.display())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(format!("Inspect {}: {e}", path.display())),
            }
        };
        let data_size = size(&data)?;
        let meta_size = size(&meta)?;
        if data_size.is_none_or(|n| n == 0) && meta_size.is_none_or(|n| n == 0) {
            continue;
        }
        if data_size.is_none_or(|n| n == 0) || meta_size.is_none_or(|n| n == 0) {
            return Err(format!(
                "Incomplete canonical save slot {} in {}; original files retained",
                slot + 1,
                saves.display()
            ));
        }
        let state = validate_state_pair(&data, &meta)?;
        if state.segment_type != expected_segment {
            return Err(format!("Canonical save slot {} in {} uses a different HuCard/CD layout; original files retained", slot + 1, saves.display()));
        }
        if state.region_tag == native_tag || alternate_tags.contains(&state.region_tag.as_str()) {
            continue;
        }
        if legacy_primary_tag == Some(state.region_tag.as_str()) {
            retags.push((data, meta, state.region_tag));
        } else {
            return Err(format!("Canonical save slot {} in {} belongs to undeclared game tag '{}'; original files retained", slot + 1, saves.display(), state.region_tag));
        }
    }
    let mut changed = 0;
    for (data, meta, old) in retags {
        if retag_state_pair(&data, &meta, &old, native_tag)? {
            changed += 1;
        }
    }
    Ok(changed)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use indexmap::IndexMap;
    use m2_psb::{Stream, Value};

    pub(crate) fn fixture_pair(data: &Path, meta: &Path, tag: &str, segment: u16, marker: u8) {
        fs::create_dir_all(data.parent().unwrap()).unwrap();
        let mut file = File::create(data).unwrap();
        file.set_len(if segment == 11 {
            CD_STATE_SIZE
        } else {
            HUCARD_STATE_SIZE
        })
        .unwrap();
        file.write_all(&0x0001_0010u32.to_le_bytes()).unwrap();
        file.write_all(&0x0001_0000u32.to_le_bytes()).unwrap();
        file.seek(SeekFrom::Start(STATE_TAG_OFFSET)).unwrap();
        let mut raw = [0; STATE_TAG_BYTES];
        raw[..tag.len()].copy_from_slice(tag.as_bytes());
        file.write_all(&raw).unwrap();
        file.seek(SeekFrom::Start(0x100)).unwrap();
        file.write_all(&[marker]).unwrap();
        file.sync_all().unwrap();
        let tree = Value::Object(IndexMap::from([(
            "md5".into(),
            Value::Stream(Stream {
                index: 0,
                data: file_digest(data).unwrap().to_vec(),
            }),
        )]));
        fs::write(meta, m2_psb::write(&tree, 3).unwrap()).unwrap();
    }

    #[test]
    fn known_layouts_and_digests_are_required_before_retagging() {
        let dir = tempfile::tempdir().unwrap();
        let (data, meta) = (
            dir.path().join("state_0.bin"),
            dir.path().join("state_0_meta.bin"),
        );
        for segment in [11, 12] {
            fixture_pair(&data, &meta, "OLD", segment, 3);
            assert_eq!(
                validate_state_pair(&data, &meta).unwrap().segment_type,
                segment
            );
            let original = fs::read(&data).unwrap();
            fs::write(&meta, b"unrecognized").unwrap();
            assert!(retag_state_pair(&data, &meta, "OLD", "NEW").is_err());
            assert_eq!(fs::read(&data).unwrap(), original);
        }
    }

    #[test]
    fn retag_is_idempotent_and_retains_original_pair() {
        let dir = tempfile::tempdir().unwrap();
        let (data, meta) = (
            dir.path().join("state_3.bin"),
            dir.path().join("state_3_meta.bin"),
        );
        fixture_pair(&data, &meta, "OLD", 12, 7);
        let before = fs::read(&data).unwrap();
        assert!(retag_state_pair(&data, &meta, "OLD", "NEW").unwrap());
        assert_eq!(validate_state_pair(&data, &meta).unwrap().region_tag, "NEW");
        assert!(!retag_state_pair(&data, &meta, "OLD", "NEW").unwrap());
        assert_eq!(
            fs::read(dir.path().join("state_3.bin.pre-chronos-v2")).unwrap(),
            before
        );
        assert_eq!(
            validate_state_pair(
                &dir.path().join("state_3.bin.pre-chronos-v2"),
                &dir.path().join("state_3_meta.bin.pre-chronos-v2")
            )
            .unwrap()
            .region_tag,
            "OLD"
        );
        assert!(!dir.path().join("state_0.bin").exists());
    }

    #[test]
    fn validated_legacy_utf8_tags_are_preserved_but_new_tags_must_be_safe() {
        let dir = tempfile::tempdir().unwrap();
        let (data, meta) = (
            dir.path().join("state_0.bin"),
            dir.path().join("state_0_meta.bin"),
        );
        fixture_pair(&data, &meta, "旧 NAME", 12, 9);
        assert_eq!(
            validate_state_pair(&data, &meta).unwrap().region_tag,
            "旧 NAME"
        );
        let (copy, copy_meta) = (
            dir.path().join("state_1.bin"),
            dir.path().join("state_1_meta.bin"),
        );
        assert!(import_state_pair(&data, &meta, &copy, &copy_meta, "旧 NAME", "旧 NAME").unwrap());
        assert!(retag_state_pair(&data, &meta, "旧 NAME", "NEW NAME").is_err());
        assert!(retag_state_pair(&data, &meta, "旧 NAME", "G0123456789abc").unwrap());
        assert_eq!(
            validate_state_pair(&data, &meta).unwrap().region_tag,
            "G0123456789abc"
        );
    }

    #[test]
    fn interrupted_pair_replacement_finishes_from_durable_journal() {
        let dir = tempfile::tempdir().unwrap();
        let (data, meta) = (
            dir.path().join("state_1.bin"),
            dir.path().join("state_1_meta.bin"),
        );
        fixture_pair(&data, &meta, "OLD", 11, 5);
        prepare_pair(&data, &meta, &data, &meta, "NEW").unwrap();
        let journal: PairJournal =
            serde_json::from_slice(&fs::read(journal_path(&data).unwrap()).unwrap()).unwrap();
        fs::rename(dir.path().join(journal.staged_data), &data).unwrap();
        assert!(validate_state_pair(&data, &meta).is_err());
        recover_state_pair(&data, &meta).unwrap();
        assert_eq!(validate_state_pair(&data, &meta).unwrap().region_tag, "NEW");
        assert!(!journal_path(&data).unwrap().exists());
    }

    #[test]
    fn concurrent_changes_are_never_overwritten_during_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let (data, meta) = (
            dir.path().join("state_2.bin"),
            dir.path().join("state_2_meta.bin"),
        );
        fixture_pair(&data, &meta, "OLD", 12, 5);
        prepare_pair(&data, &meta, &data, &meta, "NEW").unwrap();
        fs::write(&meta, b"independent newer metadata").unwrap();
        let before = fs::read(&data).unwrap();
        assert!(recover_state_pair(&data, &meta).is_err());
        assert_eq!(fs::read(&data).unwrap(), before);
        assert!(journal_path(&data).unwrap().exists());
    }

    #[test]
    fn installed_journal_keeps_stages_even_when_commit_reports_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("state_0.bin");
        let meta = dir.path().join("state_0_meta.bin");
        fixture_pair(&data, &meta, "OLD", 12, 7);
        prepare_pair(&data, &meta, &data, &meta, "NEW").unwrap();
        let journal = journal_path(&data).unwrap();
        let bytes = fs::read(&journal).unwrap();
        let transaction: PairJournal = serde_json::from_slice(&bytes).unwrap();
        let staged_data = dir.path().join(transaction.staged_data);
        let staged_meta = dir.path().join(transaction.staged_meta);
        cleanup_unpublished_stages(&journal, &bytes, &staged_data, &staged_meta);
        assert!(staged_data.is_file() && staged_meta.is_file());
        recover_state_pair(&data, &meta).unwrap();
        assert_eq!(validate_state_pair(&data, &meta).unwrap().region_tag, "NEW");
    }

    #[test]
    fn canonical_store_checks_all_slots_before_retag_and_preserves_alternates() {
        let dir = tempfile::tempdir().unwrap();
        let saves = dir.path().join("saves");
        let data = saves.join("state_0.bin");
        let meta = saves.join("state_0_meta.bin");
        fixture_pair(&data, &meta, "OLD", 12, 1);
        let data3 = saves.join("state_3.bin");
        let meta3 = saves.join("state_3_meta.bin");
        fixture_pair(&data3, &meta3, "FOREIGN", 12, 3);
        let original = fs::read(&data).unwrap();
        assert!(validate_canonical_states(&saves, "NEW", Some("OLD"), &["EUR"], 12).is_err());
        assert_eq!(fs::read(&data).unwrap(), original);
        assert!(!saves.join("state_0.bin.pre-chronos-v2").exists());
        fixture_pair(&data3, &meta3, "EUR", 12, 3);
        assert_eq!(
            validate_canonical_states(&saves, "NEW", Some("OLD"), &["EUR"], 12).unwrap(),
            1
        );
        assert_eq!(
            validate_state_pair(&data3, &meta3).unwrap().region_tag,
            "EUR"
        );
        assert_eq!(
            validate_canonical_states(&saves, "NEW", Some("OLD"), &["EUR"], 12).unwrap(),
            0
        );
        assert!(!saves.join("state_1.bin").exists());
        assert!(!saves.join("state_2.bin").exists());
        fs::remove_file(&meta3).unwrap();
        assert!(
            validate_canonical_states(&saves, "NEW", Some("OLD"), &["EUR"], 12)
                .unwrap_err()
                .contains("Incomplete")
        );
        fixture_pair(&data3, &meta3, "EUR", 11, 3);
        assert!(
            validate_canonical_states(&saves, "NEW", Some("OLD"), &["EUR"], 12)
                .unwrap_err()
                .contains("layout")
        );
    }
}
