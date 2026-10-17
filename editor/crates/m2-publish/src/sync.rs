//! Import legacy saves from the last published PSB, never the edited ordering.
//! V2 writes directly beside game.json and does not need positional sync.
use crate::{
    library::{Game, Library, LineupEntry},
    save_format, Error,
};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

// Verified stock schema and STATRAM0 entries: no four-byte trailer follows it.
const SRAM_OFFSET: usize = 0x5E84;
const SRAM_BLOCK_SIZE: usize = 8448;
const SRAM_GAME_COUNT: usize = 150;
const SRAM_SLICE_SIZE: usize = SRAM_BLOCK_SIZE * SRAM_GAME_COUNT;
const MIGRATION_FILE: &str = ".chronos-save-migration-v2.json";

#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub library_root: PathBuf,
    pub published_root: PathBuf,
}
#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    pub backup_flags_updated: bool,
    pub sram_blocks_imported: usize,
    pub sram_blocks_unmapped: usize,
    pub state_files_imported: usize,
    pub state_files_unmapped: usize,
    pub lock_masks_imported: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Imported {
    digest: String,
    seconds: u64,
    nanos: u32,
}
impl Imported {
    fn new(digest: String, modified: SystemTime) -> Self {
        let time = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
        Self {
            digest,
            seconds: time.as_secs(),
            nanos: time.subsec_nanos(),
        }
    }
    fn suppresses(&self, digest: &str, time: SystemTime) -> bool {
        let next = Self::new(digest.into(), time);
        self.digest == digest || (next.seconds, next.nanos) <= (self.seconds, self.nanos)
    }
}
#[derive(Debug, Serialize, Deserialize)]
struct Migration {
    version: u32,
    #[serde(default)]
    states: BTreeMap<String, Imported>,
    #[serde(default)]
    sram: BTreeMap<String, Imported>,
}
impl Default for Migration {
    fn default() -> Self {
        Self {
            version: 1,
            states: BTreeMap::new(),
            sram: BTreeMap::new(),
        }
    }
}
struct IndexedGame<'a> {
    game: &'a Game,
    aliases: BTreeSet<String>,
}
struct PublishedItem {
    tag: String,
    id: Option<String>,
    tags: Vec<String>,
    is_game: bool,
}

fn index_games<'a>(library: &'a Library, root: &Path) -> Result<Vec<IndexedGame<'a>>, Error> {
    let mut out = Vec::new();
    let mut ids = BTreeSet::new();
    let canonical_root = root.canonicalize()?;
    for lineup in [&library.jp, &library.us] {
        for entry in &lineup.root_entries {
            let games: Vec<&Game> = match entry {
                LineupEntry::Game(game) => vec![game],
                LineupEntry::Folder(folder) => folder.games.iter().collect(),
            };
            for game in games {
                if game.save_state_type().is_none() {
                    continue;
                }
                if !game.source_dir.canonicalize()?.starts_with(&canonical_root) {
                    return Err(Error::Library(
                        "Game save directory lies outside the library".into(),
                    ));
                }
                if let Some(id) = game.game_id() {
                    if !ids.insert(id.to_owned()) {
                        return Err(Error::Library(format!(
                            "Duplicate game_id '{id}'; saves retained"
                        )));
                    }
                }
                let mut aliases = BTreeSet::from([game.region_tag.clone(), game.dir_name.clone()]);
                for tag in [
                    &game.data.rom.region_tag,
                    &game.data.rom_alt.region_tag_e,
                    &game.data.rom_alt.region_tag_s1,
                    &game.data.rom_alt.region_tag_s2,
                ]
                .into_iter()
                .flatten()
                {
                    if !tag.is_empty() && tag != "NO" {
                        aliases.insert(tag.clone());
                    }
                }
                out.push(IndexedGame { game, aliases });
            }
        }
    }
    Ok(out)
}
fn string(object: &indexmap::IndexMap<String, m2_psb::Value>, key: &str) -> Option<String> {
    match object.get(key) {
        Some(m2_psb::Value::String(s)) if !s.is_empty() && s != "NO" => Some(s.clone()),
        _ => None,
    }
}
fn published_items(pack: &Path, lineup: &str) -> Result<Option<Vec<PublishedItem>>, Error> {
    use m2_psb::Value;
    let path = pack.join("title_mode_top.psb.m");
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = m2_mzs::unpack_default(&fs::read(path)?, "title_mode_top.psb.m")?;
    let Value::Object(root) = m2_psb::read(&bytes)? else {
        return Err(Error::Template("Published menu is not an object".into()));
    };
    if matches!(root.get("chronos_save_schema"), Some(Value::Int(2))) {
        return Ok(None);
    }
    let count_key = if lineup == "us" {
        "titleNumTG"
    } else {
        "titleNum"
    };
    let count = match root.get(count_key) {
        Some(Value::Int(n)) if (0..=50).contains(n) => *n as usize,
        _ => return Err(Error::Template(format!("Invalid published {count_key}"))),
    };
    let Some(Value::Array(items)) = root.get("items") else {
        return Err(Error::Template("Published menu lacks items".into()));
    };
    let first = if lineup == "us" { 50 } else { 0 };
    if items.len() < first + count {
        return Err(Error::Template("Truncated published menu".into()));
    }
    let mut out = Vec::new();
    for item in &items[first..first + count] {
        let Value::Object(item) = item else {
            return Err(Error::Template("Invalid published item".into()));
        };
        let tag = string(item, "regionTag")
            .ok_or_else(|| Error::Template("Published item lacks regionTag".into()))?;
        let id = string(item, "game_id");
        if id
            .as_ref()
            .is_some_and(|id| !crate::identity::valid_game_id(id))
        {
            return Err(Error::Template("Invalid published game_id".into()));
        }
        let path = string(item, "save_path");
        if path.as_ref().is_some_and(|p| {
            Path::new(p)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        }) {
            return Err(Error::Template("Invalid published save_path".into()));
        }
        let mut tags = vec![tag.clone()];
        for key in ["regionTagE", "regionTagS1", "regionTagS2"] {
            if let Some(tag) = string(item, key) {
                tags.push(tag);
            }
        }
        let is_game = tag != "DUMMY"
            && !tag.starts_with("FOLDER_")
            && !matches!(item.get("csize"), Some(Value::Int(10 | 11)))
            && !matches!(item.get("save_type"), Some(Value::Int(0)));
        out.push(PublishedItem {
            tag,
            id,
            tags,
            is_game,
        });
    }
    Ok(Some(out))
}
fn resolve<'a>(item: &PublishedItem, games: &'a [IndexedGame<'a>]) -> Option<&'a IndexedGame<'a>> {
    if !item.is_game {
        return None;
    }
    let candidates: Vec<_> = if let Some(id) = &item.id {
        games
            .iter()
            .filter(|g| g.game.game_id() == Some(id.as_str()))
            .collect()
    } else {
        games
            .iter()
            .filter(|g| g.aliases.contains(&item.tag))
            .collect()
    };
    // A move must not bind saves to a new occupant of the old folder.
    if candidates.len() == 1 {
        Some(candidates[0])
    } else {
        None
    }
}

