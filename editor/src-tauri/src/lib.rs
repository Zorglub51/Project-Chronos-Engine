use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;
mod import_commands;
mod font_commands;

// --- Data structures matching game.json schema ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameDisplay {
    pub name: String,
    #[serde(default)]
    pub tname: String,
    #[serde(default)]
    pub name_eng: String,
    #[serde(default)]
    pub titlebar: u32,
    #[serde(default)]
    pub ccolor: u32,
    #[serde(default)]
    pub csize: u32,
    #[serde(default)]
    pub players: u32,
    #[serde(default)]
    pub demo_time: u32,
}

// Emulator overrides per game (output offsets and screen mode). Matches the
// `emulator` block in the original retail title_mode_top items. Field names
// match the engine's PSB keys exactly.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameEmulator {
    #[serde(rename = "EmuOfsX", default)]
    pub emu_ofs_x: i32,
    #[serde(rename = "EmuOfsY", default)]
    pub emu_ofs_y: i32,
    #[serde(rename = "ScreenMode", default)]
    pub screen_mode: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameRom {
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub preamp: f64,
    #[serde(default)]
    pub rom: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tg16cd_systemcard: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tg16_option: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tg16pad_type: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "regionTag")]
    pub region_tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoverSize {
    pub width: u32,
    pub height: u32,
    #[serde(rename = "originX")]
    pub origin_x: u32,
    #[serde(rename = "originY")]
    pub origin_y: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameJson {
    pub display: GameDisplay,
    #[serde(default)]
    pub emulator: GameEmulator,
    pub rom: GameRom,
    // `rom_alt` (regionTagE/S1/S2) is intentionally not modelled here. The
    // publisher renders missing alt-region fields as the retail default "NO".
    // Old game.json files with `rom_alt` will have it silently dropped on
    // first editor save — that's intended; the feature is being removed.
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_size: Option<CoverSize>,
    // `genre` removed from the editor for now (kept in retail PSBs only
    // when present in the source title_mode_top). Re-enable when we wire
    // genre editing into the UI.
    // #[serde(default, skip_serializing_if = "Option::is_none")]
    // pub genre: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameListEntry {
    pub folder: String,
    #[serde(default)]
    pub sor_date: u32,
    #[serde(default)]
    pub sor_demo: u32,
    #[serde(default)]
    pub sor_genr: u32,
    #[serde(default)]
    pub sor_name: u32,
    #[serde(default)]
    pub sor_pnum: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEntry {
    pub folder: String,
    pub game: GameJson,
    pub has_cover: bool,
    pub sort: GameListEntry,
    #[serde(default)]
    pub is_folder: bool,
    #[serde(default)]
    pub game_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub games: Vec<GameEntry>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualLibrary {
    pub jp: Library,
    pub us: Library,
    pub path: String,
}

// --- Helper: load a single lineup from a subdirectory ---

fn load_lineup(lineup_path: &Path) -> Library {
    let gamelist_path = lineup_path.join("gamelist.json");
    let path_str = lineup_path.to_string_lossy().to_string();

    if !gamelist_path.exists() {
        return Library {
            games: Vec::new(),
            path: path_str,
        };
    }

    let gamelist_str = match fs::read_to_string(&gamelist_path) {
        Ok(s) => s,
        Err(_) => {
            return Library {
                games: Vec::new(),
                path: path_str,
            }
        }
    };

    let gamelist: Vec<GameListEntry> = match serde_json::from_str(&gamelist_str) {
        Ok(g) => g,
        Err(_) => {
            return Library {
                games: Vec::new(),
                path: path_str,
            }
        }
    };

    let mut games = Vec::new();

    for entry in &gamelist {
        // Detect folder entries (name starts with "FOLDER")
        if entry.folder.starts_with("FOLDER") {
            let folder_path = lineup_path.join(&entry.folder);
            if !folder_path.is_dir() {
                continue;
            }

            // Count games in subfolder
            let sub_gamelist_path = folder_path.join("gamelist.json");
            let game_count = if sub_gamelist_path.exists() {
                fs::read_to_string(&sub_gamelist_path)
                    .ok()
                    .and_then(|s| serde_json::from_str::<Vec<GameListEntry>>(&s).ok())
                    .map(|g| g.len())
                    .unwrap_or(0)
            } else {
                0
            };

            let has_cover = folder_path.join("cover.png").exists();

            // Read display name from folder.json, fall back to folder name
            let display_name = folder_path.join("folder.json")
                .exists()
                .then(|| {
                    fs::read_to_string(folder_path.join("folder.json"))
                        .ok()
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                        .and_then(|v| v["name"].as_str().map(|s| s.to_string()))
                })
                .flatten()
                .unwrap_or_else(|| entry.folder.clone());

            let game = GameJson {
                display: GameDisplay {
                    name: display_name.clone(),
                    tname: display_name.clone(),
                    name_eng: display_name,
                    titlebar: 4,
                    ccolor: 0,
                    csize: 0,
                    players: 0,
                    demo_time: 0,
                },
                emulator: GameEmulator::default(),
                rom: GameRom {
                    arch: "folder".to_string(),
                    country: String::new(),
                    preamp: 0.0,
                    rom: String::new(),
                    ..Default::default()
                },
                cover: Some("cover.png".to_string()),
                cover_size: None,
            };

            games.push(GameEntry {
                folder: entry.folder.clone(),
                game,
                has_cover,
                sort: entry.clone(),
                is_folder: true,
                game_count,
            });
            continue;
        }

        let game_json_path = lineup_path.join(&entry.folder).join("game.json");
        if !game_json_path.exists() {
            continue;
        }

        let game_str = match fs::read_to_string(&game_json_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let game: GameJson = match serde_json::from_str(&game_str) {
            Ok(g) => g,
            Err(_) => continue,
        };

        let cover_file = game.cover.clone().unwrap_or_else(|| "cover.png".to_string());
        let has_cover = lineup_path.join(&entry.folder).join(&cover_file).exists();

        games.push(GameEntry {
            folder: entry.folder.clone(),
            game,
            has_cover,
            sort: entry.clone(),
            is_folder: false,
            game_count: 0,
        });
    }

    Library {
        games,
        path: path_str,
    }
}

// --- Helper: save a single lineup to a subdirectory ---

fn save_lineup(lineup_path: &Path, library: &Library) -> Result<(), String> {
    // Create lineup dir if needed
    if !lineup_path.exists() {
        fs::create_dir_all(lineup_path)
            .map_err(|e| format!("Failed to create {}: {}", lineup_path.display(), e))?;
    }

    // Build gamelist.json from library order
    let gamelist: Vec<GameListEntry> = library.games.iter().map(|g| g.sort.clone()).collect();

    let gamelist_json = serde_json::to_string_pretty(&gamelist)
        .map_err(|e| format!("Failed to serialize gamelist: {}", e))?;
    fs::write(lineup_path.join("gamelist.json"), gamelist_json)
        .map_err(|e| format!("Failed to write gamelist.json: {}", e))?;

    for entry in &library.games {
        let game_dir = lineup_path.join(&entry.folder);
        if !game_dir.exists() {
            fs::create_dir_all(&game_dir)
                .map_err(|e| format!("Failed to create {}: {}", entry.folder, e))?;
        }

        if entry.is_folder {
            // Save folder display name as folder.json
            let folder_meta = serde_json::json!({
                "name": entry.game.display.name_eng
            });
            let meta_json = serde_json::to_string_pretty(&folder_meta)
                .map_err(|e| format!("Failed to serialize {}/folder.json: {}", entry.folder, e))?;
            fs::write(game_dir.join("folder.json"), meta_json)
                .map_err(|e| format!("Failed to write {}/folder.json: {}", entry.folder, e))?;
        } else {
            let game_json = serde_json::to_string_pretty(&entry.game)
                .map_err(|e| format!("Failed to serialize {}/game.json: {}", entry.folder, e))?;
            fs::write(game_dir.join("game.json"), game_json)
                .map_err(|e| format!("Failed to write {}/game.json: {}", entry.folder, e))?;
        }
    }

    Ok(())
}

// --- Tauri commands ---

/// Resolve the wrapper-vs-legacy layout from a picked path.
///
/// Wrapper layout (canonical):
///   <picked>/
///     library/
///       jp/         <- exploded JP lineup
///       us/         <- exploded US lineup
///       published/  <- publish output (sibling of jp/us)
///       templates/  <- stock PSB templates (sibling of jp/us)
///
/// Legacy: the picked path itself contains jp/ and us/ directly. We
/// detect via the presence of `<picked>/library/` — if it's a directory,
/// wrapper mode; otherwise legacy.
///
/// Returns (library_root, published_root, templates_root). Callers use
/// the parts they need.
fn resolve_library_paths(picked: &Path) -> (PathBuf, PathBuf, PathBuf) {
    if picked.join("library").is_dir() {
        // wrapper mode: published + templates sit inside library/ alongside jp/us
        let lib = picked.join("library");
        let published = lib.join("published");
        let templates = lib.join("templates");
        (lib, published, templates)
    } else {
        // legacy: <picked> is the library root, published/templates as siblings of jp/us
        (
            picked.to_path_buf(),
            picked.join("published"),
            picked.join("templates"),
        )
    }
}

#[tauri::command]
fn load_library(games_path: String) -> Result<DualLibrary, String> {
    let (library_root, _, _) = resolve_library_paths(Path::new(&games_path));

    let jp = load_lineup(&library_root.join("jp"));
    let us = load_lineup(&library_root.join("us"));

    Ok(DualLibrary {
        jp,
        us,
        // Preserve the resolved library_root in `path` — downstream commands
        // (save_library, get_cover, import_*) all key off it.
        path: library_root.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
fn save_library(games_path: String, library: DualLibrary) -> Result<(), String> {
    // games_path here comes from the loaded DualLibrary's path which is
    // already the resolved library_root — no further resolution needed.
    let base = Path::new(&games_path);

    save_lineup(&base.join("jp"), &library.jp)?;
    save_lineup(&base.join("us"), &library.us)?;

    Ok(())
}

#[tauri::command]
fn get_cover(
    games_path: String,
    lineup: String,
    folder: String,
    filename: String,
) -> Result<String, String> {
    let cover_path = Path::new(&games_path)
        .join(&lineup)
        .join(&folder)
        .join(&filename);
    if !cover_path.exists() {
        return Err("Cover file not found".to_string());
    }

    let data =
        fs::read(&cover_path).map_err(|e| format!("Failed to read cover: {}", e))?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    Ok(format!("data:image/png;base64,{}", b64))
}

#[tauri::command]
fn import_file(src_path: String, dest_dir: String, filename: String) -> Result<String, String> {
    let dir = Path::new(&dest_dir);
    fs::create_dir_all(dir)
        .map_err(|e| format!("create_dir_all({}) failed: {}", dir.display(), e))?;
    let dest = dir.join(&filename);
    fs::copy(&src_path, &dest)
        .map_err(|e| format!("copy({} -> {}) failed: {}", src_path, dest.display(), e))?;
    Ok(dest.to_string_lossy().to_string())
}

#[tauri::command]
fn import_cover(
    src_path: String,
    dest_dir: String,
    filename: String,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<String, String> {
    let w = width.unwrap_or(240);
    let h = height.unwrap_or(240);

    let img = image::open(&src_path)
        .map_err(|e| format!("Failed to open image: {}", e))?;

    let resized = img.resize_exact(w, h, image::imageops::FilterType::Lanczos3);

    fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("Failed to create directory: {}", e))?;

    let dest = Path::new(&dest_dir).join(&filename);
    resized
        .save(&dest)
        .map_err(|e| format!("Failed to save cover: {}", e))?;

    Ok(dest.to_string_lossy().to_string())
}

/// Compose a 240×240 folder cover by tiling 4 game covers into a 2×2
/// mosaic. `folder_dir` is the absolute path to the folder's library
/// directory (e.g. `library/jp/FOLDER01`). `game_dir_names` is the
/// ordered list of 4 game subdirectory names INSIDE that folder, top-left,
/// top-right, bottom-left, bottom-right. The result overwrites
/// `<folder_dir>/cover.png`.
///
/// Each tile is the matching game's `cover.png` resized to 120×120 with
/// Lanczos3. Games can override their cover filename via `game.json::cover`
/// (the same fallback chain used elsewhere in the publisher).
#[tauri::command]
fn generate_folder_mosaic_cover(
    folder_dir: String,
    game_dir_names: Vec<String>,
) -> Result<String, String> {
    if game_dir_names.len() != 4 {
        return Err(format!(
            "need exactly 4 game dir names for a 2x2 mosaic, got {}",
            game_dir_names.len()
        ));
    }
    let folder = PathBuf::from(folder_dir);
    if !folder.is_dir() {
        return Err(format!("folder does not exist: {}", folder.display()));
    }

    let mut mosaic: image::RgbaImage = image::ImageBuffer::new(240, 240);
    let positions: [(i64, i64); 4] = [(0, 0), (120, 0), (0, 120), (120, 120)];

    for (i, name) in game_dir_names.iter().enumerate() {
        let game_dir = folder.join(name);
        if !game_dir.is_dir() {
            return Err(format!("game dir does not exist: {}", game_dir.display()));
        }
        // Honour game.json::cover override; fall back to cover.png.
        let cover_filename = fs::read_to_string(game_dir.join("game.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<GameJson>(&s).ok())
            .and_then(|g| g.cover)
            .unwrap_or_else(|| "cover.png".to_string());
        let cover_path = game_dir.join(&cover_filename);
        let img = image::open(&cover_path)
            .map_err(|e| format!("open {}: {}", cover_path.display(), e))?;
        let tile = img.resize_exact(120, 120, image::imageops::FilterType::Lanczos3).to_rgba8();
        let (dx, dy) = positions[i];
        image::imageops::overlay(&mut mosaic, &tile, dx, dy);
    }

    let out = folder.join("cover.png");
    mosaic.save(&out).map_err(|e| format!("save {}: {}", out.display(), e))?;
    Ok(out.to_string_lossy().to_string())
}

#[tauri::command]
fn create_game(
    games_path: String,
    lineup: String,
    folder_name: String,
) -> Result<GameEntry, String> {
    let base = Path::new(&games_path).join(&lineup);
    let game_dir = base.join(&folder_name);

    if game_dir.exists() {
        return Err(format!("Folder '{}' already exists", folder_name));
    }

    fs::create_dir_all(&game_dir)
        .map_err(|e| format!("Failed to create folder: {}", e))?;

    let is_us = lineup == "us" || lineup.starts_with("us/");
    let default_country = if is_us { "us" } else { "jp" };
    let default_titlebar: u32 = if is_us { 9 } else { 4 };

    let game = GameJson {
        display: GameDisplay {
            name: String::new(),
            tname: String::new(),
            name_eng: String::new(),
            titlebar: default_titlebar,
            ccolor: 0,
            csize: 0,
            players: 1,
            demo_time: 6000,
        },
        emulator: GameEmulator::default(),
        rom: GameRom {
            arch: "tg16".to_string(),
            country: default_country.to_string(),
            preamp: 0.0,
            rom: String::new(),
            ..Default::default()
        },
        cover: Some("cover.png".to_string()),
        cover_size: None,
    };

    let game_json = serde_json::to_string_pretty(&game)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    fs::write(game_dir.join("game.json"), game_json)
        .map_err(|e| format!("Failed to write game.json: {}", e))?;

    let sort = GameListEntry {
        folder: folder_name.clone(),
        sor_date: 0,
        sor_demo: 0,
        sor_genr: 0,
        sor_name: 0,
        sor_pnum: 0,
    };

    Ok(GameEntry {
        folder: folder_name,
        game,
        has_cover: false,
        sort,
        is_folder: false,
        game_count: 0,
    })
}

#[tauri::command]
fn delete_game(games_path: String, lineup: String, folder_name: String) -> Result<(), String> {
    let game_dir = Path::new(&games_path).join(&lineup).join(&folder_name);
    if !game_dir.exists() {
        return Err("Folder does not exist".to_string());
    }
    fs::remove_dir_all(&game_dir)
        .map_err(|e| format!("Failed to delete folder: {}", e))?;
    Ok(())
}


#[tauri::command]
fn list_files_in_folder(
    folder_path: String,
    extensions: Vec<String>,
) -> Result<Vec<String>, String> {
    let dir = Path::new(&folder_path);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    let entries =
        fs::read_dir(dir).map_err(|e| format!("Failed to read directory: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Dir entry error: {}", e))?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                let ext_lower = ext.to_string_lossy().to_lowercase();
                if extensions.iter().any(|e| e.to_lowercase() == ext_lower) {
                    if let Some(name) = path.file_name() {
                        files.push(name.to_string_lossy().to_string());
                    }
                }
            }
        }
    }

    files.sort();
    Ok(files)
}

#[tauri::command]
fn load_folder_contents(
    games_path: String,
    lineup: String,
    folder_name: String,
) -> Result<Library, String> {
    let folder_path = Path::new(&games_path).join(&lineup).join(&folder_name);
    if !folder_path.is_dir() {
        return Err(format!("Folder '{}' does not exist", folder_name));
    }
    Ok(load_lineup(&folder_path))
}

#[tauri::command]
fn save_folder_contents(
    games_path: String,
    lineup: String,
    folder_name: String,
    library: Library,
) -> Result<(), String> {
    let folder_path = Path::new(&games_path).join(&lineup).join(&folder_name);
    save_lineup(&folder_path, &library)
}

#[tauri::command]
fn create_folder(
    games_path: String,
    lineup: String,
    folder_name: String,
) -> Result<(), String> {
    let folder_path = Path::new(&games_path).join(&lineup).join(&folder_name);
    if folder_path.exists() {
        return Err(format!("Folder '{}' already exists", folder_name));
    }

    fs::create_dir_all(&folder_path)
        .map_err(|e| format!("Failed to create folder: {}", e))?;

    // Write empty gamelist.json
    fs::write(folder_path.join("gamelist.json"), "[]")
        .map_err(|e| format!("Failed to write gamelist.json: {}", e))?;

    // Copy default folder cover if available
    let default_cover = Path::new(&games_path).join("folder_default.png");
    if default_cover.exists() {
        let _ = fs::copy(&default_cover, folder_path.join("cover.png"));
    }

    Ok(())
}

#[tauri::command]
fn delete_folder(
    games_path: String,
    lineup: String,
    folder_name: String,
) -> Result<(), String> {
    let folder_path = Path::new(&games_path).join(&lineup).join(&folder_name);
    if !folder_path.exists() {
        return Err("Folder does not exist".to_string());
    }
    fs::remove_dir_all(&folder_path)
        .map_err(|e| format!("Failed to delete folder: {}", e))?;
    Ok(())
}

// ---- Load a single game entry from disk ----
// Used after move_game to re-read the entry with updated country field.

#[tauri::command]
fn load_game_entry(
    games_path: String,
    lineup: String,
    folder_name: String,
) -> Result<GameEntry, String> {
    let game_dir = Path::new(&games_path).join(&lineup).join(&folder_name);
    let game_json_path = game_dir.join("game.json");
    if !game_json_path.exists() {
        return Err(format!("game.json not found in {}/{}", lineup, folder_name));
    }
    let game_str = fs::read_to_string(&game_json_path)
        .map_err(|e| format!("Failed to read game.json: {}", e))?;
    let game: GameJson = serde_json::from_str(&game_str)
        .map_err(|e| format!("Failed to parse game.json: {}", e))?;
    let cover_file = game.cover.clone().unwrap_or_else(|| "cover.png".to_string());
    let has_cover = game_dir.join(&cover_file).exists();

    Ok(GameEntry {
        folder: folder_name.clone(),
        game,
        has_cover,
        sort: GameListEntry {
            folder: folder_name,
            sor_date: 0,
            sor_demo: 0,
            sor_genr: 0,
            sor_name: 0,
            sor_pnum: 0,
        },
        is_folder: false,
        game_count: 0,
    })
}

// ---- Move game between lineups/folders ----
// Moves a game directory from src_lineup to dest_lineup using fs::rename (instant, same FS).
// Auto-suffixes the folder name on conflict (-2, -3, etc.).
// Updates the country field in game.json to match the destination lineup.
// Returns the final folder name (may differ from original if renamed).

#[tauri::command]
fn move_game(
    games_path: String,
    src_lineup: String,
    folder_name: String,
    dest_lineup: String,
) -> Result<String, String> {
    let src_path = Path::new(&games_path).join(&src_lineup).join(&folder_name);
    if !src_path.exists() {
        return Err(format!("Source folder '{}' does not exist", folder_name));
    }

    // Find a unique destination folder name (auto-suffix on conflict)
    let dest_base = Path::new(&games_path).join(&dest_lineup);
    let mut dest_name = folder_name.clone();
    let mut suffix = 2u32;
    while dest_base.join(&dest_name).exists() {
        dest_name = format!("{}-{}", folder_name, suffix);
        suffix += 1;
    }
    let dest_path = dest_base.join(&dest_name);

    // Move the directory
    fs::rename(&src_path, &dest_path)
        .map_err(|e| format!("Failed to move game folder: {}", e))?;

    // Update country in game.json to match destination lineup
    let new_country = if dest_lineup == "us" || dest_lineup.starts_with("us/") { "us" } else { "jp" };
    let game_json_path = dest_path.join("game.json");
    if game_json_path.exists() {
        if let Ok(content) = fs::read_to_string(&game_json_path) {
            if let Ok(mut game) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(rom) = game.get_mut("rom") {
                    rom["country"] = serde_json::Value::String(new_country.to_string());
                }
                if let Ok(updated) = serde_json::to_string_pretty(&game) {
                    let _ = fs::write(&game_json_path, updated);
                }
            }
        }
    }

    Ok(dest_name)
}

// ---- Editor Settings (stored next to exe as settings_editor.json) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorSettings {
    #[serde(default = "default_true")]
    pub confirm_delete: bool,
    #[serde(default = "default_true")]
    pub force_us_titlebar: bool,
    #[serde(default)]
    pub games_path: Option<String>,
    #[serde(default)]
    pub bios: Option<m2_import::BiosConfig>,
}

fn default_true() -> bool { true }

impl Default for EditorSettings {
    fn default() -> Self {
        EditorSettings {
            confirm_delete: true,
            force_us_titlebar: true,
            games_path: None,
            bios: None,
        }
    }
}

// ---- Games Settings (stored in games folder as settings_games.json) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamesSettings {
    #[serde(default = "default_genres")]
    pub genres: Vec<String>,
    #[serde(default)]
    pub alpha_exclusions: Vec<String>,
}

fn default_genres() -> Vec<String> {
    vec![
        "Shooter".into(), "Action".into(), "Fighter".into(), "Sport".into(),
        "Puzzle".into(), "Racing".into(), "RPG".into(), "Simulation".into(),
        "Adventure".into(), "ETC.".into(),
    ]
}

impl Default for GamesSettings {
    fn default() -> Self {
        GamesSettings {
            genres: default_genres(),
            alpha_exclusions: Vec::new(),
        }
    }
}

fn portable_settings_path() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Failed to get exe path: {}", e))?;
    let dir = exe.parent().ok_or("Failed to get exe directory")?;
    Ok(dir.join("settings_editor.json"))
}

fn editor_settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("settings_editor.json"))
}

