use m2_import::{BiosConfig, Progress};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use tauri::{ipc::Channel, AppHandle, Manager};

/// Keep the canonical PNG and its derived console descriptor in one editor
/// operation. Tauri's blocking workers otherwise allow two imports to overlap.
pub(crate) fn lock_cover_update() -> Result<MutexGuard<'static, ()>, String> {
    static COVER_UPDATE: Mutex<()> = Mutex::new(());
    COVER_UPDATE.lock().map_err(|_| {
        "The previous cover update stopped unexpectedly; reopen the editor".to_string()
    })
}

pub(crate) fn save_cover_png(
    image: &image::DynamicImage,
    destination: &Path,
) -> Result<(), String> {
    let mut encoded = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode cover: {e}"))?;
    m2_publish::save_format::atomic_write(destination, encoded.get_ref())
        .map_err(|e| format!("Failed to save cover: {e}"))
}

fn bios_cache(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("bios"))
}

#[tauri::command]
pub fn remove_replaced_rom(
    dest_dir: String,
    previous: String,
    replacement: String,
) -> Result<(), String> {
    m2_import::remove_replaced_rom(Path::new(&dest_dir), &previous, &replacement)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn import_rom(
    src_path: String,
    dest_dir: String,
    bios: Option<BiosConfig>,
    on_progress: Channel<Progress>,
) -> Result<m2_import::ImportResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let metadata = Path::new(&dest_dir).join("game.json");
        if metadata.is_file() {
            // A ROM replacement changes content, never the game's save identity.
            m2_publish::identity::ensure_game_identity(&metadata)?;
        }
        m2_import::import_library_rom(
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
        let result = m2_import::create_library(
            Path::new(&source_path),
            Path::new(&destination),
            include_games,
            &cache,
            &|p| {
                let _ = on_progress.send(p);
            },
        )
        .map_err(|e| format!("{e:#}"))?;
        let root = Path::new(&result.root);
        let library = if root.join("library").is_dir() {
            root.join("library")
        } else {
            root.to_path_buf()
        };
        m2_publish::identity::migrate_library(&library)?;
        Ok(result)
    })
    .await
    .map_err(|e| format!("Library creation stopped: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::mpsc, thread, time::Duration};

    #[test]
    fn overlapping_cover_updates_keep_source_and_descriptor_in_the_same_order() {
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().to_path_buf();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (attempt_tx, attempt_rx) = mpsc::channel();
        let first_entry = entry.clone();
        let first = thread::spawn(move || {
            let _update = lock_cover_update().unwrap();
            let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                8,
                8,
                image::Rgba([255, 0, 0, 255]),
            ));
            save_cover_png(&image, &first_entry.join("cover.png")).unwrap();
            ready_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            m2_publish::runtime_assets::prepare_entry_assets(&first_entry).unwrap()
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let second_entry = entry.clone();
        let second = thread::spawn(move || {
            attempt_tx.send(()).unwrap();
            let _update = lock_cover_update().unwrap();
            let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                8,
                8,
                image::Rgba([0, 0, 255, 255]),
            ));
            save_cover_png(&image, &second_entry.join("cover.png")).unwrap();
            m2_publish::runtime_assets::prepare_entry_assets(&second_entry).unwrap()
        });
        attempt_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            image::open(entry.join("cover.png"))
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)
                .0,
            [255, 0, 0, 255],
        );
        release_tx.send(()).unwrap();
        let first = first.join().unwrap();
        let second = second.join().unwrap();
        assert_ne!(first.revision, second.revision);
        let current: m2_publish::runtime_assets::PreparedCover =
            serde_json::from_slice(&fs::read(entry.join("runtime/assets.json")).unwrap()).unwrap();
        assert_eq!(current, second);
        assert_eq!(
            image::open(entry.join("cover.png"))
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)
                .0,
            [0, 0, 255, 255],
        );
    }

    #[test]
    fn geometry_edits_refresh_only_the_entry_and_retry_failed_preparation() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("library");
        let entry = library.join("jp/GAME001");
        fs::create_dir_all(&entry).unwrap();
        fs::write(library.join("jp/gamelist.json"), b"[]").unwrap();
        fs::write(library.join(".chronos-library-v2"), b"2\n").unwrap();
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            240,
            240,
            image::Rgba([230, 17, 39, 255]),
        ));
        save_cover_png(&image, &entry.join("cover.png")).unwrap();
        let source = fs::read(entry.join("cover.png")).unwrap();
        let mut game: crate::GameJson = serde_json::from_value(serde_json::json!({
            "display":{"name":"Original title"},
            "rom":{"arch":"tg16","rom":"game.pce.m"},
            "cover":"cover.png",
        }))
        .unwrap();
        let metadata = entry.join("game.json");
        crate::save_game_json(&metadata, &game).unwrap();
        let descriptor = entry.join("runtime/assets.json");
        let initial = fs::read(&descriptor).unwrap();
        let initial_time = fs::metadata(&descriptor).unwrap().modified().unwrap();

        // Ordinary metadata edits must not even decode the unchanged source.
        fs::write(entry.join("cover.png"), b"not a PNG").unwrap();
        game.display.name = "Renamed title".into();
        game.display.titlebar = 2;
        game.genre = Some(4);
        crate::save_game_json(&metadata, &game).unwrap();
        assert_eq!(fs::read(&descriptor).unwrap(), initial);
        assert_eq!(
            fs::metadata(&descriptor).unwrap().modified().unwrap(),
            initial_time
        );

        game.display.ccolor = 3;
        game.cover_size = Some(crate::CoverSize {
            width: 280,
            height: 240,
            origin_x: 140,
            origin_y: 120,
            extra: Default::default(),
        });
        assert!(crate::save_game_json(&metadata, &game)
            .unwrap_err()
            .contains("console artwork"));
        assert_eq!(fs::read(&descriptor).unwrap(), initial);
        assert!(entry.join("runtime/.chronos-cover-dirty").is_file());
        game.display.name = "Retry title".into();
        assert!(crate::save_game_json(&metadata, &game).is_err());

        fs::write(entry.join("cover.png"), &source).unwrap();
        crate::save_game_json(&metadata, &game).unwrap();
        let current: m2_publish::runtime_assets::PreparedCover =
            serde_json::from_slice(&fs::read(&descriptor).unwrap()).unwrap();
        assert_eq!(current.width, 280);
        assert_eq!(current.front_origin_x, 136.0);
        assert_eq!(current.left.unwrap().origin_x, 156.0);
        assert!(!entry.join("runtime/.chronos-cover-dirty").exists());
        assert_eq!(fs::read(entry.join("cover.png")).unwrap(), source);
    }
}
