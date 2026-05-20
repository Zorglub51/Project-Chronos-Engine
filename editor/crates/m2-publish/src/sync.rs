// On-USB sync: published/ -> library/.
//
// Console writes engine-format files under `published/` during play.
// The editor's "Open library" step reorganises those into the per-game
// library view. No console contact (no SSH, no USB hot-swap detection).
// Just an on-disk reshuffle of bytes the console has been mutating.
//
// What's captured:
//   1. BACKUP_FLAGS bytes (data_008_0000.bin[0..0x5E80)) -> library/data_008_0000.bin
//      Language, theme, lineup, last selections, etc. — per-library user prefs.
//   2. SRAM blocks (data_008_0000.bin[0x5E80..+1.27MB) for the active pack,
//      published/folders/<lineup>/<dir>/saves/sram.bin for the others) ->
//      split into 150 x 8448 blocks; each non-zero block at position N goes
//      to library/<lineup>/[<folder>/]<gameDir>/sram.bin where gameDir is
//      resolved via the library's gamelist.json (assumed to match the order
//      that was published).
//   3. Save state files (data_TTT_SSSS.bin + meta_*.bin) -> per-game
//      library/.../<gameDir>/saves/state_<S%4>.bin (+ meta). Engine writes
//      these directly into the active pack's saves/; sync moves them into
//      the per-game library view.
//
// Identity is positional: for SRAM and state files, the game is determined
// by the slot/block index and the library's gamelist order for that pack.
// We trust that the user hasn't reordered the library between publish and
// sync — that's the editor's contract (sync runs on open, before any user
// edits). Save state files have an internal regionTag at offset 0x50 that
// we COULD verify, but for v1 we trust positional identity.

use crate::library::{Folder, Game, Lineup, LineupEntry};
use crate::Error;
use std::fs;
use std::path::{Path, PathBuf};

/// Bytes from start of data_008 to where SRAM begins. Equals SRAM_OFFSET.
const BACKUP_FLAGS_SIZE: usize = 0x5E80;
/// SRAM array start offset inside data_008_0000.bin. Matches m2hook_print.c.
const SRAM_OFFSET: usize = 0x5E80;
/// Per-game struct_sram_data size.
const SRAM_BLOCK_SIZE: usize = 8448;
const SRAM_GAME_COUNT: usize = 150;
const SRAM_SLICE_SIZE: usize = SRAM_BLOCK_SIZE * SRAM_GAME_COUNT;

const SAVES_PER_GAME: u16 = 4;
const LINEUP_SLOT_OFFSET_US: u16 = 200;

#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// `library/` root (sibling of `published/` on the USB stick).
    pub library_root: PathBuf,
    /// `published/` root.
    pub published_root: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    /// Whether `library/data_008_0000.bin` was written/refreshed.
    pub backup_flags_updated: bool,
    /// Per-game `sram.bin` files written (non-zero blocks only).
    pub sram_blocks_imported: usize,
    /// SRAM blocks at positions the library didn't have a game for. Skipped.
    pub sram_blocks_unmapped: usize,
    /// Per-game `state_N.bin` + meta pairs written.
    pub state_files_imported: usize,
    /// State files whose slot didn't map to a known library game. Skipped.
    pub state_files_unmapped: usize,
}

/// Top-level sync entry point. Reads `published/` and writes the per-game
/// `library/` view. Idempotent — safe to call repeatedly.
pub fn sync_library_from_published(opts: &SyncOptions) -> Result<SyncReport, Error> {
    let mut report = SyncReport::default();
    let library = crate::library::load(&opts.library_root)?;

    // 1. BACKUP_FLAGS: copy bytes [0..0x5E80) from published data_008 to library.
    sync_backup_flags(&opts.library_root, &opts.published_root, &mut report)?;

    // 2 + 3. Per pack: SRAM blocks and state files.
    let active_pack = read_current(&opts.published_root);
    for (lineup_name, lineup) in [("jp", &library.jp), ("us", &library.us)] {
        sync_pack(
            "_root",
            None,
            &lineup_root_games(lineup),
            lineup_name,
            lineup,
            &active_pack,
            &opts.library_root,
            &opts.published_root,
            &mut report,
        )?;
        for folder in lineup.folders() {
            sync_pack(
                &folder.dir_name,
                Some(folder),
                &folder.games.iter().collect::<Vec<_>>(),
                lineup_name,
                lineup,
                &active_pack,
                &opts.library_root,
                &opts.published_root,
                &mut report,
            )?;
        }
    }

    Ok(report)
}