fn legacy_settings_path() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Failed to get exe path: {}", e))?;
    let dir = exe.parent().ok_or("Failed to get exe directory")?;
    Ok(dir.join("settings.json"))
}

#[tauri::command]
fn load_editor_settings(app: tauri::AppHandle) -> Result<EditorSettings, String> {
    let path = editor_settings_path(&app)?;
    if path.exists() {
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read editor settings: {}", e))?;
        return serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse editor settings: {}", e));
    }
    // Migrate previous settings out of the application bundle so upgrades keep them.
    let portable = portable_settings_path()?;
    if portable.exists() {
        let settings: EditorSettings = serde_json::from_slice(&fs::read(portable).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        save_editor_settings(app, settings.clone())?;
        return Ok(settings);
    }
    // Migrate from old settings.json if it exists
    let legacy = legacy_settings_path()?;
    if legacy.exists() {
        let content = fs::read_to_string(&legacy)
            .map_err(|e| format!("Failed to read legacy settings: {}", e))?;
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            let settings = EditorSettings {
                confirm_delete: v.get("confirm_delete").and_then(|v| v.as_bool()).unwrap_or(true),
                force_us_titlebar: v.get("force_us_titlebar").and_then(|v| v.as_bool()).unwrap_or(true),
                games_path: v.get("games_path").and_then(|v| v.as_str()).map(String::from),
                bios: None,
            };
            return Ok(settings);
        }
    }
    Ok(EditorSettings::default())
}

