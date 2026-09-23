// Publish pipeline. Reads the library, emits the deployable layout under
// `output/`. ROMs are deduplicated by filename across all folders.
//
// For each lineup we publish:
//   - one pack for "_root" (the depth-0 menu) including:
//       * real games at root level
//       * one folder card per subfolder (acts as a clickable "enter folder"
//         entry; arch="folder", csize=10, regionTag=FOLDER_<lineup>_<dir>)
//   - one pack per subfolder containing only its games
//
// Saves: per-folder (= per-pack), NOT per-game. The engine's save format is
// keyed by (segment_type, slot_index) — slots are pooled across all games in
// a pack with the game's identity inside each file's header (offset 0x050).
// So in the library, the saves/ dir lives at the lineup or folder level:
//   library/<lineup>/saves/                    → /pub/folders/<lineup>/_root/saves/
//   library/<lineup>/FOLDER<id>/saves/         → /pub/folders/<lineup>/<id>/saves/
// We copy verbatim — the engine writes data_TTT_SSSS.bin / meta_TTT_SSSS.bin pairs.
// Cross-folder game moves do NOT migrate save state in v1 (would require
// surgically extracting a game's SRAM from data_008's struct and merging into
// the destination's data_008).

use crate::library::{Folder, Game, Library, Lineup};
use crate::templates::{write_psb_m, Templates};
use crate::title_mode_top::{self, LineupKind};
use crate::title_prof;
use crate::title_select;
use crate::Error;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PublishOptions {
    pub library_root: PathBuf,
    pub stock_data_root: PathBuf,
    pub output_root: PathBuf,
    pub incremental: bool,
}

#[derive(Debug, Default)]
pub struct PublishReport {
    pub roms_packed: usize,
    pub roms_copied: usize,
    pub psb_files_written: usize,
    pub folders_emitted: usize,
    /// Per-game save state files (`state_N.bin`) emitted into pack `saves/`
    /// as `data_{TTT}_{SSSS}.bin`. Counts both data + matching meta as one.
    pub save_files_copied: usize,
    /// Per-game SRAM blocks (8448 bytes each) embedded into the pack's
    /// `saves/sram.bin`. Equals the count of games in the pack that had a
    /// non-empty `library/.../<game_dir>/sram.bin`.
    pub sram_blocks_embedded: usize,
    /// Per-game SRAM files that existed but were the wrong size. Skipped
    /// (slot filled with zeros) and logged as a warning.
    pub sram_blocks_skipped_wrong_size: usize,
    /// `published/m2engage/save/data_008_0000.bin` was (re)built from
    /// library BACKUP_FLAGS + active-pack SRAM.
    pub data_008_emitted: bool,
    /// Present when publishing to <USB>/library/published/.
    pub usb: Option<crate::UsbPreparation>,
}

pub fn publish(opts: &PublishOptions) -> Result<PublishReport, Error> {
    let library = crate::library::load(&opts.library_root)?;
    let templates = Templates::new(&opts.stock_data_root)?;
    // Reject a different console runtime before changing published files.
    if let Some(root) = crate::usb::usb_root(&opts.output_root) {
        let game = root.join("game");
        if game.join(crate::console::SYSTEM_PROFILE).is_file() {
            let live = crate::console::ConsoleVariant::from_directory(&game)?;
            if live != templates.console {
                return Err(Error::Library(format!("Console resources ({}) do not match library templates ({}). Create the library from the matching console dump.", live.id(), templates.console.id())));
            }
        }
    }
    let fonts = crate::fonts::prepare(&library, &opts.library_root, &opts.stock_data_root)?;

    let mut report = PublishReport::default();
    pack_roms(&library, &opts.output_root, &mut report)?;

    // Emit directly into output_root — no m2engage/ intermediate. The
    // variable name stays `m2_root` throughout this file for readability.
    let m2_root = opts.output_root.clone();
    for (lineup_name, lineup) in [("jp", &library.jp), ("us", &library.us)] {
        emit_lineup(lineup_name, lineup, &templates, &m2_root, &mut report)?;
    }

    // Rebuild data_008_0000.bin: library BACKUP_FLAGS + active pack's SRAM.
    // Console reads this on boot; without it, the engine starts with defaults
    // (no per-library prefs / no SRAM for the active pack).
    emit_data_008(&opts.library_root, &m2_root, &mut report)?;

    report.usb = crate::usb::prepare_usb(&opts.output_root, templates.console)?;
    crate::fonts::install(&fonts, &opts.output_root)?;
    if let Some(usb) = &report.usb {
        crate::fonts::install(&fonts, &usb.game_root)?;
    }
    report.psb_files_written += fonts.len();

    Ok(report)
}

// ---- ROM packing ----

