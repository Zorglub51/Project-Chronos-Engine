use m2_import::{BiosConfig, Progress};
use std::path::{Path, PathBuf};
use tauri::{ipc::Channel, AppHandle, Manager};

fn bios_cache(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("bios"))
}

#[tauri::command]
pub async fn import_rom(
    src_path: String,
    dest_dir: String,
    bios: Option<BiosConfig>,
    on_progress: Channel<Progress>,
) -> Result<m2_import::ImportResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        m2_import::import_rom(
            Path::new(&src_path),
            Path::new(&dest_dir),
            bios.as_ref(),
            &|p| {
                let _ = on_progress.send(p);
            },
        )
        .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("ROM import stopped: {e}"))?
}

#[tauri::command]
pub async fn configure_bios(
    app: AppHandle,
    pcd_path: Option<String>,
    super_path: Option<String>,
    system_path: Option<String>,
) -> Result<BiosConfig, String> {
    let cache = bios_cache(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        m2_import::configure_bios(
            pcd_path.as_deref().map(Path::new),
            super_path.as_deref().map(Path::new),
            system_path.as_deref().map(Path::new),
            &cache,
        )
        .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("BIOS extraction stopped: {e}"))?
}

#[tauri::command]
pub fn get_bios_status(bios: Option<BiosConfig>) -> m2_import::BiosStatus {
    m2_import::bios_status(bios.as_ref())
}

#[tauri::command]
pub async fn create_library_from_dump(
    app: AppHandle,
    source_path: String,
    destination: String,
    include_games: bool,
    on_progress: Channel<Progress>,
) -> Result<m2_import::NewLibraryResult, String> {
    let cache = bios_cache(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        m2_import::create_library(
            Path::new(&source_path),
            Path::new(&destination),
            include_games,
            &cache,
            &|p| {
                let _ = on_progress.send(p);
            },
        )
        .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("Library creation stopped: {e}"))?
}