#[tauri::command]
fn save_editor_settings(app: tauri::AppHandle, settings: EditorSettings) -> Result<(), String> {
    let path = editor_settings_path(&app)?;
    let json = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize editor settings: {}", e))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, json).and_then(|()| fs::rename(&temporary, &path))
        .map_err(|e| format!("Failed to write editor settings: {}", e))
}

#[tauri::command]
fn load_games_settings(games_path: String) -> Result<GamesSettings, String> {
    let path = std::path::Path::new(&games_path).join("settings_games.json");
    if !path.exists() {
        return Ok(GamesSettings::default());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read games settings: {}", e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse games settings: {}", e))
}

#[tauri::command]
fn save_games_settings(games_path: String, settings: GamesSettings) -> Result<(), String> {
    let path = std::path::Path::new(&games_path).join("settings_games.json");
    let json = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize games settings: {}", e))?;
    fs::write(&path, json)
        .map_err(|e| format!("Failed to write games settings: {}", e))
}

// ---- Save States ----
mod save_states;

#[tauri::command]
fn get_save_states(game_path: String) -> Result<save_states::GameSaves, String> {
    save_states::read_game_saves(Path::new(&game_path))
}

// ---- Sync (published -> library) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub backup_flags_updated: bool,
    pub sram_blocks_imported: usize,
    pub sram_blocks_unmapped: usize,
    pub state_files_imported: usize,
    pub state_files_unmapped: usize,
}

