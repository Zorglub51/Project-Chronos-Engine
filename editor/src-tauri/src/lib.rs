use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;
mod font_commands;
mod import_commands;
mod library_move;
mod partition_commands;

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
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoverSize {
    pub width: u32,
    pub height: u32,
    #[serde(rename = "originX")]
    pub origin_x: u32,
    #[serde(rename = "originY")]
    pub origin_y: u32,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_region_tag: Option<String>,
    pub display: GameDisplay,
    #[serde(default)]
    pub emulator: GameEmulator,
    pub rom: GameRom,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_size: Option<CoverSize>,
    // Editor genre IDs are persisted independently of console publication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<u32>,
    // Preserve native profiles, alternate ROM settings and future fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    #[serde(default)]
    pub source_schema: u32,
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
            let display_name = folder_path
                .join("folder.json")
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
                game_id: None,
                native_region_tag: None,
                display: GameDisplay {
                    name: display_name.clone(),
                    tname: display_name.clone(),
                    name_eng: display_name,
                    titlebar: 4,
                    ccolor: 0,
                    csize: 0,
                    players: 0,
                    demo_time: 0,
                    ..Default::default()
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
                genre: None,
                extra: Default::default(),
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

        let cover_file = game
            .cover
            .clone()
            .unwrap_or_else(|| "cover.png".to_string());
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

    let gamelist_json = serde_json::to_value(&gamelist)
        .map_err(|e| format!("Failed to serialize gamelist: {}", e))?;

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
            save_game_json(&game_dir.join("game.json"), &entry.game)?;
        }
    }

    m2_publish::identity::atomic_write_json(&lineup_path.join("gamelist.json"), &gamelist_json)?;

    Ok(())
}

fn save_game_json(path: &Path, game: &GameJson) -> Result<(), String> {
    // The same lock covers manual PNG imports and console image preparation.
    // Do not let an importer prepare against half of a geometry update.
    let _cover_update = import_commands::lock_cover_update()?;
    let previous: Option<serde_json::Value> = if path.exists() {
        Some(serde_json::from_slice(&fs::read(path).map_err(|e| {
            format!("Read {} before saving: {e}", path.display())
        })?).map_err(|e| format!("Invalid metadata in {}: {e}", path.display()))?)
    } else {
        None
    };
    let mut data =
        serde_json::to_value(game).map_err(|e| format!("Serialize {}: {e}", path.display()))?;
    m2_publish::parameters::validate_game(&data)?;
    m2_publish::parameters::normalize_game(&mut data);
    if path.exists() {
        let stable = m2_publish::identity::ensure_game_identity(path)?;
        if game
            .game_id
            .as_ref()
            .is_some_and(|id| id != &stable.game_id)
            || game
                .native_region_tag
                .as_ref()
                .is_some_and(|tag| tag != &stable.native_region_tag)
        {
            return Err(format!("The game identity in {} changed during editing. Reopen the library before saving; save identities are immutable.", path.display()));
        }
        data["game_id"] = serde_json::Value::String(stable.game_id);
        data["native_region_tag"] = serde_json::Value::String(stable.native_region_tag);
    }
    m2_publish::identity::normalize_game_json(path, &mut data)?;
    let cover_inputs = |value: &serde_json::Value| {
        serde_json::json!({
            "cover":value["cover"],
            "cover_size":value["cover_size"],
            "ccolor":value["display"]["ccolor"].as_u64().unwrap_or(0),
        })
    };
    let entry = path.parent().ok_or_else(|| "Game metadata directory is missing".to_string())?;
    let source_mode = m2_publish::rom_store::library_root(entry)
        .is_some_and(|root| m2_publish::source_library::enabled(&root));
    let changed = previous.as_ref().map(cover_inputs) != Some(cover_inputs(&data));
    let dirty = entry.join("runtime/.chronos-cover-dirty");
    if source_mode && changed {
        // Persist the retry signal before new JSON becomes visible. If image
        // generation fails, the next save must not claim the stale assets are
        // current just because their metadata now has the same geometry.
        mark_cover_dirty(entry)?;
    }
    m2_publish::identity::atomic_write_json(path, &data)?;
    if source_mode && (changed || dirty.exists()) {
        prepare_changed_cover(entry).map_err(|e| {
            format!("Game metadata saved, but console artwork could not be updated: {e}")
        })?;
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
async fn load_library(
    games_path: String,
    on_progress: tauri::ipc::Channel<m2_import::Progress>,
) -> Result<DualLibrary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (library_root, _, _) = resolve_library_paths(Path::new(&games_path));
        m2_publish::identity::migrate_library(&library_root)?;
        m2_publish::rom_store::migrate_with_progress(&library_root, &|name, completed, total| {
            let _ = on_progress.send(m2_import::Progress {
                message: format!("Checking shared ROM storage: {name}"),
                completed: Some(completed as u64), total: Some(total as u64),
            });
        }).map_err(|e| format!("ROM migration stopped; no unverified source was deleted: {e}"))?;
        if m2_publish::source_library::can_prepare(&library_root) {
            let _ = on_progress.send(m2_import::Progress {
                message:"Updating console resources…".into(), completed:None,total:None,
            });
            m2_publish::source_library::prepare(&library_root, &library_root.join("templates"))
                .map_err(|e| format!("Console runtime update failed; source library and original save archives are retained: {e}"))?;
        }
        Ok(DualLibrary {
            jp: load_lineup(&library_root.join("jp")),
            us: load_lineup(&library_root.join("us")),
            path: library_root.to_string_lossy().into_owned(),
            source_schema: if m2_publish::source_library::enabled(&library_root) { 2 } else { 1 },
        })
    }).await.map_err(|e| format!("Library opening stopped: {e}"))?
}