fn ambiguous_legacy(item: &PublishedItem, games: &[IndexedGame<'_>]) -> bool {
    item.id.is_none()
        && games
            .iter()
            .filter(|g| g.aliases.contains(&item.tag))
            .take(2)
            .count()
            > 1
}

fn ambiguous_error(item: &PublishedItem) -> Error {
    Error::Library(format!("Legacy saves for '{}' match multiple games. Resolve the duplicate legacy game tags before migration; original save archives were retained.", item.tag))
}
fn destination_tag(item: &PublishedItem, owner: &IndexedGame<'_>, payload: &str) -> Option<String> {
    if !item.tags.iter().any(|tag| tag == payload) && !owner.aliases.contains(payload) {
        return None;
    }
    let Some(native) = owner.game.data.native_region_tag.as_deref() else {
        return Some(payload.into());
    };
    if payload == item.tag
        || owner.game.data.rom.region_tag.as_deref() == Some(payload)
        || payload == native
    {
        return Some(native.into());
    }
    if [
        &owner.game.data.rom_alt.region_tag_e,
        &owner.game.data.rom_alt.region_tag_s1,
        &owner.game.data.rom_alt.region_tag_s2,
    ]
    .into_iter()
    .flatten()
    .any(|tag| tag == payload)
    {
        return Some(payload.into());
    }
    None
}
fn read_small(path: &Path, max: u64) -> Option<String> {
    let m = fs::symlink_metadata(path).ok()?;
    if !m.file_type().is_file() || m.len() > max {
        return None;
    }
    fs::read_to_string(path).ok()
}
fn live_directory(published: &Path) -> PathBuf {
    let canonical = published.join("save");
    if canonical.join("data_008_0000.bin").is_file() {
        canonical
    } else {
        published.join("game/save")
    }
}
fn live_valid(live: &Path) -> bool {
    let data = live.join("data_008_0000.bin");
    let Ok(mut input) = File::open(&data) else {
        return false;
    };
    if !input
        .metadata()
        .is_ok_and(|m| m.len() == (SRAM_OFFSET + SRAM_SLICE_SIZE) as u64)
    {
        return false;
    }
    let mut header = [0; 4];
    if input.read_exact(&mut header).is_err() || u32::from_le_bytes(header) != 0x0001_0018 {
        return false;
    }
    save_format::validate_data_digest(&data, &live.join("meta_008_0000.bin")).is_ok()
}
fn sync_flags(root: &Path, live: &Path, report: &mut SyncReport) -> Result<(), Error> {
    if !live_valid(live) {
        return Ok(());
    }
    let mut input = File::open(live.join("data_008_0000.bin"))?;
    if input.metadata()?.len() < SRAM_OFFSET as u64 {
        return Ok(());
    }
    let mut bytes = vec![0; SRAM_OFFSET];
    input.read_exact(&mut bytes)?;
    let target = root.join("data_008_0000.bin");
    if fs::read(&target).ok().as_deref() == Some(bytes.as_slice()) {
        return Ok(());
    }
    save_format::atomic_write(&target, &bytes).map_err(Error::Library)?;
    report.backup_flags_updated = true;
    Ok(())
}
fn fresh(source: SystemTime, target: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(target) {
        Ok(m) if m.file_type().is_file() => Ok(m.len() == 0 || source > m.modified()?),
        Ok(_) => Err(Error::Library(format!(
            "Refusing to replace non-file {}",
            target.display()
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(e.into()),
    }
}

fn migration_key(pack: &Path, item: &PublishedItem) -> String {
    // Keep completed migrations stable when the whole library is relocated.
    let lineup = pack
        .parent()
        .and_then(Path::file_name)
        .unwrap()
        .to_string_lossy();
    let folder = pack.file_name().unwrap().to_string_lossy();
    format!(
        "{lineup}/{folder}:{}",
        item.id.as_deref().unwrap_or(&item.tag)
    )
}
fn migrate_sram(
    pack: &Path,
    items: &[PublishedItem],
    games: &[IndexedGame<'_>],
    live: &Path,
    active: bool,
    report: &mut SyncReport,
    migration: &mut Migration,
) -> Result<(), Error> {
    let (source, offset) = if active {
        let expected = format!(
            "{}/{}",
            pack.parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy(),
            pack.file_name().unwrap().to_string_lossy()
        );
        if read_small(&live.join(".sram-pack"), 128)
            .as_deref()
            .map(str::trim)
            != Some(expected.as_str())
            || !live_valid(live)
        {
            return Ok(());
        }
        (live.join("data_008_0000.bin"), SRAM_OFFSET as u64)
    } else {
        // V1 snapshots began four bytes before the actual array. For every
        // real slot (at most 50), its missing tail survives in the next block.
        (pack.join("saves/sram.bin"), 4)
    };
    if !source.is_file() {
        return Ok(());
    }
    let mut input = File::open(&source)?;
    let m = input.metadata()?;
    if (active && m.len() != (SRAM_OFFSET + SRAM_SLICE_SIZE) as u64)
        || (!active && m.len() != SRAM_SLICE_SIZE as u64)
        || m.len() < offset + (items.len() * SRAM_BLOCK_SIZE) as u64
    {
        return Ok(());
    }
    let time = m.modified()?;
    input.seek(SeekFrom::Start(offset))?;
    let mut block = [0; SRAM_BLOCK_SIZE];
    for slot in 0..items.len().min(SRAM_GAME_COUNT) {
        input.read_exact(&mut block)?;
        let Some(item) = items.get(slot).filter(|i| i.is_game) else {
            continue;
        };
        let Some(owner) = resolve(item, games) else {
            if block.iter().any(|byte| *byte != 0) {
                if ambiguous_legacy(item, games) {
                    return Err(ambiguous_error(item));
                }
                report.sram_blocks_unmapped += 1;
            }
            continue;
        };
        let target = owner.game.sram_path();
        let schema = owner.game.saves_dir().join(".chronos-sram-schema");
        if read_small(&schema, 16).as_deref() == Some("2\n") {
            continue;
        }
        // A complete empty source proves alignment for an existing V1 file
        // too. Absent fresh SRAM needs no disk allocation.
        if block.iter().all(|byte| *byte == 0) && !target.exists() {
            continue;
        }
        let digest: [u8; 16] = Md5::digest(block).into();
        let digest = save_format::digest_hex(&digest);
        let key = migration_key(pack, item);
        if migration
            .sram
            .get(&key)
            .is_some_and(|r| r.suppresses(&digest, time))
        {
            continue;
        }
        if target.is_file() {
            if fs::metadata(&target)?.len() != SRAM_BLOCK_SIZE as u64 {
                return Err(Error::Library(format!(
                    "Unexpected SRAM size in {}; retained",
                    target.display()
                )));
            }
            if fs::read(&target)? == block {
                save_format::atomic_write(&schema, b"2\n").map_err(Error::Library)?;
                migration.sram.insert(key, Imported::new(digest, time));
                continue;
            }
        }
        // An unmarked per-game file is a V1 shifted slice, not a newer V2
        // source. Retain it before replacing it with a proven complete block.
        save_format::retain_original(&target).map_err(Error::Library)?;
        save_format::atomic_write(&target, &block).map_err(Error::Library)?;
        save_format::atomic_write(&schema, b"2\n").map_err(Error::Library)?;
        migration.sram.insert(key, Imported::new(digest, time));
        report.sram_blocks_imported += 1;
    }
    Ok(())
}

/// The original struct_backup_flags has ten leading uint32s, followed by
/// its lock fields in PSB lexical order. This preserves the unusual placement
/// of _100_lockdata60 before _10_lockdata00. The native flags begin at 0x980.
fn legacy_lock_offset(index: usize) -> Option<u64> {
    if index >= 100 {
        return None;
    }
    let mut fields: Vec<_> = (0..100)
        .map(|i| {
            let number = if i < 60 { i + 10 } else { i + 40 };
            (format!("_{number:02}_lockdata{i:02}"), i)
        })
        .collect();
    fields.sort_unstable();
    let position = fields.iter().position(|(_, i)| *i == index)?;
    Some(0x980 + ((10 + position) * 4) as u64)
}

fn migrate_locks(
    pack: &Path,
    lineup: &str,
    items: &[PublishedItem],
    games: &[IndexedGame<'_>],
    live: &Path,
    active: bool,
    report: &mut SyncReport,
) -> Result<(), Error> {
    if !active || !live_valid(live) {
        return Ok(());
    }
    let expected = format!("{lineup}/{}", pack.file_name().unwrap().to_string_lossy());
    if read_small(&live.join(".sram-pack"), 128)
        .as_deref()
        .map(str::trim)
        != Some(expected.as_str())
    {
        return Ok(());
    }
    // V1 locks were global and positional. Only the active pack's ownership
    // is established; assigning its flags to inactive packs would be guessing.
    let mut input = File::open(live.join("data_008_0000.bin"))?;
    for (position, item) in items.iter().enumerate().filter(|(_, i)| i.is_game) {
        let Some(owner) = resolve(item, games) else {
            continue;
        };
        let target = owner.game.saves_dir().join("locks.bin");
        if fs::symlink_metadata(&target).is_ok() {
            continue;
        } // Canonical mask is authoritative.
        let native_index = position + if lineup == "us" { 50 } else { 0 };
        let Some(offset) = legacy_lock_offset(native_index) else {
            continue;
        };
        let mut raw = [0; 4];
        input.seek(SeekFrom::Start(offset))?;
        input.read_exact(&mut raw)?;
        let value = u32::from_le_bytes(raw);
        if value > 15 {
            continue;
        } // Damaged/unsupported flags never become protection metadata.
        save_format::atomic_write(&target, &[value as u8]).map_err(Error::Library)?;
        report.lock_masks_imported += 1;
    }
    Ok(())
}
fn parse_state_name(name: &str) -> Option<(u16, usize)> {
    let body = name.strip_prefix("data_")?.strip_suffix(".bin")?;
    let (kind, number) = body.split_once('_')?;
    if kind.len() != 3 || number.len() < 4 || !number.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let kind = kind.parse().ok()?;
    if !matches!(kind, 11 | 12) {
        return None;
    }
    Some((kind, number.parse().ok()?))
}

fn report_unmapped_pack(
    pack: &Path,
    live: &Path,
    active: bool,
    report: &mut SyncReport,
) -> Result<(), Error> {
    let mut dirs = vec![pack.join("saves")];
    if active {
        dirs.push(live.into());
    }
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && entry.metadata()?.len() != 0
                && entry
                    .file_name()
                    .to_str()
                    .and_then(parse_state_name)
                    .is_some()
            {
                report.state_files_unmapped += 1;
            }
        }
    }
    let sram = pack.join("saves/sram.bin");
    if sram.is_file() && fs::metadata(&sram)?.len() != 0 {
        // The number of owners is unknowable without the published catalogue.
        report.sram_blocks_unmapped += 1;
    }
    Ok(())
}
fn migrate_states(
    pack: &Path,
    lineup: &str,
    items: &[PublishedItem],
    games: &[IndexedGame<'_>],
    live: &Path,
    active: bool,
    report: &mut SyncReport,
    migration: &mut Migration,
) -> Result<(), Error> {
    let offset = if lineup == "us" { 200 } else { 0 };
    let mut dirs = vec![pack.join("saves")];
    if active {
        dirs.push(live.into());
    }
    type Candidate = (PathBuf, PathBuf, save_format::StateInfo, SystemTime);
    let mut candidates: BTreeMap<usize, Vec<Candidate>> = BTreeMap::new();
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() || entry.metadata()?.len() == 0 {
                continue;
            }
            let Some((kind, native)) = entry.file_name().to_str().and_then(parse_state_name) else {
                continue;
            };
            let Some(local) = native.checked_sub(offset) else {
                report.state_files_unmapped += 1;
                continue;
            };
            let Some(item) = items.get(local / 4).filter(|i| i.is_game) else {
                continue;
            };
            let meta = entry
                .path()
                .with_file_name(format!("meta_{kind:03}_{native:04}.bin"));
            let info = match save_format::validate_state_pair(&entry.path(), &meta) {
                Ok(info) if info.segment_type == kind => info,
                _ => {
                    report.state_files_unmapped += 1;
                    continue;
                }
            };
            let Some(owner) = resolve(item, games) else {
                if ambiguous_legacy(item, games)
                    && item.tags.iter().any(|tag| *tag == info.region_tag)
                {
                    return Err(ambiguous_error(item));
                }
                report.state_files_unmapped += 1;
                continue;
            };
            if destination_tag(item, owner, &info.region_tag).is_none() {
                report.state_files_unmapped += 1;
                continue;
            }
            let time = fs::metadata(&meta)?.modified()?;
            candidates
                .entry(local)
                .or_default()
                .push((entry.path(), meta, info, time));
        }
    }
    for (local, mut sources) in candidates {
        let item = &items[local / 4];
        let owner = resolve(item, games).unwrap();
        if sources
            .iter()
            .map(|(_, _, info, _)| info.segment_type)
            .collect::<BTreeSet<_>>()
            .len()
            != 1
        {
            report.state_files_unmapped += sources.len();
            continue;
        }
        sources.sort_by_key(|(_, _, _, time)| *time);
        let (source, source_meta, info, time) = sources.pop().unwrap();
        let slot = local % 4;
        let data = owner.game.saves_dir().join(format!("state_{slot}.bin"));
        let meta = owner
            .game
            .saves_dir()
            .join(format!("state_{slot}_meta.bin"));
        let digest = save_format::digest_hex(&info.digest);
        let key = format!("{}:{slot}", migration_key(pack, item));
        if migration
            .states
            .get(&key)
            .is_some_and(|r| r.suppresses(&digest, time))
        {
            continue;
        }
        save_format::recover_state_pair(&data, &meta).map_err(Error::Library)?;
        let tag = destination_tag(item, owner, &info.region_tag).unwrap();
        if data.is_file() && fs::metadata(&data)?.len() != 0 {
            let previous =
                save_format::validate_state_pair(&data, &meta).map_err(Error::Library)?;
            if destination_tag(item, owner, &previous.region_tag).is_none() {
                return Err(Error::Library(format!("Canonical save {} belongs to unexpected game tag '{}'; source and destination retained.", data.display(), previous.region_tag)));
            }
            if previous.digest == info.digest {
                if previous.region_tag != tag
                    && save_format::retag_state_pair(&data, &meta, &previous.region_tag, &tag)
                        .map_err(Error::Library)?
                {
                    report.state_files_imported += 1;
                }
                migration.states.insert(key, Imported::new(digest, time));
                continue;
            }
            if !fresh(time, &meta)? {
                // Remember the rejected legacy snapshot too: deleting the
                // canonical newer slot must not bring the old one back.
                migration.states.insert(key, Imported::new(digest, time));
                continue;
            }
        }
        if save_format::import_state_pair(
            &source,
            &source_meta,
            &data,
            &meta,
            &info.region_tag,
            &tag,
        )
        .map_err(Error::Library)?
        {
            report.state_files_imported += 1;
        }
        migration.states.insert(key, Imported::new(digest, time));
    }
    Ok(())
}
pub fn sync_library_from_published(options: &SyncOptions) -> Result<SyncReport, Error> {
    let mut report = SyncReport::default();
    let live = live_directory(&options.published_root);
    sync_flags(&options.library_root, &live, &mut report)?;
    if read_small(&options.published_root.join(".per-game-saves-v2"), 16).as_deref() == Some("2\n")
    {
        return Ok(report);
    }
    let library = crate::library::load(&options.library_root)?;
    let games = index_games(&library, &options.library_root)?;
    let active = read_small(&options.published_root.join("folders/.current"), 128);
    let manifest = options.library_root.join(MIGRATION_FILE);
    let mut migration = if manifest.is_file() {
        if fs::metadata(&manifest)?.len() > 1_048_576 {
            return Err(Error::Library("Save migration manifest too large".into()));
        }
        let m: Migration = serde_json::from_slice(&fs::read(&manifest)?)?;
        if m.version != 1 {
            return Err(Error::Library("Unsupported save migration manifest".into()));
        }
        m
    } else {
        Migration::default()
    };
    for lineup in ["jp", "us"] {
        let dir = options.published_root.join("folders").join(lineup);
        if !dir.is_dir() {
            continue;
        }
        for pack in fs::read_dir(dir)? {
            let pack = pack?;
            if !pack.file_type()?.is_dir() {
                continue;
            }
            let expected = format!("{lineup}/{}", pack.file_name().to_string_lossy());
            let is_active = active.as_deref().map(str::trim) == Some(expected.as_str());
            if !pack.path().join("title_mode_top.psb.m").is_file() {
                report_unmapped_pack(&pack.path(), &live, is_active, &mut report)?;
                continue;
            }
            let Some(items) = published_items(&pack.path(), lineup)? else {
                continue;
            };
            migrate_sram(
                &pack.path(),
                &items,
                &games,
                &live,
                is_active,
                &mut report,
                &mut migration,
            )?;
            migrate_states(
                &pack.path(),
                lineup,
                &items,
                &games,
                &live,
                is_active,
                &mut report,
                &mut migration,
            )?;
            migrate_locks(
                &pack.path(),
                lineup,
                &items,
                &games,
                &live,
                is_active,
                &mut report,
            )?;
            save_format::atomic_write(&manifest, &serde_json::to_vec_pretty(&migration)?)
                .map_err(Error::Library)?;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save_format::tests::fixture_pair;
    use indexmap::IndexMap;
    use m2_psb::{Stream, Value};
    struct Fixture(tempfile::TempDir);
    impl Fixture {
        fn new() -> Self {
            let t = tempfile::tempdir().unwrap();
            for l in ["jp", "us"] {
                fs::create_dir(t.path().join(l)).unwrap();
                fs::write(t.path().join(l).join("gamelist.json"), b"[]").unwrap();
            }
            fs::create_dir_all(t.path().join("published/save")).unwrap();
            Self(t)
        }
        fn root(&self) -> &Path {
            self.0.path()
        }
        fn game(&self, p: &str, tag: &str) {
            fs::create_dir_all(self.root().join(p)).unwrap();
            fs::write(
                self.root().join(p).join("game.json"),
                serde_json::to_vec(
                    &serde_json::json!({"rom":{"rom":"test.pce","arch":"tg16","regionTag":tag}}),
                )
                .unwrap(),
            )
            .unwrap();
        }
        fn list(&self, p: &str, names: &[&str]) {
            fs::create_dir_all(self.root().join(p)).unwrap();
            fs::write(
                self.root().join(p).join("gamelist.json"),
                serde_json::to_vec(
                    &names
                        .iter()
                        .map(|n| serde_json::json!({"folder":n}))
                        .collect::<Vec<_>>(),
                )
                .unwrap(),
            )
            .unwrap();
        }
        fn pack(&self, lineup: &str, name: &str, tags: &[&str]) -> PathBuf {
            let p = self
                .root()
                .join("published/folders")
                .join(lineup)
                .join(name);
            fs::create_dir_all(p.join("saves")).unwrap();
            let start = if lineup == "us" { 50 } else { 0 };
            let mut items = vec![
                Value::Object(IndexMap::from([(
                    "regionTag".into(),
                    Value::String("DUMMY".into())
                )]));
                start + tags.len()
            ];
            for (i, tag) in tags.iter().enumerate() {
                items[start + i] = Value::Object(IndexMap::from([(
                    "regionTag".into(),
                    Value::String((*tag).into()),
                )]));
            }
            let tree = Value::Object(IndexMap::from([
                (
                    "titleNum".into(),
                    Value::Int(if lineup == "jp" { tags.len() as i64 } else { 0 }),
                ),
                (
                    "titleNumTG".into(),
                    Value::Int(if lineup == "us" { tags.len() as i64 } else { 0 }),
                ),
                ("items".into(), Value::Array(items)),
            ]));
            fs::write(
                p.join("title_mode_top.psb.m"),
                m2_mzs::pack_default(&m2_psb::write(&tree, 4).unwrap(), "title_mode_top.psb.m")
                    .unwrap(),
            )
            .unwrap();
            p
        }
        fn sync(&self) -> SyncReport {
            sync_library_from_published(&SyncOptions {
                library_root: self.root().into(),
                published_root: self.root().join("published"),
            })
            .unwrap()
        }
    }
    #[test]
    fn last_published_mapping_survives_reorder_move_and_sparse_slots() {
        let f = Fixture::new();
        f.game("jp/GAME_B", "B");
        f.game("us/GAME_A", "A");
        f.list("jp", &["GAME_B"]);
        f.list("us", &["GAME_A"]);
        let pack = f.pack("jp", "_root", &["A", "B"]);
        for s in [0, 3] {
            fixture_pair(
                &pack.join(format!("saves/data_012_{s:04}.bin")),
                &pack.join(format!("saves/meta_012_{s:04}.bin")),
                "A",
                12,
                s as u8,
            );
        }
        let mut sram = vec![0; SRAM_SLICE_SIZE];
        sram[4..4 + SRAM_BLOCK_SIZE].fill(17);
        fs::write(pack.join("saves/sram.bin"), sram).unwrap();
        let report = f.sync();
        assert_eq!(report.state_files_imported, 2);
        assert_eq!(report.sram_blocks_imported, 1);
        for s in [0, 3] {
            assert_eq!(
                save_format::inspect_state(
                    &f.root().join(format!("us/GAME_A/saves/state_{s}.bin"))
                )
                .unwrap()
                .0,
                "A"
            );
        }
        for s in [1, 2] {
            assert!(!f
                .root()
                .join(format!("us/GAME_A/saves/state_{s}.bin"))
                .exists());
        }
        assert!(!f.root().join("jp/GAME_B/saves").exists());
        assert_eq!(
            fs::read(f.root().join("us/GAME_A/sram.bin")).unwrap(),
            vec![17; SRAM_BLOCK_SIZE]
        );
    }
    #[test]
    fn us_offset_back_cards_and_live_placeholders_do_not_shift_slots() {
        let f = Fixture::new();
        f.game("us/GAME_A", "A");
        f.list("us", &["GAME_A"]);
        let p = f.pack("us", "FOLDER_OLD", &["FOLDER_us_BACK", "A"]);
        fs::write(
            f.root().join("published/folders/.current"),
            b"us/FOLDER_OLD\n",
        )
        .unwrap();
        fixture_pair(
            &p.join("saves/data_012_0206.bin"),
            &p.join("saves/meta_012_0206.bin"),
            "A",
            12,
            2,
        );
        fs::write(f.root().join("published/save/data_012_0206.bin"), b"").unwrap();
        fixture_pair(
            &p.join("saves/data_012_0200.bin"),
            &p.join("saves/meta_012_0200.bin"),
            "BACK",
            12,
            0,
        );
        assert_eq!(f.sync().state_files_imported, 1);
        assert!(f.root().join("us/GAME_A/saves/state_2.bin").is_file());
        assert!(!f.root().join("us/GAME_A/saves/state_0.bin").exists());
    }
    #[test]
    fn ambiguous_tags_and_foreign_payloads_remain_untouched() {
        let f = Fixture::new();
        f.game("jp/GAME_A", "SAME");
        f.game("us/GAME_B", "SAME");
        f.list("jp", &["GAME_A"]);
        f.list("us", &["GAME_B"]);
        let p = f.pack("jp", "_root", &["SAME"]);
        fixture_pair(
            &p.join("saves/data_012_0000.bin"),
            &p.join("saves/meta_012_0000.bin"),
            "SAME",
            12,
            3,
        );
        let source = fs::read(p.join("saves/data_012_0000.bin")).unwrap();
        let error = sync_library_from_published(&SyncOptions {
            library_root: f.root().into(),
            published_root: f.root().join("published"),
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("match multiple games"));
        assert_eq!(fs::read(p.join("saves/data_012_0000.bin")).unwrap(), source);
        assert!(!f.root().join("jp/GAME_A/saves").exists());
        fs::remove_file(p.join("saves/data_012_0000.bin")).unwrap();
        fs::remove_file(p.join("saves/meta_012_0000.bin")).unwrap();
        let mut sram = vec![0; SRAM_SLICE_SIZE];
        sram[4] = 5;
        fs::write(p.join("saves/sram.bin"), &sram).unwrap();
        assert!(sync_library_from_published(&SyncOptions {
            library_root: f.root().into(),
            published_root: f.root().join("published"),
        })
        .unwrap_err()
        .to_string()
        .contains("match multiple games"));
        assert_eq!(fs::read(p.join("saves/sram.bin")).unwrap(), sram);
        assert!(!f.root().join("us/GAME_B/sram.bin").exists());
        fs::remove_file(p.join("saves/sram.bin")).unwrap();
        f.list("us", &[]);
        fixture_pair(
            &p.join("saves/data_012_0000.bin"),
            &p.join("saves/meta_012_0000.bin"),
            "FOREIGN",
            12,
            3,
        );
        assert_eq!(f.sync().state_files_imported, 0);
    }

    #[test]
    fn absent_published_catalogue_retains_saves_and_reports_missing_ownership() {
        let f = Fixture::new();
        f.game("jp/GAME_A", "A");
        f.list("jp", &["GAME_A"]);
        let p = f.pack("jp", "_root", &["A"]);
        fixture_pair(
            &p.join("saves/data_012_0000.bin"),
            &p.join("saves/meta_012_0000.bin"),
            "A",
            12,
            3,
        );
        fs::remove_file(p.join("title_mode_top.psb.m")).unwrap();
        let before = fs::read(p.join("saves/data_012_0000.bin")).unwrap();
        let report = f.sync();
        assert_eq!(report.state_files_imported, 0);
        assert_eq!(report.state_files_unmapped, 1);
        assert_eq!(fs::read(p.join("saves/data_012_0000.bin")).unwrap(), before);
        assert!(!f.root().join("jp/GAME_A/saves").exists());
    }
    #[test]
    fn stale_legacy_files_and_v2_marker_cannot_resurrect_deleted_canonical_slot() {
        let f = Fixture::new();
        f.game("jp/GAME_A", "A");
        f.list("jp", &["GAME_A"]);
        let p = f.pack("jp", "_root", &["A"]);
        fixture_pair(
            &p.join("saves/data_012_0001.bin"),
            &p.join("saves/meta_012_0001.bin"),
            "A",
            12,
            3,
        );
        assert_eq!(f.sync().state_files_imported, 1);
        fs::remove_file(f.root().join("jp/GAME_A/saves/state_1.bin")).unwrap();
        fs::remove_file(f.root().join("jp/GAME_A/saves/state_1_meta.bin")).unwrap();
        assert_eq!(f.sync().state_files_imported, 0);
        // Relocating the published directory cannot reset migration history.
        fs::rename(f.root().join("published"), f.root().join("published-moved")).unwrap();
        assert_eq!(
            sync_library_from_published(&SyncOptions {
                library_root: f.root().into(),
                published_root: f.root().join("published-moved"),
            })
            .unwrap()
            .state_files_imported,
            0
        );
        fs::rename(f.root().join("published-moved"), f.root().join("published")).unwrap();
        fs::write(f.root().join("published/.per-game-saves-v2"), b"2\n").unwrap();
        fixture_pair(
            &p.join("saves/data_012_0001.bin"),
            &p.join("saves/meta_012_0001.bin"),
            "A",
            12,
            9,
        );
        assert_eq!(f.sync().state_files_imported, 0);
        assert!(!f.root().join("jp/GAME_A/saves/state_1.bin").exists());
    }
    #[test]
    fn active_sram_requires_matching_owner_and_digest() {
        let f = Fixture::new();
        f.game("jp/GAME_A", "A");
        f.list("jp", &["GAME_A"]);
        f.pack("jp", "_root", &["A"]);
        fs::write(f.root().join("published/folders/.current"), b"jp/_root\n").unwrap();
        let p = f.root().join("published/save");
        let mut bytes = vec![0; SRAM_OFFSET + SRAM_SLICE_SIZE];
        bytes[..4].copy_from_slice(&0x0001_0018u32.to_le_bytes());
        bytes[SRAM_OFFSET..SRAM_OFFSET + SRAM_BLOCK_SIZE].fill(9);
        fs::write(p.join("data_008_0000.bin"), &bytes).unwrap();
        let meta = Value::Object(IndexMap::from([(
            "md5".into(),
            Value::Stream(Stream {
                index: 0,
                data: Md5::digest(&bytes).to_vec(),
            }),
        )]));
        fs::write(
            p.join("meta_008_0000.bin"),
            m2_psb::write(&meta, 3).unwrap(),
        )
        .unwrap();
        assert_eq!(f.sync().sram_blocks_imported, 0);
        fs::write(p.join(".sram-pack"), b"us/_root\n").unwrap();
        assert_eq!(f.sync().sram_blocks_imported, 0);
        fs::write(p.join(".sram-pack"), b"jp/_root\n").unwrap();
        assert_eq!(f.sync().sram_blocks_imported, 1);
        bytes[0] = 6;
        fs::write(p.join("data_008_0000.bin"), bytes).unwrap();
        assert_eq!(f.sync().sram_blocks_imported, 0);
        assert_eq!(
            fs::read(f.root().join("jp/GAME_A/sram.bin")).unwrap(),
            vec![9; SRAM_BLOCK_SIZE]
        );
    }

    #[test]
    fn stable_id_disambiguates_a_moved_game_and_retags_only_validated_pair() {
        let f = Fixture::new();
        f.game("jp/GAME_B", "SAME");
        f.game("us/GAME_A", "SAME");
        f.list("jp", &["GAME_B"]);
        f.list("us", &["GAME_A"]);
        let id = "0123456789abcdef0123456789abcdef";
        let path = f.root().join("us/GAME_A/game.json");
        let mut json: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        json["game_id"] = id.into();
        json["native_region_tag"] = "NATIVE_A".into();
        fs::write(path, serde_json::to_vec(&json).unwrap()).unwrap();
        let pack = f.pack("jp", "_root", &["SAME"]);
        let packed = pack.join("title_mode_top.psb.m");
        let bytes =
            m2_mzs::unpack_default(&fs::read(&packed).unwrap(), "title_mode_top.psb.m").unwrap();
        let mut tree = m2_psb::read(&bytes).unwrap();
        if let Value::Object(root) = &mut tree {
            if let Some(Value::Array(items)) = root.get_mut("items") {
                if let Value::Object(item) = &mut items[0] {
                    item.insert("game_id".into(), Value::String(id.into()));
                }
            }
        }
        fs::write(
            packed,
            m2_mzs::pack_default(&m2_psb::write(&tree, 4).unwrap(), "title_mode_top.psb.m")
                .unwrap(),
        )
        .unwrap();
        fixture_pair(
            &pack.join("saves/data_012_0003.bin"),
            &pack.join("saves/meta_012_0003.bin"),
            "SAME",
            12,
            3,
        );
        // A byte-identical legacy canonical slot still needs its explicit
        // native identity migrated, independently of metadata freshness.
        fixture_pair(
            &f.root().join("us/GAME_A/saves/state_3.bin"),
            &f.root().join("us/GAME_A/saves/state_3_meta.bin"),
            "SAME",
            12,
            3,
        );
        assert_eq!(f.sync().state_files_imported, 1);
        assert_eq!(
            save_format::validate_state_pair(
                &f.root().join("us/GAME_A/saves/state_3.bin"),
                &f.root().join("us/GAME_A/saves/state_3_meta.bin")
            )
            .unwrap()
            .region_tag,
            "NATIVE_A"
        );
        assert_eq!(
            save_format::inspect_state(&pack.join("saves/data_012_0003.bin"))
                .unwrap()
                .0,
            "SAME"
        );
        assert!(!f.root().join("jp/GAME_B/saves").exists());
        assert!(f
            .root()
            .join("us/GAME_A/saves/state_3.bin.pre-chronos-v2")
            .exists());
    }

    #[test]
    fn newer_canonical_metadata_wins_over_legacy_source() {
        let f = Fixture::new();
        f.game("jp/GAME_A", "A");
        f.list("jp", &["GAME_A"]);
        let p = f.pack("jp", "_root", &["A"]);
        fixture_pair(
            &p.join("saves/data_012_0000.bin"),
            &p.join("saves/meta_012_0000.bin"),
            "A",
            12,
            3,
        );
        let data = f.root().join("jp/GAME_A/saves/state_0.bin");
        let meta = f.root().join("jp/GAME_A/saves/state_0_meta.bin");
        fixture_pair(&data, &meta, "A", 12, 9);
        File::options()
            .write(true)
            .open(p.join("saves/meta_012_0000.bin"))
            .unwrap()
            .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(10))
            .unwrap();
        File::options()
            .write(true)
            .open(&meta)
            .unwrap()
            .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(20))
            .unwrap();
        let before = fs::read(&data).unwrap();
        assert_eq!(f.sync().state_files_imported, 0);
        assert_eq!(fs::read(&data).unwrap(), before);
        fs::remove_file(&data).unwrap();
        fs::remove_file(&meta).unwrap();
        assert_eq!(f.sync().state_files_imported, 0);
        assert!(!data.exists());
    }

    #[test]
    fn identical_canonical_state_is_recorded_before_user_deletes_it() {
        let f = Fixture::new();
        f.game("jp/GAME_A", "A");
        f.list("jp", &["GAME_A"]);
        let p = f.pack("jp", "_root", &["A"]);
        fixture_pair(
            &p.join("saves/data_012_0000.bin"),
            &p.join("saves/meta_012_0000.bin"),
            "A",
            12,
            3,
        );
        let data = f.root().join("jp/GAME_A/saves/state_0.bin");
        let meta = f.root().join("jp/GAME_A/saves/state_0_meta.bin");
        fixture_pair(&data, &meta, "A", 12, 3);
        assert_eq!(f.sync().state_files_imported, 0);
        fs::remove_file(&data).unwrap();
        fs::remove_file(&meta).unwrap();
        assert_eq!(f.sync().state_files_imported, 0);
        assert!(!data.exists());
    }

    #[test]
    fn shifted_legacy_sram_tail_is_recovered_and_aligned_file_becomes_authoritative() {
        let f = Fixture::new();
        f.game("jp/GAME_A", "A");
        f.list("jp", &["GAME_A"]);
        let p = f.pack("jp", "_root", &["A"]);
        let mut legacy = vec![0; SRAM_SLICE_SIZE];
        legacy[4..4 + SRAM_BLOCK_SIZE].fill(7);
        legacy[SRAM_BLOCK_SIZE..SRAM_BLOCK_SIZE + 4].fill(0xee);
        fs::write(p.join("saves/sram.bin"), &legacy).unwrap();
        let target = f.root().join("jp/GAME_A/sram.bin");
        fs::write(&target, &legacy[..SRAM_BLOCK_SIZE]).unwrap();
        assert_eq!(f.sync().sram_blocks_imported, 1);
        assert_eq!(fs::read(&target).unwrap(), legacy[4..4 + SRAM_BLOCK_SIZE]);
        assert_eq!(
            fs::read(f.root().join("jp/GAME_A/sram.bin.pre-chronos-v2")).unwrap(),
            legacy[..SRAM_BLOCK_SIZE]
        );
        assert_eq!(
            fs::read(f.root().join("jp/GAME_A/saves/.chronos-sram-schema")).unwrap(),
            b"2\n"
        );
        legacy[4] = 42;
        fs::write(p.join("saves/sram.bin"), legacy).unwrap();
        assert_eq!(f.sync().sram_blocks_imported, 0);
        assert_eq!(fs::read(target).unwrap()[0], 7);
    }

    #[test]
    fn complete_empty_legacy_sram_marks_existing_file_but_does_not_allocate_fresh_files() {
        let f = Fixture::new();
        f.game("jp/GAME_A", "A");
        f.game("jp/GAME_B", "B");
        f.list("jp", &["GAME_A", "GAME_B"]);
        let p = f.pack("jp", "_root", &["A", "B"]);
        fs::write(p.join("saves/sram.bin"), vec![0; SRAM_SLICE_SIZE]).unwrap();
        fs::write(
            f.root().join("jp/GAME_A/sram.bin"),
            vec![0; SRAM_BLOCK_SIZE],
        )
        .unwrap();
        assert_eq!(f.sync().sram_blocks_imported, 0);
        assert_eq!(
            fs::read(f.root().join("jp/GAME_A/saves/.chronos-sram-schema")).unwrap(),
            b"2\n"
        );
        assert!(!f.root().join("jp/GAME_B/sram.bin").exists());
    }

    #[test]
    fn active_lock_mask_follows_published_identity_after_move() {
        let f = Fixture::new();
        f.game("us/GAME_A", "A");
        f.list("us", &["GAME_A"]);
        f.pack("jp", "_root", &["FOLDER_jp_OLD", "A"]);
        fs::write(f.root().join("published/folders/.current"), b"jp/_root\n").unwrap();
        let p = f.root().join("published/save");
        let mut data = vec![0; SRAM_OFFSET + SRAM_SLICE_SIZE];
        data[..4].copy_from_slice(&0x0001_0018u32.to_le_bytes());
        // Original schema _11_lockdata01 is at native 0x9fc, not index*4.
        data[0x9fc..0xa00].copy_from_slice(&9u32.to_le_bytes());
        fs::write(p.join("data_008_0000.bin"), &data).unwrap();
        let meta = Value::Object(IndexMap::from([(
            "md5".into(),
            Value::Stream(Stream {
                index: 0,
                data: Md5::digest(&data).to_vec(),
            }),
        )]));
        fs::write(
            p.join("meta_008_0000.bin"),
            m2_psb::write(&meta, 3).unwrap(),
        )
        .unwrap();
        fs::write(p.join(".sram-pack"), b"jp/_root\n").unwrap();
        assert_eq!(f.sync().lock_masks_imported, 1);
        let target = f.root().join("us/GAME_A/saves/locks.bin");
        assert_eq!(fs::read(&target).unwrap(), [9]);
        fs::write(&target, [3]).unwrap();
        assert_eq!(f.sync().lock_masks_imported, 0);
        assert_eq!(fs::read(target).unwrap(), [3]);
    }
}