/// Reorganise the USB's `published/` engine-format files into the per-game
/// `library/` view. Called from the editor at "Open library" time, before
/// any user edits. Idempotent.
///
/// `library_root` can be either the wrapper folder (`<lib>/`) or the
/// exploded library subfolder (`<lib>/library/` or a legacy direct path).
/// `published_root` is optional — if empty, derived as sibling.
#[tauri::command]
fn sync_library(library_root: String, published_root: String) -> Result<SyncResult, String> {
    let (lib, pub_default, _) = resolve_library_paths(Path::new(&library_root));
    let pub_root = if published_root.is_empty() {
        pub_default
    } else {
        PathBuf::from(published_root)
    };
    let opts = m2_publish::SyncOptions {
        library_root: lib,
        published_root: pub_root,
    };
    match m2_publish::sync_library_from_published(&opts) {
        Ok(r) => Ok(SyncResult {
            backup_flags_updated: r.backup_flags_updated,
            sram_blocks_imported: r.sram_blocks_imported,
            sram_blocks_unmapped: r.sram_blocks_unmapped,
            state_files_imported: r.state_files_imported,
            state_files_unmapped: r.state_files_unmapped,
        }),
        Err(e) => Err(format!("sync failed: {}", e)),
    }
}

// ---- Library initialization (first-run setup) ----