#[tauri::command]
async fn save_library(games_path: String, library: DualLibrary) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        // games_path here comes from the loaded DualLibrary's path which is
        // already the resolved library_root — no further resolution needed.
        let base = Path::new(&games_path);

        save_lineup(&base.join("jp"), &library.jp)?;
        save_lineup(&base.join("us"), &library.us)?;
        m2_publish::source_library::metadata_changed(base).map_err(|e| {
            format!("Library metadata saved, but console fonts could not be updated: {e}")
        })?;

        Ok(())
    })
    .await
    .map_err(|e| format!("Library update stopped: {e}"))?
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

    let data = fs::read(&cover_path).map_err(|e| format!("Failed to read cover: {}", e))?;

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
async fn import_cover(
    src_path: String,
    dest_dir: String,
    filename: String,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _cover_update = import_commands::lock_cover_update()?;
        if Path::new(&filename)
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err("Cover filename must stay inside its game or folder".to_string());
        }
        let w = width.unwrap_or(240);
        let h = height.unwrap_or(240);

        let img = image::open(&src_path).map_err(|e| format!("Failed to open image: {}", e))?;

        let resized = img.resize_exact(w, h, image::imageops::FilterType::Lanczos3);

        fs::create_dir_all(&dest_dir).map_err(|e| format!("Failed to create directory: {}", e))?;

        let dest = Path::new(&dest_dir).join(&filename);
        mark_cover_dirty(Path::new(&dest_dir))?;
        import_commands::save_cover_png(&resized, &dest)?;
        prepare_changed_cover(Path::new(&dest_dir))?;

        Ok(dest.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("Cover update stopped: {e}"))?
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
async fn generate_folder_mosaic_cover(
    folder_dir: String,
    game_dir_names: Vec<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _cover_update = import_commands::lock_cover_update()?;
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
            let tile = img
                .resize_exact(120, 120, image::imageops::FilterType::Lanczos3)
                .to_rgba8();
            let (dx, dy) = positions[i];
            image::imageops::overlay(&mut mosaic, &tile, dx, dy);
        }

        let out = folder.join("cover.png");
        mark_cover_dirty(&folder)?;
        import_commands::save_cover_png(&image::DynamicImage::ImageRgba8(mosaic), &out)?;
        prepare_changed_cover(&folder)?;
        Ok(out.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("Folder cover update stopped: {e}"))?
}