#[cfg(test)]
mod rom_tests {
    use super::*;
    use crate::library::{GameJson, GameRom, LineupEntry};
    use std::fs;

    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    #[test]
    fn packed_hucards_are_copied_verbatim_and_cannot_overwrite_raw_rom_outputs() {
        let root = Temp(std::env::temp_dir().join(format!("chronos-packed-roms-{}-{}",
            std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())));
        fs::create_dir(&root.0).unwrap();
        let mut library = Library {
            jp: Lineup { root_entries: vec![], source_dir: root.0.clone() },
            us: Lineup { root_entries: vec![], source_dir: root.0.clone() },
        };
        for name in ["Game.pce.m", "Neutopia_II_J.PCE.m", "Upper.PCE.M"] {
            let packed = m2_mzs::pack_default(b"HuCard ROM", name).unwrap();
            fs::write(root.0.join(name), &packed).unwrap();
            let game = Game {
                dir_name: name.into(), region_tag: name.into(),
                data: GameJson { rom: GameRom { arch: "tg16".into(), rom: name.into(), ..Default::default() }, ..Default::default() },
                sort: Default::default(), source_dir: root.0.clone(),
            };
            library.jp.root_entries.push(LineupEntry::Game(game));
        }
        let output = root.0.join("published");
        let mut report = PublishReport::default();
        pack_roms(&library, &output, &mut report).unwrap();
        assert_eq!((report.roms_copied, report.roms_packed), (3, 0));
        for game in iter_all_games(&library.jp) {
            let original = fs::read(game.rom_path()).unwrap();
            let output_name = format!("{}.m", &game.data.rom.rom[..game.data.rom.rom.len() - 2]);
            assert_eq!(fs::read(output.join("roms").join(&output_name)).unwrap(), original);
            let template = m2_psb::write(&m2_psb::Value::Object(indexmap::IndexMap::from([
                ("root".into(), m2_psb::Value::Object(indexmap::IndexMap::from([
                    ("m2epi".into(), m2_psb::Value::Object(indexmap::IndexMap::new())),
                ]))),
            ])), 4).unwrap();
            let profile = title_prof::generate(&title_prof::GenInputs { template_psb: &template, games: &[game.clone()] }).unwrap();
            let profile = m2_psb::read(&profile).unwrap().to_json();
            let rom = profile["root"]["m2epi"]["version"][&game.region_tag]["rom"].as_str().unwrap();
            assert_eq!(fs::read(output.join(format!("{rom}.m"))).unwrap(), original);
        }
        let mut raw = match &library.jp.root_entries[0] { LineupEntry::Game(g) => g.clone(), _ => unreachable!() };
        raw.data.rom.rom = "Game.pce".into();
        fs::write(root.0.join("Game.pce"), b"different raw ROM").unwrap();
        library.jp.root_entries.push(LineupEntry::Game(raw));
        let before = fs::read(output.join("roms/Game.pce.m")).unwrap();
        assert!(pack_roms(&library, &output, &mut PublishReport::default()).unwrap_err().to_string().contains("collision"));
        assert_eq!(fs::read(output.join("roms/Game.pce.m")).unwrap(), before);
    }
}

fn pack_roms(library: &Library, output_root: &Path, report: &mut PublishReport) -> Result<(), Error> {
    let roms_out = output_root.join("roms");
    std::fs::create_dir_all(&roms_out)?;

    let mut seen: std::collections::HashMap<String, std::path::PathBuf> = std::collections::HashMap::new();
    for lineup in [&library.jp, &library.us] {
        for game in iter_all_games(lineup) {
            // Skip games whose ROM filename is empty / unset (rom field
            // never filled in). Otherwise source_dir.join("") resolves to
            // the game directory itself and fs::copy fails with a useless
            // "not a regular file" error.
            if game.data.rom.rom.trim().is_empty() {
                eprintln!("WARN: skipping ROM-less game {} ({})",
                    game.dir_name, game.source_dir.display());
                continue;
            }
            let rom_src = game.rom_path();
            if !rom_src.exists() {
                return Err(Error::Library(format!(
                    "ROM missing for {}: {}", game.dir_name, rom_src.display()
                )));
            }
            // Also skip if rom_src somehow resolved to a directory (e.g.
            // someone set rom = "." or a directory name).
            if !std::fs::metadata(&rom_src).map(|m| m.is_file()).unwrap_or(false) {
                eprintln!("WARN: skipping non-file ROM source {} ({})",
                    game.dir_name, rom_src.display());
                continue;
            }
            let rom_filename = rom_src.file_name()
                .ok_or_else(|| Error::Library("rom has no filename".into()))?
                .to_string_lossy().to_string();
            let lower = rom_filename.to_ascii_lowercase();
            let needs_packing = lower.ends_with(".pce") || lower.ends_with(".sgx");
            let out_name = if needs_packing {
                format!("{}.m", rom_filename)
            } else if lower.ends_with(".pce.m") || lower.ends_with(".sgx.m") {
                // The native loader appends lowercase .m. MZS keys are case-insensitive.
                format!("{}.m", &rom_filename[..rom_filename.len() - 2])
            } else {
                rom_filename.clone()
            };
            // Raw foo.pce and packed foo.pce.m target the same published file.
            if let Some(prev) = seen.get(&out_name) {
                if !files_have_same_content(prev, &rom_src)? {
                    return Err(Error::Library(format!(
                        "ROM filename collision: '{}' used by both {} and {} with different contents",
                        out_name, prev.display(), rom_src.display()
                    )));
                }
                continue;
            }
            seen.insert(out_name.clone(), rom_src.clone());

            if needs_packing {
                let raw = std::fs::read(&rom_src)?;
                let m = m2_mzs::pack_default(&raw, &out_name)?;
                std::fs::write(roms_out.join(&out_name), m)?;
                report.roms_packed += 1;
            } else {
                let dst = roms_out.join(&out_name);
                std::fs::copy(&rom_src, &dst).map_err(|e| {
                    std::io::Error::new(e.kind(), format!("copy ROM {} -> {}: {}", rom_src.display(), dst.display(), e))
                })?;
                report.roms_copied += 1;
            }
        }
    }
    Ok(())
}