/// Return the directory the editor executable lives in. In the deployed
/// workflow this is the USB root (the editor binary sits next to
/// `library/`, `published/`, `templates/`, `BACKUP/`). In dev mode
/// (`npm run dev`) this points at `target/debug/` — frontend should
/// gracefully fall back to a saved `games_path` setting in that case.
#[tauri::command]
fn get_editor_root() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = exe.parent().ok_or("editor has no parent dir")?;
    // On macOS the binary lives inside `<USB>/PCE Game Editor.app/Contents/MacOS/`.
    // Walk up to the .app's parent so the USB root resolves cleanly.
    let mut cur = parent.to_path_buf();
    if cur.ends_with("MacOS") {
        if let Some(p) = cur.parent() { cur = p.to_path_buf(); }      // Contents
        if cur.ends_with("Contents") {
            if let Some(p) = cur.parent() { cur = p.to_path_buf(); }  // *.app
            if cur.extension().map_or(false, |e| e == "app") {
                if let Some(p) = cur.parent() { cur = p.to_path_buf(); }
            }
        }
    }
    Ok(cur.to_string_lossy().into_owned())
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryStatus {
    /// Wrapper folder exists and contains a usable `library/` subdir.
    pub has_library: bool,
    /// `BACKUP/game/` with the PSB templates we need is present.
    pub has_backup: bool,
    /// Specific files we'd copy as templates. Useful for diagnostics.
    pub missing_templates: Vec<String>,
}