#[tauri::command]
fn create_game(
    games_path: String,
    lineup: String,
    folder_name: String,
) -> Result<GameEntry, String> {
    let base = Path::new(&games_path).join(&lineup);
    m2_publish::identity::migrate_library(Path::new(&games_path))?;
    let game_dir = base.join(&folder_name);

    if game_dir.exists() {
        return Err(format!("Folder '{}' already exists", folder_name));
    }

    fs::create_dir_all(&game_dir).map_err(|e| format!("Failed to create folder: {}", e))?;

    let is_us = lineup == "us" || lineup.starts_with("us/");
    let default_country = if is_us { "us" } else { "jp" };
    let default_titlebar: u32 = if is_us { 9 } else { 4 };

    let identity = m2_publish::identity::new_game_identity();
    let game = GameJson {
        game_id: Some(identity.game_id),
        native_region_tag: Some(identity.native_region_tag),
        display: GameDisplay {
            name: String::new(),
            tname: String::new(),
            name_eng: String::new(),
            titlebar: default_titlebar,
            ccolor: 0,
            csize: 0,
            players: 1,
            demo_time: 6000,
            ..Default::default()
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
        genre: None,
        extra: Default::default(),
    };

    save_game_json(&game_dir.join("game.json"), &game)?;

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
    fs::remove_dir_all(&game_dir).map_err(|e| format!("Failed to delete folder: {}", e))?;
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
    let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read directory: {}", e))?;

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
    m2_publish::identity::migrate_library(Path::new(&games_path))?;
    if !folder_path.is_dir() {
        return Err(format!("Folder '{}' does not exist", folder_name));
    }
    Ok(load_lineup(&folder_path))
}

#[tauri::command]
async fn save_folder_contents(
    games_path: String,
    lineup: String,
    folder_name: String,
    library: Library,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let folder_path = Path::new(&games_path).join(&lineup).join(&folder_name);
        save_lineup(&folder_path, &library)?;
        m2_publish::source_library::metadata_changed(Path::new(&games_path)).map_err(|e| {
            format!("Folder metadata saved, but console fonts could not be updated: {e}")
        })?;
        Ok(())
    })
    .await
    .map_err(|e| format!("Folder update stopped: {e}"))?
}

fn mark_cover_dirty(entry: &Path) -> Result<(), String> {
    if m2_publish::rom_store::library_root(entry)
        .is_some_and(|root| m2_publish::source_library::enabled(&root))
    {
        m2_publish::save_format::atomic_write(&entry.join("runtime/.chronos-cover-dirty"), b"2\n")?;
    }
    Ok(())
}

fn prepare_changed_cover(entry: &Path) -> Result<(), String> {
    if let Some(root) = m2_publish::rom_store::library_root(entry) {
        if m2_publish::source_library::enabled(&root) {
            m2_publish::runtime_assets::prepare_entry_assets(entry).map_err(|e| {
                format!("Cover image saved, but console artwork could not be updated: {e}")
            })?;
            let dirty = entry.join("runtime/.chronos-cover-dirty");
            match fs::remove_file(&dirty) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("Console artwork prepared, but its retry flag could not be cleared: {e}")),
            }
        }
    }
    Ok(())
}

#[tauri::command]
fn create_folder(games_path: String, lineup: String, folder_name: String) -> Result<(), String> {
    let _cover_update = import_commands::lock_cover_update()?;
    let folder_path = Path::new(&games_path).join(&lineup).join(&folder_name);
    if folder_path.exists() {
        return Err(format!("Folder '{}' already exists", folder_name));
    }

    fs::create_dir_all(&folder_path).map_err(|e| format!("Failed to create folder: {}", e))?;

    // Write empty gamelist.json
    fs::write(folder_path.join("gamelist.json"), "[]")
        .map_err(|e| format!("Failed to write gamelist.json: {}", e))?;

    // Copy default folder cover if available
    let default_cover = Path::new(&games_path).join("folder_default.png");
    if default_cover.exists() {
        let _ = fs::copy(&default_cover, folder_path.join("cover.png"));
    }
    prepare_changed_cover(&folder_path)?;

    Ok(())
}