fn iter_all_games(lineup: &Lineup) -> impl Iterator<Item = &Game> {
    lineup.root_entries.iter().flat_map(|e| {
        let games: Vec<&Game> = match e {
            crate::library::LineupEntry::Game(g) => vec![g],
            crate::library::LineupEntry::Folder(f) => f.games.iter().collect(),
        };
        games
    })
}

fn files_have_same_content(a: &Path, b: &Path) -> Result<bool, Error> {
    let am = std::fs::metadata(a)?;
    let bm = std::fs::metadata(b)?;
    if am.len() != bm.len() { return Ok(false); }
    Ok(std::fs::read(a)? == std::fs::read(b)?)
}

// ---- Per-lineup publish ----

fn emit_lineup(
    lineup_name: &str,
    lineup: &Lineup,
    templates: &Templates,
    m2_root: &Path,
    report: &mut PublishReport,
) -> Result<(), Error> {
    let paths = templates.console.templates();
    let title_prof_template = templates.load_stock_psb(paths[0])?;
    let title_mode_top_template = templates.load_stock_psb(paths[1])?;
    let kind = match lineup_name {
        "jp" => LineupKind::Jp,
        "us" => LineupKind::Us,
        other => return Err(Error::Library(format!("unknown lineup '{}'", other))),
    };
    // Cover-sheet template: load the matching stock cover sheet for the lineup.
    // We patch it (add covers, replace front/sg tracks, compact old atlases) rather than
    // rebuild from scratch, to preserve sg / soft31..33 / plus / thumb sections
    // that reference the stock atlases.
    let title_select_template_path = match kind {
        LineupKind::Jp => paths[2],
        LineupKind::Us => paths[3],
    };
    let title_select_template = templates.load_stock_psb(title_select_template_path)?;
    let select_filename = Path::new(title_select_template_path).file_name().unwrap().to_str().unwrap();

    // _root pack: real games + folder cards (mixed in the order from gamelist.json)
    emit_root_pack(
        lineup_name, lineup, &title_prof_template, &title_mode_top_template,
        &title_select_template, &kind, select_filename,
        m2_root, report,
    )?;

    // Per-subfolder packs
    for folder in lineup.folders() {
        emit_folder_pack(
            lineup_name, lineup, folder, &title_prof_template, &title_mode_top_template,
            &title_select_template, &kind, select_filename,
            m2_root, report,
        )?;
    }
    Ok(())
}

fn emit_root_pack(
    lineup_name: &str,
    lineup: &Lineup,
    title_prof_template: &[u8],
    title_mode_top_template: &[u8],
    title_select_template: &[u8],
    lineup_kind: &LineupKind,
    select_filename: &str,
    m2_root: &Path,
    report: &mut PublishReport,
) -> Result<(), Error> {
    let folder_dir = m2_root.join("folders").join(lineup_name).join("_root");
    std::fs::create_dir_all(&folder_dir)?;
    std::fs::create_dir_all(folder_dir.join("saves"))?;

    // For root we need both real games AND folder cards interleaved in the
    // order the user laid them out (gamelist.json ordering).
    let entries = &lineup.root_entries;

    // Build a synthetic Game list — folder cards become Game-shaped pseudo-entries
    // for downstream PSB generators. ROM packing is unaffected (folder cards have
    // no ROM). The editor marker `arch="folder"` is exported as native tg16;
    // menu scripts identify navigation through csize and regionTag.
    let synth_games = synthesize_root_games(entries, lineup_name);

    // Per-pack title_prof. Indices in title_prof.game_versions[*][0] are 0..N-1
    // within this pack only; the script-side folder rebuild releases s_rsc_title
    // and re-runs _init_game_content_config() so s_gameRegionTags is rebuilt
    // from the swapped file. Per-pack SRAM is then aligned with the per-pack
    // index space (each pack uses up to LINEUPMAX (50) of the systemdata's
    // 150-slot array; different packs share the slot space but only one pack
    // is live at a time).
    let prof_psb = title_prof::generate(&title_prof::GenInputs {
        template_psb: title_prof_template,
        games: &synth_games,
    })?;
    write_psb_m(&folder_dir.join("title_prof.psb.m"), &prof_psb)?;
    report.psb_files_written += 1;

    let mode_top_psb = title_mode_top::generate(&title_mode_top::GenInputs {
        template_psb: title_mode_top_template,
        lineup: clone_kind(lineup_kind),
        games: &synth_games,
    })?;
    write_psb_m(&folder_dir.join("title_mode_top.psb.m"), &mode_top_psb)?;
    report.psb_files_written += 1;

    // Title-select: cover for each entry. For real games, use game.cover_path().
    // For folder cards, use the folder's own cover.png (FOLDER<id>/cover.png).
    let select_inputs = build_title_select_inputs(entries);
    let select_psb = title_select::generate(&title_select::GenInputs {
        games: &select_inputs,
        template_psb: title_select_template,
    })?;
    write_psb_m(&folder_dir.join(select_filename), &select_psb)?;
    report.psb_files_written += 1;

    // Save states: per-game library files emitted at the destination's
    // positional slot. Walks synth_games (root-pack ordered games).
    emit_pack_state_files(&folder_dir.join("saves"), &synth_games, lineup_kind, report)?;
    // SRAM: concatenate per-game library blocks in display order. Identity
    // is purely positional (block at index i = game at synth_games[i]).
    emit_pack_sram_bin(&folder_dir.join("saves"), &synth_games, report)?;

    report.folders_emitted += 1;
    Ok(())
}