/// Capture published `data_008_0000.bin` BACKUP_FLAGS bytes into the
/// library. The library file ends up containing exactly the first 0x5E80
/// bytes (no SRAM trailer — that's per-game).
fn sync_backup_flags(library_root: &Path, published_root: &Path, report: &mut SyncReport) -> Result<(), Error> {
    let src = published_root.join("game/save/data_008_0000.bin");
    if !src.exists() { return Ok(()); }
    let bytes = fs::read(&src)?;
    if bytes.len() < BACKUP_FLAGS_SIZE { return Ok(()); }
    let dst = library_root.join("data_008_0000.bin");
    fs::write(&dst, &bytes[..BACKUP_FLAGS_SIZE])?;
    report.backup_flags_updated = true;
    Ok(())
}

/// `(lineup, dir)` from `published/folders/.current`, or None.
fn read_current(published_root: &Path) -> Option<(String, String)> {
    let path = published_root.join("folders/.current");
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    let (lineup, dir) = trimmed.split_once('/')?;
    Some((lineup.to_string(), dir.to_string()))
}

/// Returns the root-pack's ordered games (excludes folder cards — those
/// have no SRAM/state). Folder cards just take up a position; sync skips
/// them by `arch=="folder"`.
fn lineup_root_games(lineup: &Lineup) -> Vec<&Game> {
    lineup.root_entries.iter().filter_map(|e| match e {
        LineupEntry::Game(g) => Some(g),
        LineupEntry::Folder(_) => None,
    }).collect()
}