#[tauri::command]
fn delete_folder(games_path: String, lineup: String, folder_name: String) -> Result<(), String> {
    let folder_path = Path::new(&games_path).join(&lineup).join(&folder_name);
    if !folder_path.exists() {
        return Err("Folder does not exist".to_string());
    }
    fs::remove_dir_all(&folder_path).map_err(|e| format!("Failed to delete folder: {}", e))?;
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
    m2_publish::identity::ensure_game_identity(&game_json_path)?;
    if !game_json_path.exists() {
        return Err(format!("game.json not found in {}/{}", lineup, folder_name));
    }
    let game_str = fs::read_to_string(&game_json_path)
        .map_err(|e| format!("Failed to read game.json: {}", e))?;
    let game: GameJson =
        serde_json::from_str(&game_str).map_err(|e| format!("Failed to parse game.json: {}", e))?;
    let cover_file = game
        .cover
        .clone()
        .unwrap_or_else(|| "cover.png".to_string());
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

// ---- Move game and both catalogues as one operation ----

#[tauri::command]
async fn move_game(
    games_path: String,
    src_lineup: String,
    folder_name: String,
    dest_lineup: String,
    source_sort: Vec<GameListEntry>,
    destination_sort: Vec<GameListEntry>,
) -> Result<library_move::MoveGameResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        library_move::perform_move(
            Path::new(&games_path),
            &src_lineup,
            &folder_name,
            &dest_lineup,
            &source_sort,
            &destination_sort,
        )
    })
    .await
    .map_err(|e| format!("Game move stopped: {e}"))?
}

// ---- Editor Settings (stored next to exe as settings_editor.json) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorSettings {
    #[serde(default = "default_true")]
    pub confirm_delete: bool,
    #[serde(default)]
    pub games_path: Option<String>,
    #[serde(default)]
    pub bios: Option<m2_import::BiosConfig>,
}

fn default_true() -> bool {
    true
}

impl Default for EditorSettings {
    fn default() -> Self {
        EditorSettings {
            confirm_delete: true,
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
        "Shooter".into(),
        "Action".into(),
        "Fighter".into(),
        "Sport".into(),
        "Puzzle".into(),
        "Racing".into(),
        "RPG".into(),
        "Simulation".into(),
        "Adventure".into(),
        "ETC.".into(),
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
        let settings: EditorSettings =
            serde_json::from_slice(&fs::read(portable).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
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
                confirm_delete: v
                    .get("confirm_delete")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
                games_path: v
                    .get("games_path")
                    .and_then(|v| v.as_str())
                    .map(String::from),
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
    fs::write(&temporary, json)
        .and_then(|()| fs::rename(&temporary, &path))
        .map_err(|e| format!("Failed to write editor settings: {}", e))
}

#[tauri::command]
fn load_games_settings(games_path: String) -> Result<GamesSettings, String> {
    let path = std::path::Path::new(&games_path).join("settings_games.json");
    if !path.exists() {
        return Ok(GamesSettings::default());
    }
    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read games settings: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("Failed to parse games settings: {}", e))
}

#[tauri::command]
async fn save_games_settings(games_path: String, settings: GamesSettings) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || write_games_settings(&games_path, &settings))
        .await
        .map_err(|e| format!("Settings update stopped: {e}"))?
}

fn write_games_settings(games_path: &str, settings: &GamesSettings) -> Result<(), String> {
    let path = std::path::Path::new(&games_path).join("settings_games.json");
    let json = serde_json::to_value(settings)
        .map_err(|e| format!("Failed to serialize games settings: {}", e))?;
    m2_publish::identity::atomic_write_json(&path, &json)?;
    m2_publish::source_library::metadata_changed(Path::new(&games_path))
        .map_err(|e| format!("Settings saved, but console fonts could not be updated: {e}"))
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
    pub lock_masks_imported: usize,
}