fn emit_folder_pack(
    lineup_name: &str,
    lineup: &Lineup,
    folder: &Folder,
    title_prof_template: &[u8],
    title_mode_top_template: &[u8],
    title_select_template: &[u8],
    lineup_kind: &LineupKind,
    select_filename: &str,
    m2_root: &Path,
    report: &mut PublishReport,
) -> Result<(), Error> {
    let folder_dir = m2_root.join("folders").join(lineup_name).join(&folder.dir_name);
    std::fs::create_dir_all(&folder_dir)?;
    std::fs::create_dir_all(folder_dir.join("saves"))?;

    // Synthesize a back card at items[0] of every folder pack:
    //   * gives the user a way out (csize=11 -> ::exitGameFolder())
    //   * leaves even an otherwise empty folder navigable (BACK only); the
    //     patched carousel bounds its center index for one or two entries.
    let back_png = ensure_back_arrow_png(m2_root)?;
    let games_with_back: Vec<Game> = folder_pack_games(folder, lineup_name, &back_png);

    // Per-pack title_prof. See emit_root_pack for rationale.
    let prof_psb = title_prof::generate(&title_prof::GenInputs {
        template_psb: title_prof_template,
        games: &games_with_back,
    })?;
    write_psb_m(&folder_dir.join("title_prof.psb.m"), &prof_psb)?;
    report.psb_files_written += 1;

    let mode_top_psb = title_mode_top::generate(&title_mode_top::GenInputs {
        template_psb: title_mode_top_template,
        lineup: clone_kind(lineup_kind),
        games: &games_with_back,
    })?;
    write_psb_m(&folder_dir.join("title_mode_top.psb.m"), &mode_top_psb)?;
    report.psb_files_written += 1;

    // Title-select for folder: back card + games.
    let select_inputs: Vec<title_select::CoverEntry> = games_with_back.iter()
        .map(|g| title_select::CoverEntry {
            cover_path: g.cover_path(),
            cover_size: g.data.cover_size.clone(),
            label: g.dir_name.clone(),
        }).collect();
    let select_psb = title_select::generate(&title_select::GenInputs {
        games: &select_inputs,
        template_psb: title_select_template,
    })?;
    write_psb_m(&folder_dir.join(select_filename), &select_psb)?;
    report.psb_files_written += 1;

    // Per-folder saves. Slot numbers may have shifted since the last
    // publish (game reorder/move within the folder, or back-card position
    // change) — migrate remaps each save's slot based on its internal
    // regionTag. games_with_back puts the back card at index 0, so real
    // games start at game_index 1; migration uses this exact ordering.
    emit_pack_state_files(&folder_dir.join("saves"), &games_with_back, lineup_kind, report)?;
    // SRAM: same positional ordering as title_mode_top (back card at
    // slot 0 gets 8448 zeros — its sram_path() points at a non-existent
    // file, so the slot stays zero).
    emit_pack_sram_bin(&folder_dir.join("saves"), &games_with_back, report)?;

    report.folders_emitted += 1;
    Ok(())
}

// ---- Back card synthesis ----