/// Pack-level sync (root or folder). `pack_dir_name` is `_root` for the
/// lineup root, `FOLDER<id>` for folders.
#[allow(clippy::too_many_arguments)]
fn sync_pack(
    pack_dir_name: &str,
    folder: Option<&Folder>,
    pack_games: &[&Game],
    lineup_name: &str,
    lineup: &Lineup,
    active_pack: &Option<(String, String)>,
    library_root: &Path,
    published_root: &Path,
    report: &mut SyncReport,
) -> Result<(), Error> {
    let pack_dir = published_root.join("folders").join(lineup_name).join(pack_dir_name);
    let saves_dir = pack_dir.join("saves");
    if !saves_dir.exists() { return Ok(()); }

    // SRAM source: for active pack, slice out of live data_008; otherwise
    // read the pack's saves/sram.bin (written by hook on last swap-out).
    let is_active = active_pack.as_ref().map_or(false, |(l, d)| l == lineup_name && d == pack_dir_name);
    let sram_blob: Option<Vec<u8>> = if is_active {
        let data_008 = published_root.join("game/save/data_008_0000.bin");
        if data_008.exists() {
            let bytes = fs::read(&data_008)?;
            if bytes.len() >= SRAM_OFFSET + SRAM_SLICE_SIZE {
                Some(bytes[SRAM_OFFSET .. SRAM_OFFSET + SRAM_SLICE_SIZE].to_vec())
            } else { None }
        } else { None }
    } else {
        let sram_path = saves_dir.join("sram.bin");
        if sram_path.exists() {
            let bytes = fs::read(&sram_path)?;
            if bytes.len() >= SRAM_SLICE_SIZE { Some(bytes[..SRAM_SLICE_SIZE].to_vec()) }
            else { None }
        } else { None }
    };

    // For folder packs, slot N=0 is the back card (no game). Real games
    // start at slot 1 -> pack_games[0]. For root packs, no offset.
    let pack_pos_offset: usize = if folder.is_some() { 1 } else { 0 };

    if let Some(blob) = sram_blob.as_ref() {
        for slot in 0..SRAM_GAME_COUNT {
            let block = &blob[slot * SRAM_BLOCK_SIZE .. (slot + 1) * SRAM_BLOCK_SIZE];
            if block.iter().all(|&b| b == 0) { continue; }

            if slot < pack_pos_offset { continue; } // back card slot, no game
            let game_idx = slot - pack_pos_offset;
            if game_idx >= pack_games.len() {
                report.sram_blocks_unmapped += 1;
                continue;
            }
            let game = pack_games[game_idx];
            let dst = library_game_sram_path(library_root, lineup_name, folder, game);
            fs::write(&dst, block)?;
            report.sram_blocks_imported += 1;
        }
    }

    // State files: walk pack/saves/*.bin and route to per-game library/<gameDir>/saves/.
    let lineup_offset = if lineup_name == "us" { LINEUP_SLOT_OFFSET_US } else { 0 } as usize;
    for entry in fs::read_dir(&saves_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() { continue; }
        let name = entry.file_name();
        let name_str = match name.to_str() { Some(s) => s, None => continue };

        // Only data_011 / data_012 are state files; meta_* renamed alongside data_*.
        let (seg_type, slot) = match parse_state_filename(name_str) {
            Some(x) => x,
            None => continue,
        };
        let _ = seg_type; // for now we don't preserve type in library naming

        let local_slot = slot.saturating_sub(lineup_offset);
        let game_idx_pack = (local_slot / SAVES_PER_GAME as usize).saturating_sub(pack_pos_offset);
        let save_slot = local_slot % SAVES_PER_GAME as usize;
        if game_idx_pack >= pack_games.len() {
            report.state_files_unmapped += 1;
            continue;
        }
        let game = pack_games[game_idx_pack];
        let game_saves_dir = library_game_saves_dir(library_root, lineup_name, folder, game);
        fs::create_dir_all(&game_saves_dir)?;

        let dst_data = game_saves_dir.join(format!("state_{}.bin", save_slot));
        fs::copy(entry.path(), &dst_data)?;

        let src_meta = saves_dir.join(format!("meta_{:03}_{:04}.bin", seg_type, slot));
        if src_meta.exists() {
            let dst_meta = game_saves_dir.join(format!("state_{}_meta.bin", save_slot));
            fs::copy(&src_meta, &dst_meta)?;
        }
        report.state_files_imported += 1;
    }

    let _ = lineup;
    Ok(())
}

fn library_game_sram_path(library_root: &Path, lineup_name: &str, folder: Option<&Folder>, game: &Game) -> PathBuf {
    library_game_dir(library_root, lineup_name, folder, game).join("sram.bin")
}

fn library_game_saves_dir(library_root: &Path, lineup_name: &str, folder: Option<&Folder>, game: &Game) -> PathBuf {
    library_game_dir(library_root, lineup_name, folder, game).join("saves")
}

fn library_game_dir(library_root: &Path, lineup_name: &str, folder: Option<&Folder>, game: &Game) -> PathBuf {
    let base = library_root.join(lineup_name);
    let with_folder = match folder {
        Some(f) => base.join(&f.dir_name),
        None => base,
    };
    with_folder.join(&game.dir_name)
}

/// `data_TTT_SSSS.bin` -> (segment_type, slot). None if not a state file.
fn parse_state_filename(name: &str) -> Option<(u16, usize)> {
    let stem = name.strip_suffix(".bin")?;
    let body = stem.strip_prefix("data_")?;
    let (ttt, sss) = body.split_once('_')?;
    let t: u16 = ttt.parse().ok()?;
    if t != 11 && t != 12 { return None; }
    let s: usize = sss.parse().ok()?;
    Some((t, s))
}