/// Reorganise the USB's `published/` engine-format files into the per-game
/// `library/` view. Called from the editor at "Open library" time, before
/// any user edits. Idempotent.
///
/// `library_root` can be either the wrapper folder (`<lib>/`) or the
/// exploded library subfolder (`<lib>/library/` or a legacy direct path).
/// `published_root` is optional — if empty, derived as sibling.
#[tauri::command]
async fn sync_library(library_root: String, published_root: String) -> Result<SyncResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (lib, pub_default, _) = resolve_library_paths(Path::new(&library_root));
        m2_publish::identity::migrate_library(&lib)?;
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
                lock_masks_imported: r.lock_masks_imported,
            }),
            Err(e) => Err(format!("sync failed: {}", e)),
        }
    })
    .await
    .map_err(|e| format!("Save migration stopped: {e}"))?
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
        if let Some(p) = cur.parent() {
            cur = p.to_path_buf();
        } // Contents
        if cur.ends_with("Contents") {
            if let Some(p) = cur.parent() {
                cur = p.to_path_buf();
            } // *.app
            if cur.extension().map_or(false, |e| e == "app") {
                if let Some(p) = cur.parent() {
                    cur = p.to_path_buf();
                }
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

fn required_templates(root: &Path) -> Result<Vec<&'static str>, String> {
    let console =
        m2_publish::console::ConsoleVariant::from_directory(root).map_err(|e| e.to_string())?;
    let mut paths = console.templates().to_vec();
    paths.extend_from_slice(m2_publish::fonts::RESOURCES);
    if root.join(m2_publish::console::SYSTEM_PROFILE).is_file() {
        paths.push(m2_publish::console::SYSTEM_PROFILE);
    }
    Ok(paths)
}

/// Examine a wrapper folder and report whether init is needed and possible.
/// Called from the frontend on app launch to decide between "load existing"
/// and "prompt to initialize empty library".
#[tauri::command]
fn check_library_status(usb_root: String) -> Result<LibraryStatus, String> {
    let root = Path::new(&usb_root);
    let valid =
        |p: &Path| p.join("jp/gamelist.json").is_file() && p.join("us/gamelist.json").is_file();
    let has_library = valid(&root.join("library")) || valid(root);
    let backup_game = root.join("BACKUP").join("game");
    let has_backup = backup_game.is_dir();
    let mut missing = Vec::new();
    if has_backup {
        match required_templates(&backup_game) {
            Ok(paths) => {
                for rel in paths {
                    if !backup_game.join(rel).is_file() {
                        missing.push(rel.to_string());
                    }
                }
            }
            Err(error) => missing.push(error),
        }
    }
    Ok(LibraryStatus {
        has_library,
        has_backup,
        missing_templates: missing,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitResult {
    pub library_root: String,
    pub templates_copied: usize,
    pub source_schema: u32,
}

/// Complete the BACKUP/game first-run layout as a source library. No folder
/// catalogues are exported, including when both lineups are empty.
#[tauri::command]
async fn init_library(
    usb_root: String,
    on_progress: tauri::ipc::Channel<m2_import::Progress>,
) -> Result<InitResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = Path::new(&usb_root);
        let backup_game = root.join("BACKUP/game");
        if !backup_game.is_dir() {
            return Err(format!("BACKUP/game/ not found under {usb_root}"));
        }
        let templates = required_templates(&backup_game)?;
        for relative in templates.iter().copied().chain([
            "m2engage",
            "libopus.so.0",
            "version",
            "shutdown.png",
            "system/script/init.nut.m",
            m2_publish::console::SYSTEM_PROFILE,
        ]) {
            if !backup_game.join(relative).is_file() {
                return Err(format!(
                    "Original console dump is missing BACKUP/game/{relative}"
                ));
            }
        }
        let library_root = root.join("library");
        for directory in [
            &library_root,
            &library_root.join("jp"),
            &library_root.join("us"),
        ] {
            match fs::symlink_metadata(directory) {
                Ok(meta) if !meta.file_type().is_dir() => {
                    return Err(format!(
                        "Expected a regular library directory: {}",
                        directory.display()
                    ))
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir_all(directory).map_err(|e| e.to_string())?
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        for lineup in ["jp", "us"] {
            let gamelist = library_root.join(lineup).join("gamelist.json");
            if !gamelist.exists() {
                m2_publish::identity::atomic_write_json(&gamelist, &serde_json::json!([]))?;
            }
        }
        let templates_copied = templates
            .iter()
            .filter(|relative| !library_root.join("templates").join(relative).exists())
            .count();
        m2_import::prepare_existing_library_runtime(&backup_game, &library_root, &|progress| {
            let _ = on_progress.send(progress);
        })
        .map_err(|e| {
            format!("Console preparation failed; original BACKUP/game is retained: {e:#}")
        })?;
        Ok(InitResult {
            library_root: library_root.to_string_lossy().into_owned(),
            templates_copied,
            source_schema: 2,
        })
    })
    .await
    .map_err(|e| format!("Library initialization stopped: {e}"))?
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
        Err(e) => Err(e.to_string()),
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(partition_commands::PartitionState::default())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window
                    .state::<partition_commands::PartitionState>()
                    .is_busy()
                {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            partition_commands::console_partitions,
            partition_commands::console_status,
            partition_commands::console_backup,
            partition_commands::console_inspect_image,
            partition_commands::console_prepare_restore,
            partition_commands::console_cancel_restore,
            partition_commands::console_restore,
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
            import_commands::remove_replaced_rom,
            import_commands::configure_bios,
            import_commands::get_bios_status,
            import_commands::create_library_from_dump,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if app.state::<partition_commands::PartitionState>().is_busy() {
                    api.prevent_exit();
                }
            }
        });
}

#[cfg(test)]
mod genre_persistence_tests {
    use super::*;

    #[test]
    fn genres_and_sort_order_survive_reopening_without_publication() {
        let root = tempfile::tempdir().unwrap();
        let root_str = root.path().to_str().unwrap().to_owned();
        write_games_settings(
            &root_str,
            &GamesSettings {
                genres: vec!["Action".into(), "RPG".into()],
                alpha_exclusions: vec![],
            },
        )
        .unwrap();
        // Exercise both root lineups and the same save/load path used by folders.
        for lineup in ["jp", "us", "jp/HuCard"] {
            let mut first = create_game(root_str.clone(), lineup.into(), "GAME001".into()).unwrap();
            let mut second =
                create_game(root_str.clone(), lineup.into(), "GAME002".into()).unwrap();
            // Deserialize the frontend payload, as the save command does.
            let mut payload = serde_json::to_value(&first.game).unwrap();
            payload["genre"] = serde_json::json!(2);
            first.game = serde_json::from_value(payload).unwrap();
            second.game.genre = Some(1);
            first.sort.sor_genr = 1;
            second.sort.sor_genr = 0;
            let path = root.path().join(lineup);
            save_lineup(
                &path,
                &Library {
                    games: vec![first, second],
                    path: path.to_string_lossy().into_owned(),
                },
            )
            .unwrap();
            let mut reopened = load_lineup(&path);
            assert_eq!(reopened.games.len(), 2);
            assert_eq!(reopened.games[0].game.genre, Some(2));
            assert_eq!(reopened.games[1].game.genre, Some(1));
            assert_eq!(reopened.games[0].sort.sor_genr, 1);
            assert_eq!(reopened.games[1].sort.sor_genr, 0);
            // Clearing a genre must also survive another reopen.
            reopened.games[0].game.genre = None;
            save_lineup(&path, &reopened).unwrap();
            assert_eq!(load_lineup(&path).games[0].game.genre, None);
        }
        assert_eq!(
            load_games_settings(root_str).unwrap().genres,
            ["Action", "RPG"]
        );
        assert!(!root.path().join("published").exists());
    }

    #[test]
    fn legacy_games_without_genre_remain_readable() {
        let game: GameJson = serde_json::from_value(serde_json::json!({
            "display": {"name": "Legacy game"}, "rom": {}
        }))
        .unwrap();
        assert_eq!(game.genre, None);
        assert!(serde_json::to_value(game).unwrap().get("genre").is_none());
    }
}

#[cfg(test)]
mod stable_game_save_tests {
    use super::*;

    #[test]
    fn game_identity_unknown_profile_fields_and_four_sparse_slots_survive_move_and_reorder() {
        let root = tempfile::tempdir().unwrap();
        let root_str = root.path().to_string_lossy().into_owned();
        let entry = create_game(root_str.clone(), "jp".into(), "GAME001".into()).unwrap();
        let source = root.path().join("jp/GAME001");
        let mut json: serde_json::Value =
            serde_json::from_slice(&fs::read(source.join("game.json")).unwrap()).unwrap();
        json["display"]["nativeExtra"] = serde_json::json!({"keep":"display"});
        json["rom"]["regionTag"] = serde_json::json!("ORIGINAL");
        json["rom"]["native_option"] = serde_json::json!(17);
        json["emulator"]["native_flags"] = serde_json::json!([3, 4]);
        json["rom_alt"] = serde_json::json!({"regionTagE":"ALT"});
        json["future"] = serde_json::json!({"keep":true});
        m2_publish::identity::atomic_write_json(&source.join("game.json"), &json).unwrap();
        fs::create_dir(source.join("saves")).unwrap();
        fs::write(source.join("sram.bin"), b"persistent-memory").unwrap();
        for slot in [1, 3] {
            fs::write(
                source.join(format!("saves/state_{slot}.bin")),
                format!("state-{slot}"),
            )
            .unwrap();
            fs::write(
                source.join(format!("saves/state_{slot}_meta.bin")),
                format!("meta-{slot}"),
            )
            .unwrap();
        }
        let loaded = load_game_entry(root_str.clone(), "jp".into(), "GAME001".into()).unwrap();
        let serialized = serde_json::to_value(&loaded.game).unwrap();
        assert_eq!(serialized["future"], json["future"]);
        assert_eq!(serialized["rom_alt"], json["rom_alt"]);
        assert_eq!(
            serialized["display"]["nativeExtra"],
            json["display"]["nativeExtra"]
        );
        assert_eq!(
            serialized["rom"]["native_option"],
            json["rom"]["native_option"]
        );
        assert_eq!(
            serialized["emulator"]["native_flags"],
            json["emulator"]["native_flags"]
        );
        // The destination already uses the directory name; the suffix must
        // not change either identity or any save payload.
        create_game(root_str.clone(), "us".into(), "GAME001".into()).unwrap();
        let source_entry = load_game_entry(root_str.clone(), "jp".into(), "GAME001".into()).unwrap();
        save_lineup(&root.path().join("jp"), &Library { games: vec![source_entry.clone()], path: String::new() }).unwrap();
        let destination_entry = load_game_entry(root_str.clone(), "us".into(), "GAME001".into()).unwrap();
        save_lineup(&root.path().join("us"), &Library { games: vec![destination_entry.clone()], path: String::new() }).unwrap();
        let result = library_move::perform_move(root.path(), "jp", "GAME001", "us", &[],
            &[destination_entry.sort, source_entry.sort]).unwrap();
        let name = result.folder_name;
        assert_eq!(name, "GAME001-2");
        let mut moved = load_game_entry(root_str, "us".into(), name.clone()).unwrap();
        assert_eq!(moved.game.game_id, entry.game.game_id);
        assert_eq!(moved.game.native_region_tag, entry.game.native_region_tag);
        assert_eq!(moved.game.rom.region_tag.as_deref(), Some("ORIGINAL"));
        assert_eq!(moved.game.display.titlebar, entry.game.display.titlebar);
        moved.sort.sor_date = 8;
        moved.sort.sor_genr = 3;
        moved.game.genre = Some(2);
        let destination = root.path().join("us");
        save_lineup(
            &destination,
            &Library {
                games: vec![moved],
                path: destination.to_string_lossy().into_owned(),
            },
        )
        .unwrap();
        let reopened = load_lineup(&destination);
        assert_eq!(reopened.games[0].game.game_id, entry.game.game_id);
        assert_eq!(
            reopened.games[0].game.native_region_tag,
            entry.game.native_region_tag
        );
        let saved = root.path().join("us").join(name);
        assert_eq!(
            fs::read(saved.join("sram.bin")).unwrap(),
            b"persistent-memory"
        );
        let saves = save_states::read_game_saves(&saved).unwrap();
        assert_eq!(
            saves
                .states
                .iter()
                .map(|slot| slot.slot)
                .collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_eq!(
            saves
                .states
                .iter()
                .map(|slot| slot.exists)
                .collect::<Vec<_>>(),
            [false, true, false, true]
        );
        for slot in [1, 3] {
            assert_eq!(
                fs::read(saved.join(format!("saves/state_{slot}.bin"))).unwrap(),
                format!("state-{slot}").as_bytes()
            );
            assert_eq!(
                fs::read(saved.join(format!("saves/state_{slot}_meta.bin"))).unwrap(),
                format!("meta-{slot}").as_bytes()
            );
        }
    }

    #[test]
    fn stale_or_mutated_identity_cannot_overwrite_a_saved_game() {
        let root = tempfile::tempdir().unwrap();
        let root_str = root.path().to_string_lossy().into_owned();
        let mut entry = create_game(root_str, "jp".into(), "GAME001".into()).unwrap();
        let path = root.path().join("jp/GAME001/game.json");
        let before = fs::read(&path).unwrap();
        entry.game.game_id = Some("0123456789abcdef0123456789abcdef".into());
        assert!(save_game_json(&path, &entry.game)
            .unwrap_err()
            .contains("immutable"));
        assert_eq!(fs::read(&path).unwrap(), before);
        // Older editor payloads omit identities; retain the disk values.
        entry.game.game_id = None;
        entry.game.native_region_tag = None;
        entry.game.display.name = "Edited title".into();
        save_game_json(&path, &entry.game).unwrap();
        let after: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let original: serde_json::Value = serde_json::from_slice(&before).unwrap();
        assert_eq!(after["game_id"], original["game_id"]);
        assert_eq!(after["native_region_tag"], original["native_region_tag"]);
        assert_eq!(after["display"]["name"], "Edited title");
    }

    #[test]
    fn expert_settings_survive_a_real_save_and_reopen_without_changing_identity() {
        let root = tempfile::tempdir().unwrap();
        let mut entry = create_game(root.path().to_string_lossy().into_owned(), "jp".into(), "GAME700".into()).unwrap();
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../../tests/fixtures/expert-game.json")).unwrap();
        let id = entry.game.game_id.clone();
        let tag = entry.game.native_region_tag.clone();
        entry.game = serde_json::from_value(fixture).unwrap();
        entry.game.game_id = id.clone();
        entry.game.native_region_tag = tag.clone();
        let path = root.path().join("jp/GAME700/game.json");
        save_game_json(&path, &entry.game).unwrap();
        let saved: GameJson = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.game_id, id);
        assert_eq!(saved.native_region_tag, tag);
        assert_eq!(saved.rom.extra["overdrive"], 2);
        assert_eq!(saved.rom.extra["use_analog"], false);
        assert_eq!(saved.rom.extra["future_option"]["values"][2], 1.25);
        assert_eq!(saved.emulator.screen_mode, 4);
        let before = fs::read(&path).unwrap();
        entry.game.rom.tg16_option = Some(16);
        assert!(save_game_json(&path, &entry.game).unwrap_err().contains("tg16_option"));
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