/// Write a 240x240 RGBA PNG of a left-arrow + colored border to use as the
/// back card cover for every folder pack. Returns the path. Idempotent —
/// re-creates the file each publish run (no caching).
fn ensure_back_arrow_png(m2_root: &Path) -> Result<PathBuf, Error> {
    let assets_dir = m2_root.join(".assets");
    std::fs::create_dir_all(&assets_dir)?;
    let path = assets_dir.join("back_arrow.png");

    use image::{Rgba, RgbaImage};
    let bg     = Rgba([20u8, 20, 30, 255]);
    let border = Rgba([180u8, 180, 200, 255]);
    let arrow  = Rgba([220u8, 220, 230, 255]);

    let mut img = RgbaImage::from_pixel(240, 240, bg);

    // 3-pixel border
    for x in 0..240u32 {
        for t in 0..3u32 {
            img.put_pixel(x, t, border);
            img.put_pixel(x, 239 - t, border);
        }
    }
    for y in 0..240u32 {
        for t in 0..3u32 {
            img.put_pixel(t, y, border);
            img.put_pixel(239 - t, y, border);
        }
    }

    // Left-pointing arrow: triangular head + rectangular tail
    // Triangle apex at (50, 120); base from (110, 60) to (110, 180).
    for y in 60..180u32 {
        let dy = if y >= 120 { y - 120 } else { 120 - y } as i32;
        let tri_left = (50 + dy) as u32;
        for x in tri_left..110 {
            img.put_pixel(x, y, arrow);
        }
    }
    // Tail rectangle: (110, 100) → (190, 140)
    for y in 100..140u32 {
        for x in 110..190u32 {
            img.put_pixel(x, y, arrow);
        }
    }

    img.save(&path)
        .map_err(|e| Error::Library(format!("failed to write {}: {}", path.display(), e)))?;
    Ok(path)
}

/// Synthesize a Game entry representing the "back" card placed at items[0]
/// of every folder pack. csize=11 triggers the script's exit-folder branch.
fn back_card_game(lineup_name: &str, back_png: &Path) -> Game {
    use crate::library::*;
    let region_tag = format!("FOLDER_{}_BACK", lineup_name);
    let cover_filename = back_png
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "back_arrow.png".to_string());
    let source_dir = back_png.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let data = GameJson {
        display: GameDisplay {
            name: "Back".into(),
            tname: "Back".into(),
            name_eng: "Back".into(),
            titlebar: 4,
            ccolor: 0,
            csize: 11,    // back marker — script's csize==11 branch calls ::exitGameFolder()
            players: 0,
            demo_time: 0,
        },
        emulator: GameEmulator::default(),
        rom: GameRom {
            arch: "folder".to_string(),
            country: lineup_name.into(),
            preamp: 0.0,
            // No file is needed: title_prof exports the folder as tg16,
            // which stock M2 can initialize with a missing ROM.
            rom: region_tag.clone(),
            region_tag: Some(region_tag.clone()),
            ..Default::default()
        },
        rom_alt: RomAlt::default(),
        cover: Some(cover_filename),
        cover_size: None,
    };
    Game {
        dir_name: "BACK".to_string(),
        region_tag,
        data,
        sort: SortIndices::default(),  // overwritten by renumber below
        source_dir,
    }
}

/// Build the games list a folder pack actually publishes: a back card at
/// position 0, then the folder's real games. Sort indices are renumbered
/// 0..N to be contiguous.
fn folder_pack_games(folder: &Folder, lineup_name: &str, back_png: &Path) -> Vec<Game> {
    let mut games: Vec<Game> = Vec::with_capacity(folder.games.len() + 1);
    games.push(back_card_game(lineup_name, back_png));
    games.extend(folder.games.iter().cloned());
    for (i, g) in games.iter_mut().enumerate() {
        let v = i as u32;
        g.sort.sor_demo = v;
        g.sort.sor_date = v;
        g.sort.sor_genr = v;
        g.sort.sor_name = v;
        g.sort.sor_pnum = v;
    }
    games
}

fn clone_kind(k: &LineupKind) -> LineupKind {
    match k {
        LineupKind::Jp => LineupKind::Jp,
        LineupKind::Us => LineupKind::Us,
    }
}

// ---- Folder-card synthesis ----

/// Turn the lineup's root entries into a flat Game list usable by the existing
/// title_prof / title_mode_top generators. Folder entries are synthesized into
/// folder-card games (arch="folder", csize=10, regionTag=FOLDER_<lineup>_<id>).
fn synthesize_root_games(entries: &[crate::library::LineupEntry], lineup_name: &str) -> Vec<Game> {
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        match e {
            crate::library::LineupEntry::Game(g) => out.push(g.clone()),
            crate::library::LineupEntry::Folder(f) => out.push(folder_to_game(f, lineup_name)),
        }
    }
    out
}

fn folder_to_game(folder: &Folder, lineup_name: &str) -> Game {
    use crate::library::*;
    let region_tag = folder.region_tag(lineup_name);
    let data = GameJson {
        display: GameDisplay {
            name: folder.display_name.clone(),
            tname: folder.display_name.clone(),
            name_eng: folder.display_name.clone(),
            titlebar: 4,
            ccolor: 0,
            csize: 10, // folder marker — script logic treats csize=10 as enter-folder
            players: 0,
            demo_time: 0,
        },
        emulator: GameEmulator::default(),
        rom: GameRom {
            arch: "folder".to_string(),
            country: lineup_name.into(),
            preamp: 0.0,
            // Keep a non-playable pseudo-ROM name; title_prof exports the
            // editor's folder marker as native tg16. Navigation uses regionTag.
            rom: region_tag.clone(),
            region_tag: Some(region_tag.clone()),
            ..Default::default()
        },
        rom_alt: RomAlt::default(),
        cover: Some("cover.png".to_string()),
        cover_size: None,
    };
    Game {
        dir_name: folder.dir_name.clone(),
        region_tag,
        data,
        sort: folder.sort.clone(),
        source_dir: folder.source_dir.clone(),
    }
}