const REQUIRED_TEMPLATES: &[&str] = &[
    "040/config/title_prof.psb.m",
    "040/config/title_mode_top.psb.m",
    "040/motion/title_jp_titleselect_jp.psb.m",
    "040/motion/title_jp_titleselect_us.psb.m",
    "system/font/makoto_basefont.psb.m",
    "system/font/makoto_basefont_18pt.psb.m",
    "system/font/makoto_basefont_32pt.psb.m",
    "system/motion/titleselect_ui.psb.m",
];

/// Examine a wrapper folder and report whether init is needed and possible.
/// Called from the frontend on app launch to decide between "load existing"
/// and "prompt to initialize empty library".
#[tauri::command]
fn check_library_status(usb_root: String) -> Result<LibraryStatus, String> {
    let root = Path::new(&usb_root);
    let valid = |p: &Path| p.join("jp/gamelist.json").is_file() && p.join("us/gamelist.json").is_file();
    let has_library = valid(&root.join("library")) || valid(root);
    let backup_game = root.join("BACKUP").join("game");
    let has_backup = backup_game.is_dir();
    let mut missing = Vec::new();
    if has_backup {
        for rel in REQUIRED_TEMPLATES {
            if !backup_game.join(rel).is_file() {
                missing.push((*rel).to_string());
            }
        }
    }
    Ok(LibraryStatus { has_library, has_backup, missing_templates: missing })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitResult {
    pub library_root: String,
    pub templates_copied: usize,
}

/// Create an empty library at `<usb_root>/library/` plus the sibling
/// `published/` and `templates/` directories. PSB templates are copied
/// from `<usb_root>/BACKUP/game/`. The library lineups are bootstrapped
/// with empty `gamelist.json` files. No games extracted.
#[tauri::command]
fn init_library(usb_root: String) -> Result<InitResult, String> {
    let root = Path::new(&usb_root);
    let backup_game = root.join("BACKUP").join("game");
    if !backup_game.is_dir() {
        return Err(format!("BACKUP/game/ not found under {}", usb_root));
    }
    // Verify templates exist before doing anything.
    for rel in REQUIRED_TEMPLATES {
        if !backup_game.join(rel).is_file() {
            return Err(format!("required template missing: BACKUP/game/{}", rel));
        }
    }

    let library_root = root.join("library");
    let published_root = library_root.join("published");
    let templates_root = library_root.join("templates");

    // Lineup skeletons with empty gamelists.
    for lineup in &["jp", "us"] {
        let dir = library_root.join(lineup);
        fs::create_dir_all(&dir)
            .map_err(|e| format!("create {}: {}", dir.display(), e))?;
        let gamelist = dir.join("gamelist.json");
        if !gamelist.exists() {
            fs::write(&gamelist, "[]")
                .map_err(|e| format!("write {}: {}", gamelist.display(), e))?;
        }
    }
    fs::create_dir_all(&published_root)
        .map_err(|e| format!("create {}: {}", published_root.display(), e))?;
    fs::create_dir_all(&templates_root)
        .map_err(|e| format!("create {}: {}", templates_root.display(), e))?;

    // Copy template PSBs. Preserves the relative path under templates/ so the
    // publisher's existing template lookup (relative_path under stock_data_root)
    // works unchanged.
    let mut copied = 0;
    for rel in REQUIRED_TEMPLATES {
        let src = backup_game.join(rel);
        let dst = templates_root.join(rel);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {}", parent.display(), e))?;
        }
        fs::copy(&src, &dst)
            .map_err(|e| format!("copy template {}: {}", rel, e))?;
        copied += 1;
    }

    Ok(InitResult {
        library_root: library_root.to_string_lossy().into_owned(),
        templates_copied: copied,
    })
}

