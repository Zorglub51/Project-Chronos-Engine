// Library reader. Walks the on-disk source tree and produces a typed model
// the publish pipeline can consume.
//
// On-disk schema (matches pce-game-editor's editor model):
//
//   library/<lineup>/                        ← jp or us
//     gamelist.json                          ← [{folder, sor_*}], includes BOTH games and folders
//     <gameDirName>/                         ← regular game (folder name does NOT start with "FOLDER")
//       game.json                            ← display, emulator, rom, cover, cover_size
//       <rom_filename>                       ← e.g. KungFu_J.pce
//       cover.png
//       sram.bin                             ← 8448 B; one slot of _92_sram_datas (per-game)
//       saves/                               ← per-game save states
//         state_0.bin .. state_3.bin         ← slot-agnostic; publisher renames at publish time
//         state_0_meta.bin .. state_3_meta.bin
//     FOLDER<id>/                            ← folder entry (folder name starts with "FOLDER")
//       folder.json                          ← {name: "Display name"}
//       gamelist.json                        ← folder's own [{folder, sor_*}]
//       cover.png                            ← folder's icon shown as a card in parent menu
//       <gameDirName>/...                    ← games inside the folder (same per-game shape)
//
// Saves are PER-GAME and follow the game when the editor moves it between
// folders or lineups. The publisher computes engine-format slot numbers
// at publish time (slot = game_index_in_pack * 4 + save_slot + lineup_offset)
// and emits `data_{TTT}_{SSSS:04}.bin` + `meta_*.bin` into each pack's
// `published/folders/<lineup>/<dir>/saves/`. The on-USB published side is
// the engine-format authoritative copy; the editor's "open library" step
// reorganises it into the per-game library view above.
//
// regionTag comes from game.json::rom.regionTag (optional). If absent, falls
// back to the game's directory name. The directory name is the editor's
// stable identifier; the regionTag can be renamed independently.