// ---- Title-select cover inputs (mixed games + folder cards) ----

fn build_title_select_inputs(entries: &[crate::library::LineupEntry]) -> Vec<title_select::CoverEntry> {
    entries.iter().map(|e| match e {
        crate::library::LineupEntry::Game(g) => title_select::CoverEntry {
            cover_path: g.cover_path(),
            cover_size: g.data.cover_size.clone(),
            label: g.dir_name.clone(),
        },
        crate::library::LineupEntry::Folder(f) => title_select::CoverEntry {
            cover_path: f.cover_path(),
            cover_size: None,
            label: f.dir_name.clone(),
        },
    }).collect()
}

// ---- Save state emission ----
//
// Library stores per-game slot-agnostic save states in
// `<gameDir>/saves/state_N.bin` (N = 0..3) + `state_N_meta.bin`. At publish
// time the publisher walks the destination pack's ordered games and emits
// engine-format `data_{TTT}_{SSSS:04}.bin` + `meta_*.bin` files into the
// pack's `saves/` directory. Slot number = game_index * 4 + save_slot +
// lineup_offset. Segment type comes from the game's `arch` (011 for CD,
// 012 for HuCard / SGX / everything else; folder cards have no save).
//
// Identity is positional: game at ordered_games[i] gets slots [i*4..i*4+3].
// No regionTag parsing needed — the library is already keyed by the
// per-game directory, which is the stable identifier. When the editor
// reorders or moves games, the library file stays in the same per-game
// directory; the publisher just emits it at the new positional slot.
//
// SRAM (segment type 008) is handled separately in `emit_pack_sram_bin`.

/// 4 save slots per game, fixed by the engine.
const SAVES_PER_GAME: u16 = 4;
/// US lineup slot offset = LINEUPMAX (50) games * SAVES_PER_GAME slots.
const LINEUP_SLOT_OFFSET_US: u16 = 200;