// ---- Publish ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishResult {
    pub roms_packed: usize,
    pub roms_copied: usize,
    pub psb_files_written: usize,
    pub folders_emitted: usize,
    pub save_files_copied: usize,
    pub sram_blocks_embedded: usize,
    pub sram_blocks_skipped_wrong_size: usize,
    pub data_008_emitted: bool,
    pub output_root: String,
    pub usb: Option<m2_publish::UsbPreparation>,
}

/// Publish the library at `games_path` into a deployable m2engage tree.
///
/// In wrapper layout, `games_path` is the wrapper folder and both
/// `stock_data_root` and `output_root` can be left empty — they auto-resolve
/// to `<wrapper>/library/templates/` and `<wrapper>/library/published/`.
/// Canonical USB publication also installs the bundled Chronos scripts/hook
/// in `<wrapper>/game/` and creates its ROM mount point, without copying ROMs.
/// Caller may override either by passing a non-empty path.
#[tauri::command]
fn publish_library(
    games_path: String,
    stock_data_root: String,
    output_root: String,
) -> Result<PublishResult, String> {
    let (lib, pub_default, tmpl_default) = resolve_library_paths(Path::new(&games_path));
    let stock = if stock_data_root.is_empty() {
        tmpl_default
    } else {
        PathBuf::from(stock_data_root)
    };
    let out = if output_root.is_empty() {
        pub_default
    } else {
        PathBuf::from(&output_root)
    };
    let resolved_output_str = out.to_string_lossy().into_owned();
    let opts = m2_publish::PublishOptions {
        library_root: lib,
        stock_data_root: stock,
        output_root: out,
        incremental: false,
    };
    match m2_publish::publish(&opts) {
        Ok(report) => Ok(PublishResult {
            roms_packed: report.roms_packed,
            roms_copied: report.roms_copied,
            psb_files_written: report.psb_files_written,
            folders_emitted: report.folders_emitted,
            save_files_copied: report.save_files_copied,
            sram_blocks_embedded: report.sram_blocks_embedded,
            sram_blocks_skipped_wrong_size: report.sram_blocks_skipped_wrong_size,
            data_008_emitted: report.data_008_emitted,
            output_root: resolved_output_str,
            usb: report.usb,
        }),
        Err(e) => Err(format!("publish failed: {}", e)),
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            font_commands::japanese_font,
            font_commands::title_preview,
            load_library,
            save_library,
            get_cover,
            import_file,
            import_cover,
            generate_folder_mosaic_cover,
            create_game,
            delete_game,
            list_files_in_folder,
            load_folder_contents,
            save_folder_contents,
            create_folder,
            delete_folder,
            load_game_entry,
            move_game,
            load_editor_settings,
            save_editor_settings,
            load_games_settings,
            save_games_settings,
            get_save_states,
            publish_library,
            sync_library,
            check_library_status,
            init_library,
            get_editor_root,
            import_commands::import_rom,
            import_commands::configure_bios,
            import_commands::get_bios_status,
            import_commands::create_library_from_dump,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