use crate::Error;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---- Raw JSON shapes ----

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameDisplay {
    #[serde(default)] pub name: String,
    #[serde(default)] pub tname: String,
    #[serde(default)] pub name_eng: String,
    #[serde(default)] pub titlebar: u32,
    #[serde(default)] pub ccolor: u32,
    #[serde(default)] pub csize: u32,
    #[serde(default)] pub players: u32,
    #[serde(default)] pub demo_time: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameEmulator {
    #[serde(rename = "EmuOfsX", default)] pub emu_ofs_x: i32,
    #[serde(rename = "EmuOfsY", default)] pub emu_ofs_y: i32,
    #[serde(rename = "ScreenMode", default)] pub screen_mode: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameRom {
    #[serde(default)] pub arch: String,
    #[serde(default)] pub country: String,
    #[serde(default)] pub preamp: f64,
    #[serde(default)] pub rom: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub tg16cd_systemcard: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub tg16_option: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")] pub tg16pad_type: Option<u32>,
    #[serde(rename = "regionTag", skip_serializing_if = "Option::is_none")]
    pub region_tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoverSize {
    #[serde(default)] pub width: u32,
    #[serde(default)] pub height: u32,
    #[serde(rename = "originX", default)] pub origin_x: u32,
    #[serde(rename = "originY", default)] pub origin_y: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RomAlt {
    #[serde(rename = "regionTagE", default)] pub region_tag_e: Option<String>,
    #[serde(rename = "regionTagS1", default)] pub region_tag_s1: Option<String>,
    #[serde(rename = "regionTagS2", default)] pub region_tag_s2: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameJson {
    #[serde(default)] pub display: GameDisplay,
    #[serde(default)] pub emulator: GameEmulator,
    pub rom: GameRom,
    #[serde(default)] pub rom_alt: RomAlt,
    #[serde(default)] pub cover: Option<String>,
    #[serde(default)] pub cover_size: Option<CoverSize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SortIndices {
    #[serde(default)] pub sor_date: u32,
    #[serde(default)] pub sor_demo: u32,
    #[serde(default)] pub sor_genr: u32,
    #[serde(default)] pub sor_name: u32,
    #[serde(default)] pub sor_pnum: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GameListEntry {
    pub folder: String,
    #[serde(flatten)]
    pub sort: SortIndices,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FolderJson {
    #[serde(default)] pub name: String,
}

// ---- Typed model used by the publish pipeline ----

#[derive(Debug, Clone)]
pub struct Game {
    /// Directory name on disk (stable identifier across game.json edits).
    pub dir_name: String,
    /// Logical regionTag — from game.json::rom.regionTag, defaults to dir_name.
    pub region_tag: String,
    pub data: GameJson,
    pub sort: SortIndices,
    pub source_dir: PathBuf,
}

impl Game {
    pub fn rom_path(&self) -> PathBuf { self.source_dir.join(&self.data.rom.rom) }
    pub fn cover_path(&self) -> PathBuf {
        let f = self.data.cover.clone().unwrap_or_else(|| "cover.png".into());
        self.source_dir.join(f)
    }
    /// Path to this game's SRAM block in the library. The publisher reads
    /// this on publish and embeds it into the destination pack's
    /// concatenated `saves/sram.bin` at the game's positional offset.
    /// 8448 bytes if present; missing/wrong-size = zero block in the pack.
    /// Editor writes it during console-sync by splitting a pack's sram.bin
    /// into per-game blocks using `gamelist.json[N]` for identity.
    pub fn sram_path(&self) -> PathBuf { self.source_dir.join("sram.bin") }
    /// Per-game save state directory in the library. Contains slot-agnostic
    /// `state_0.bin..state_3.bin` and matching `state_N_meta.bin` files.
    /// Publisher emits them into the destination pack's `saves/` as
    /// `data_{TTT}_{game_index*4+N+lineup_offset:04}.bin` (+ meta).
    pub fn saves_dir(&self) -> PathBuf { self.source_dir.join("saves") }
    /// Save state segment type for this game's arch.
    /// 011 = CD games (EMU_STATE_L), 012 = HuCard / SGX / everything else.
    /// `None` for non-savable entries (folder cards).
    pub fn save_state_type(&self) -> Option<u16> {
        let arch = self.data.rom.arch.as_str();
        if arch == "folder" { None }
        else if arch.contains("cd") { Some(11) }
        else { Some(12) }
    }
    pub fn cover_dimensions(&self) -> (u32, u32, u32, u32) {
        if let Some(cs) = &self.data.cover_size {
            let w = if cs.width == 0 { 240 } else { cs.width };
            let h = if cs.height == 0 { 240 } else { cs.height };
            let ox = if cs.origin_x == 0 { w / 2 } else { cs.origin_x };
            let oy = if cs.origin_y == 0 { h / 2 } else { cs.origin_y };
            (w, h, ox, oy)
        } else {
            (240, 240, 120, 120)
        }
    }
}

#[derive(Debug, Clone)]
pub struct Folder {
    /// Directory name on disk, e.g. "FOLDER01".
    pub dir_name: String,
    /// Display name from folder.json, falls back to dir_name.
    pub display_name: String,
    /// Sort indices come from the PARENT lineup's gamelist.json (folder's row in it).
    pub sort: SortIndices,
    pub source_dir: PathBuf,
    /// Games inside the folder, in order (from folder's own gamelist.json).
    pub games: Vec<Game>,
}

impl Folder {
    pub fn cover_path(&self) -> PathBuf { self.source_dir.join("cover.png") }
    /// regionTag used in PSB output for this folder card. Matches the
    /// convention in m2engage-mac/games_loader.cpp::make_folder_game_entry.
    pub fn region_tag(&self, lineup: &str) -> String {
        format!("FOLDER_{}_{}", lineup, self.dir_name)
    }
}

/// An entry in the lineup's root gamelist — either a game or a folder card.
/// They are siblings; their relative order in the menu carousel comes from
/// `gamelist.json` order.
#[derive(Debug, Clone)]
pub enum LineupEntry {
    Game(Game),
    Folder(Folder),
}

impl LineupEntry {
    pub fn dir_name(&self) -> &str {
        match self {
            LineupEntry::Game(g) => &g.dir_name,
            LineupEntry::Folder(f) => &f.dir_name,
        }
    }
    pub fn sort(&self) -> &SortIndices {
        match self {
            LineupEntry::Game(g) => &g.sort,
            LineupEntry::Folder(f) => &f.sort,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Lineup {
    /// Root-level entries (games + folder cards) in the order the editor wrote.
    pub root_entries: Vec<LineupEntry>,
    /// Path to lineup directory — used to find saves/ at the lineup level.
    pub source_dir: PathBuf,
}

impl Lineup {
    /// Flatten only the folder entries.
    pub fn folders(&self) -> impl Iterator<Item = &Folder> {
        self.root_entries.iter().filter_map(|e| match e {
            LineupEntry::Folder(f) => Some(f),
            _ => None,
        })
    }
    /// Iterator of (folder_id, &[Game]) covering: lineup root (id="_root") + each folder.
    /// Each tuple is one publishable folder pack.
    pub fn iter_publish_folders(&self) -> Vec<(String, Vec<&Game>)> {
        let mut out = Vec::new();
        // Root pack: the games at root level (folders are NOT games inside themselves;
        // they appear as folder-card items in the root pack via Lineup::root_entries).
        let root_games: Vec<&Game> = self.root_entries.iter()
            .filter_map(|e| match e {
                LineupEntry::Game(g) => Some(g),
                _ => None,
            }).collect();
        out.push(("_root".to_string(), root_games));
        // Each subfolder is its own pack.
        for f in self.folders() {
            out.push((f.dir_name.clone(), f.games.iter().collect()));
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct Library {
    pub jp: Lineup,
    pub us: Lineup,
}

// ---- Loader ----

pub fn load(library_root: &Path) -> Result<Library, Error> {
    Ok(Library {
        jp: load_lineup(&library_root.join("jp"))?,
        us: load_lineup(&library_root.join("us"))?,
    })
}

fn load_lineup(lineup_dir: &Path) -> Result<Lineup, Error> {
    if !lineup_dir.exists() {
        return Ok(Lineup { root_entries: Vec::new(), source_dir: lineup_dir.to_path_buf() });
    }
    let entries = read_gamelist(lineup_dir)?;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let entry_dir = lineup_dir.join(&entry.folder);
        if !entry_dir.exists() {
            return Err(Error::Library(format!(
                "gamelist.json references '{}' but {} does not exist",
                entry.folder, entry_dir.display()
            )));
        }
        if entry.folder.starts_with("FOLDER") {
            out.push(LineupEntry::Folder(load_folder(&entry_dir, entry)?));
        } else {
            out.push(LineupEntry::Game(load_game(&entry_dir, entry)?));
        }
    }
    Ok(Lineup { root_entries: out, source_dir: lineup_dir.to_path_buf() })
}

fn load_folder(folder_dir: &Path, entry: GameListEntry) -> Result<Folder, Error> {
    let folder_meta: FolderJson = match read_optional_json(&folder_dir.join("folder.json"))? {
        Some(j) => j,
        None => FolderJson::default(),
    };
    let display_name = if folder_meta.name.is_empty() {
        entry.folder.clone()
    } else {
        folder_meta.name
    };

    let sub_entries = read_gamelist(folder_dir)?;
    let mut games = Vec::with_capacity(sub_entries.len());
    for sub in sub_entries {
        let game_dir = folder_dir.join(&sub.folder);
        if !game_dir.exists() {
            return Err(Error::Library(format!(
                "folder '{}' gamelist references '{}' but {} does not exist",
                entry.folder, sub.folder, game_dir.display()
            )));
        }
        if sub.folder.starts_with("FOLDER") {
            // Editor model is single-level: folders inside folders aren't supported.
            return Err(Error::Library(format!(
                "nested folder '{}' inside '{}' is not supported (single-level only)",
                sub.folder, entry.folder
            )));
        }
        games.push(load_game(&game_dir, sub)?);
    }

    Ok(Folder {
        dir_name: entry.folder.clone(),
        display_name,
        sort: entry.sort,
        source_dir: folder_dir.to_path_buf(),
        games,
    })
}

fn load_game(game_dir: &Path, entry: GameListEntry) -> Result<Game, Error> {
    let game_json = game_dir.join("game.json");
    if !game_json.exists() {
        return Err(Error::Library(format!("missing {}", game_json.display())));
    }
    let txt = std::fs::read_to_string(&game_json)?;
    let data: GameJson = serde_json::from_str(&txt)?;
    let region_tag = data.rom.region_tag.clone().unwrap_or_else(|| entry.folder.clone());
    Ok(Game {
        dir_name: entry.folder,
        region_tag,
        data,
        sort: entry.sort,
        source_dir: game_dir.to_path_buf(),
    })
}

fn read_gamelist(dir: &Path) -> Result<Vec<GameListEntry>, Error> {
    let path = dir.join("gamelist.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let txt = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&txt)?)
}

fn read_optional_json<T: for<'de> serde::Deserialize<'de>>(path: &Path) -> Result<Option<T>, Error> {
    if !path.exists() { return Ok(None); }
    let txt = std::fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&txt)?))
}
