#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{AppHandle, Manager, State};
mod netprobe;
mod network;
mod partitions;
mod recovery;

#[derive(Default)]
struct AppState {
    busy: Arc<AtomicBool>,
}
struct Operation(Arc<AtomicBool>);
impl Drop for Operation {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
fn acquire(state: &AppState) -> Result<Operation, String> {
    state
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "Another recovery or partition operation is already running.".to_string())?;
    Ok(Operation(state.busy.clone()))
}

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            netprobe::spawn_persistent(app.handle().clone(), Duration::from_secs(2));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_recovery,
            default_payloads_dir,
            network_interfaces,
            network_connect,
            partitions_list,
            pick_save_file,
            pick_open_file,
            pick_directory,
            partition_dump,
            partition_restore,
            partition_inspect_image,
            partition_dump_all
        ])
        .run(tauri::generate_context!())
        .expect("Failed to launch PCE Mini Recovery");
}

#[tauri::command]
fn partitions_list() -> Vec<partitions::PartitionInfo> {
    partitions::list()
}
#[tauri::command]
async fn pick_save_file(default_name: String) -> Option<String> {
    partitions::pick_save_file(default_name).await
}
#[tauri::command]
async fn pick_open_file() -> Option<String> {
    partitions::pick_open_file().await
}
#[tauri::command]
async fn pick_directory() -> Option<String> {
    partitions::pick_directory().await
}
#[tauri::command]
fn partition_dump(
    app: AppHandle,
    state: State<'_, AppState>,
    id: u8,
    out_path: String,
) -> Result<(), String> {
    partitions::dump(app, id, out_path, acquire(&state)?);
    Ok(())
}
#[tauri::command]
fn partition_restore(
    app: AppHandle,
    state: State<'_, AppState>,
    id: u8,
    in_path: String,
    selection: pce_recovery::image::ImageEntry,
) -> Result<(), String> {
    partitions::restore(app, id, in_path, selection, acquire(&state)?);
    Ok(())
}
#[tauri::command]
async fn partition_inspect_image(
    id: u8,
    in_path: String,
) -> Result<Vec<pce_recovery::image::ImageEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || partitions::inspect(id, in_path))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
fn partition_dump_all(
    app: AppHandle,
    state: State<'_, AppState>,
    dir: String,
) -> Result<(), String> {
    partitions::dump_all(app, dir, acquire(&state)?);
    Ok(())
}
#[tauri::command]
fn start_recovery(
    app: AppHandle,
    state: State<'_, AppState>,
    payloads_dir: String,
    wait_secs: u64,
) -> Result<(), String> {
    let operation = acquire(&state)?;
    std::thread::spawn(move || {
        let _operation = operation;
        recovery::run(
            app,
            PathBuf::from(payloads_dir),
            Duration::from_secs(wait_secs.clamp(5, 600)),
        );
    });
    Ok(())
}
#[tauri::command]
fn default_payloads_dir(app: AppHandle) -> String {
    let candidates = [
        app.path().resource_dir().ok().map(|p| p.join("payloads")),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("payloads"))),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../payloads")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|p| p.join("fes1.bin").is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}
#[tauri::command]
fn network_interfaces() -> Result<Vec<linux_network::Interface>, String> {
    linux_network::discover().map_err(|e| e.to_string())
}
#[tauri::command]
async fn network_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    interface: String,
) -> Result<network::ConnectionReport, String> {
    let operation = acquire(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        network::connect(&app, &interface)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partition_and_boot_operations_cannot_overlap() {
        let state = AppState::default();
        let operation = acquire(&state).unwrap();
        assert!(acquire(&state).is_err());
        drop(operation);
        assert!(acquire(&state).is_ok());
    }
}