/// Walk `ordered_games` in order and copy each game's per-game state files
/// into `dest_saves_dir` with engine-format names. Skips games with no
/// `saves/state_N.bin` files (most games), and folder cards (no save type).
fn emit_pack_state_files(
    dest_saves_dir: &Path,
    ordered_games: &[Game],
    lineup_kind: &LineupKind,
    report: &mut PublishReport,
) -> Result<(), Error> {
    std::fs::create_dir_all(dest_saves_dir)?;

    // Clear out stale state files from previous publishes before writing
    // fresh ones. Without this, a game that moved between folders or got
    // removed leaves its data_TTT_SSSS.bin behind at the old slot index,
    // which on the next folder swap leaks into the live save dir as a
    // "ghost save" for whatever entry now occupies that position
    // (typically a folder card or a different game). Per-game library
    // saves are the authoritative source — anything in the pack's saves/
    // dir that isn't about to be re-emitted is stale by definition.
    if dest_saves_dir.exists() {
        for entry in std::fs::read_dir(dest_saves_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() { continue; }
            let name = entry.file_name();
            let name_str = match name.to_str() { Some(s) => s, None => continue };
            let is_state = (name_str.starts_with("data_011_") || name_str.starts_with("data_012_")
                            || name_str.starts_with("meta_011_") || name_str.starts_with("meta_012_"))
                           && name_str.ends_with(".bin");
            if is_state {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    let lineup_offset = match lineup_kind {
        LineupKind::Jp => 0,
        LineupKind::Us => LINEUP_SLOT_OFFSET_US,
    };

    for (i, game) in ordered_games.iter().enumerate() {
        let seg_type = match game.save_state_type() {
            Some(t) => t,
            None => continue, // folder card etc.
        };
        let game_index = i as u16;
        let game_saves = game.saves_dir();
        if !game_saves.exists() { continue; }

        for save_slot in 0..SAVES_PER_GAME {
            let src_data = game_saves.join(format!("state_{}.bin", save_slot));
            if !src_data.exists() { continue; }
            // Defend against `state_N.bin` being a directory (or other
            // non-regular file) on disk. fs::copy would emit a useless
            // "source is neither a regular file nor a symlink to a
            // regular file" error; skip with a stderr warning instead.
            let md = match std::fs::metadata(&src_data) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !md.is_file() {
                eprintln!("WARN: skipping non-file save state {}", src_data.display());
                continue;
            }
            let src_meta = game_saves.join(format!("state_{}_meta.bin", save_slot));

            let slot = game_index * SAVES_PER_GAME + save_slot + lineup_offset;
            let dst_data = dest_saves_dir.join(format!("data_{:03}_{:04}.bin", seg_type, slot));
            let dst_meta = dest_saves_dir.join(format!("meta_{:03}_{:04}.bin", seg_type, slot));

            std::fs::copy(&src_data, &dst_data).map_err(|e| {
                std::io::Error::new(e.kind(), format!("copy {} -> {}: {}", src_data.display(), dst_data.display(), e))
            })?;
            if src_meta.exists() && std::fs::metadata(&src_meta).map(|m| m.is_file()).unwrap_or(false) {
                std::fs::copy(&src_meta, &dst_meta).map_err(|e| {
                    std::io::Error::new(e.kind(), format!("copy {} -> {}: {}", src_meta.display(), dst_meta.display(), e))
                })?;
            }
            report.save_files_copied += 1;
        }
    }
    Ok(())
}

// ---- SRAM (pack-level concatenated blob) ----
//
// The hook writes a single `pack/saves/sram.bin` per pack: 150 × 8448-byte
// blocks (= 1,267,200 bytes), spliced verbatim into `data_008_0000.bin` at
// offset 0x5E80 on swap-in. Game identity is purely positional — block N
// belongs to the game at `gamelist.json[N]`. The block's *internal* tag
// (e.g. "STATRAM0") is the ROM's own save identifier, not the regionTag,
// so it can't be used to identify which game a block belongs to outside
// the context of the source pack.
//
// Publisher contract: take per-game `library/.../<game_dir>/sram.bin`
// blocks (8448 bytes each), concatenate in the destination pack's
// `gamelist.json` order, emit as `pack/saves/sram.bin`. Slots without a
// per-game file (or with the wrong size) get 8448 bytes of zeros.

/// Bytes per `struct_sram_data` slot in data_008. Matches m2hook_print.c.
const SRAM_BLOCK_SIZE: usize = 8448;
/// Number of slots in the SRAM array. Matches m2hook_print.c.
const SRAM_GAME_COUNT: usize = 150;
/// Offset in data_008_0000.bin where _92_sram_datas begins. Matches m2hook_print.c.
const SRAM_OFFSET: usize = 0x5E80;
/// Total engine-expected size of data_008_0000.bin (BACKUP_FLAGS + SRAM array + 4 trailing bytes).
const DATA_008_SIZE: usize = 1_291_396;

/// Build the pack's `saves/sram.bin` from per-game library blocks.
/// `ordered_games` must use the SAME ordering as the pack's title_mode_top
/// items[] (i.e., back card at position 0 for folder packs, real games
/// follow). Always writes exactly `SRAM_GAME_COUNT * SRAM_BLOCK_SIZE` bytes.
///
/// Games whose per-game SRAM file is missing -> 8448 zeros at that slot.
/// Games whose file exists but isn't exactly 8448 bytes -> 8448 zeros at
/// that slot + warning logged + report.sram_blocks_skipped_wrong_size++.
/// Games beyond the 150th slot are dropped with a warning (LINEUPMAX=50
/// plus a back card keeps us well under 150 in practice).
fn emit_pack_sram_bin(
    dest_saves_dir: &Path,
    ordered_games: &[Game],
    report: &mut PublishReport,
) -> Result<(), Error> {
    std::fs::create_dir_all(dest_saves_dir)?;
    let mut blob = vec![0u8; SRAM_GAME_COUNT * SRAM_BLOCK_SIZE];

    let n = ordered_games.len().min(SRAM_GAME_COUNT);
    if ordered_games.len() > SRAM_GAME_COUNT {
        eprintln!(
            "WARN: pack has {} games but SRAM array is only {} slots; tail truncated",
            ordered_games.len(), SRAM_GAME_COUNT
        );
    }

    for (i, game) in ordered_games.iter().take(n).enumerate() {
        let path = game.sram_path();
        if !path.exists() { continue; }
        let bytes = std::fs::read(&path)?;
        if bytes.len() != SRAM_BLOCK_SIZE {
            eprintln!(
                "WARN: {} is {} bytes, expected {}; slot {} zeroed",
                path.display(), bytes.len(), SRAM_BLOCK_SIZE, i
            );
            report.sram_blocks_skipped_wrong_size += 1;
            continue;
        }
        let start = i * SRAM_BLOCK_SIZE;
        blob[start .. start + SRAM_BLOCK_SIZE].copy_from_slice(&bytes);
        report.sram_blocks_embedded += 1;
    }

    let out = dest_saves_dir.join("sram.bin");
    std::fs::write(&out, &blob)?;
    Ok(())
}

// ---- data_008 reconstruction ----
//
// data_008_0000.bin layout:
//   [0 .. 0x5E80)               BACKUP_FLAGS + settings (per-library user prefs)
//   [0x5E80 .. 0x5E80+1.27 MB)  _92_sram_datas (active pack's per-game SRAM)
//   trailing 4 bytes            padding
// Total = 1,291,396 bytes (engine-expected; size check in load_systemdata).
//
// On publish we rebuild this from:
//   * library/data_008_0000.bin (captured by sync from console-side writes
//     of language/theme/lineup/etc.) for the BACKUP_FLAGS region.
//   * active pack's published/m2engage/folders/<lineup>/<dir>/saves/sram.bin
//     for the SRAM region. Active = folders/.current (default jp/_root).
// Missing sources are filled with zeros; engine boots with defaults.

fn read_current(m2_root: &Path) -> Option<(String, String)> {
    let raw = std::fs::read_to_string(m2_root.join("folders/.current")).ok()?;
    let trimmed = raw.trim();
    let (l, d) = trimmed.split_once('/')?;
    Some((l.to_string(), d.to_string()))
}

fn emit_data_008(
    library_root: &Path,
    m2_root: &Path,
    report: &mut PublishReport,
) -> Result<(), Error> {
    let save_dir = m2_root.join("save");
    std::fs::create_dir_all(&save_dir)?;
    let data_008_path = save_dir.join("data_008_0000.bin");
    let meta_008_path = save_dir.join("meta_008_0000.bin");

    // Preserve user state on republish.
    //
    // Once the console has written real settings (language, theme, etc.)
    // into BACKUP_FLAGS — either via in-game writes through the bind-mount
    // or via the seed-from-NAND fallback — the published save files become
    // authoritative user state, not build output. Overwriting them on
    // every publish would lose settings every time the user added a game.
    //
    // We detect that by scanning the first 4 KiB of data_008 (covers the
    // header region engine initialises early). If anything non-zero is
    // there, leave both files alone. SRAM region (0x5E80+) is managed
    // in-place by the hook on pack switches; we don't rewrite it here.
    if has_user_state(&data_008_path)? {
        report.data_008_emitted = false;
        return Ok(());
    }

    let mut buf = vec![0u8; DATA_008_SIZE];

    // BACKUP_FLAGS section: copy first 0x5E80 bytes of library/data_008
    // (synced from console-side writes in prior workflow; usually absent
    // for fresh libraries — buffer stays zero, engine boots with defaults).
    let lib_data_008 = library_root.join("data_008_0000.bin");
    if lib_data_008.exists() {
        let bytes = std::fs::read(&lib_data_008)?;
        let n = bytes.len().min(SRAM_OFFSET);
        buf[..n].copy_from_slice(&bytes[..n]);
    }

    // SRAM section: active pack's sram.bin (defaults to jp/_root).
    let (lineup, dir) = read_current(m2_root)
        .unwrap_or_else(|| ("jp".to_string(), "_root".to_string()));
    let active_sram = m2_root.join("folders").join(&lineup).join(&dir).join("saves/sram.bin");
    if active_sram.exists() {
        let bytes = std::fs::read(&active_sram)?;
        let n = bytes.len().min(SRAM_BLOCK_SIZE * SRAM_GAME_COUNT);
        buf[SRAM_OFFSET .. SRAM_OFFSET + n].copy_from_slice(&bytes[..n]);
    }

    // Native autoload parses meta_008 as PSB before inspecting the data.
    // An all-zero placeholder crashes the loader instead of being rebuilt.
    let metadata = system_save_metadata(&buf)?;
    std::fs::write(&data_008_path, &buf)?;
    std::fs::write(&meta_008_path, metadata)?;
    report.data_008_emitted = true;
    Ok(())
}

fn system_save_metadata(data: &[u8]) -> Result<Vec<u8>, Error> {
    use md5::{Digest, Md5};
    use m2_psb::{Stream, Value};
    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs() as i64;
    let fields = indexmap::IndexMap::from([
        ("CryptMagic".into(), Value::Int(0)),
        ("Digest".into(), Value::Stream(Stream { index: 0, data: Md5::digest(data).to_vec() })),
        ("FileSize".into(), Value::Int(data.len() as i64)),
        ("FileVersion".into(), Value::Int(0x10200)),
        ("OriginalSize".into(), Value::Int(data.len() as i64)),
        ("TimeStamp".into(), Value::Int(timestamp)),
    ]);
    Ok(m2_psb::write(&Value::Object(fields), 3)?)
}

#[cfg(test)]
mod system_save_tests {
    use super::*;
    #[test]
    fn generated_metadata_is_native_psb_with_matching_size_and_digest() {
        use md5::{Digest, Md5};
        use m2_psb::Value;
        let data = vec![0u8; DATA_008_SIZE];
        let meta = system_save_metadata(&data).unwrap();
        assert_eq!(&meta[..8], b"PSB\0\x03\0\0\0");
        let Value::Object(fields) = m2_psb::read(&meta).unwrap() else { panic!("metadata object"); };
        assert_eq!(fields["FileSize"], Value::Int(DATA_008_SIZE as i64));
        assert_eq!(fields["OriginalSize"], fields["FileSize"]);
        assert_eq!(fields["FileVersion"], Value::Int(0x10200));
        let Value::Stream(digest) = &fields["Digest"] else { panic!("digest stream"); };
        assert_eq!(digest.data, Md5::digest(&data).to_vec());
    }
}

fn has_user_state(path: &Path) -> Result<bool, Error> {
    if !path.exists() { return Ok(false); }
    let mut f = std::fs::File::open(path)?;
    let mut buf = [0u8; 4096];
    use std::io::Read;
    let n = f.read(&mut buf)?;
    Ok(buf[..n].iter().any(|&b| b != 0))
}
