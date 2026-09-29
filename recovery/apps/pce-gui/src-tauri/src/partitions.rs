//! Tauri commands for the Partitions panel.
//!
//! Each dump/restore runs on a worker thread and streams progress events to
//! the frontend. Errors are emitted via `partition-done { ok: false, msg }`
//! rather than returned from the command, so the UI can update its
//! per-row state asynchronously.

use std::path::PathBuf;
use std::time::Instant;

use pce_recovery::{
    dump_partition,
    image::{inspect_image, load_image, ImageEntry},
    partitions, restore_partition,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize)]
pub struct PartitionInfo {
    pub id: u8,
    pub device_path: &'static str,
    pub role: &'static str,
    pub size_bytes: u64,
}

pub fn list() -> Vec<PartitionInfo> {
    let mut out: Vec<PartitionInfo> = partitions::PARTITIONS
        .iter()
        .map(|p| PartitionInfo {
            id: p.id,
            device_path: p.device_path,
            role: p.role,
            size_bytes: p.size_bytes(),
        })
        .collect();
    let f = &partitions::FULL;
    out.push(PartitionInfo {
        id: f.id,
        device_path: f.device_path,
        role: f.role,
        size_bytes: f.size_bytes(),
    });
    out
}

/// Pop a native "save as" dialog. Returns `None` if the user cancels.
pub async fn pick_save_file(default_name: String) -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_file_name(&default_name)
        .save_file()
        .await
        .map(|f| f.path().to_string_lossy().into_owned())
}

/// Pop a native "open" dialog. Returns `None` if the user cancels.
pub async fn pick_open_file() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("Restore partition — raw image or ZIP archive")
        .add_filter(
            "Partition images and ZIP archives",
            &["bin", "img", "mod", "zip"],
        )
        .add_filter("All files", &["*"])
        .pick_file()
        .await
        .map(|f| f.path().to_string_lossy().into_owned())
}

/// Pop a native "choose folder" dialog. Returns `None` if the user cancels.
pub async fn pick_directory() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .pick_folder()
        .await
        .map(|f| f.path().to_string_lossy().into_owned())
}

pub fn dump(app: AppHandle, id: u8, out_path: String, operation: crate::Operation) {
    std::thread::spawn(move || {
        let _operation = operation;
        let res = dump_inner(&app, id, PathBuf::from(out_path));
        emit_done(&app, id, "dump", res);
    });
}

pub fn inspect(id: u8, in_path: String) -> Result<Vec<ImageEntry>, String> {
    let p = partitions::lookup(id).ok_or_else(|| format!("unknown partition id {id}"))?;
    inspect_image(&PathBuf::from(in_path), p.size_bytes(), false).map_err(|e| e.to_string())
}

pub fn restore(
    app: AppHandle,
    id: u8,
    in_path: String,
    selection: ImageEntry,
    operation: crate::Operation,
) {
    std::thread::spawn(move || {
        let _operation = operation;
        let res = restore_inner(&app, id, PathBuf::from(in_path), selection);
        emit_done(&app, id, "restore", res);
    });
}

/// Dump every numbered partition (p1..p10, skipping p3/p4 which don't exist)
/// sequentially into `dir`. Files are named `mmcblk0pN.bin` to match
/// `nand_dump split`. Continues past per-partition errors so a single bad
/// transfer doesn't kill the whole batch.
pub fn dump_all(app: AppHandle, dir: String, operation: crate::Operation) {
    std::thread::spawn(move || {
        let _operation = operation;
        let dir_path = PathBuf::from(&dir);
        if let Err(e) = std::fs::create_dir_all(&dir_path) {
            let _ = app.emit(
                "log",
                serde_json::json!({ "level": "error", "msg": format!("create_dir_all {}: {e}", dir_path.display()) }),
            );
            let _ = app.emit(
                "partition-batch",
                serde_json::json!({ "state": "error", "msg": e.to_string() }),
            );
            return;
        }

        let total_count = partitions::PARTITIONS.len();
        let _ = app.emit(
            "partition-batch",
            serde_json::json!({ "state": "start", "total": total_count, "dir": dir }),
        );

        let mut failed = 0;
        for (i, p) in partitions::PARTITIONS.iter().enumerate() {
            let out = dir_path.join(format!("mmcblk0p{}.bin", p.id));
            let _ = app.emit(
                "partition-batch",
                serde_json::json!({ "state": "next", "index": i + 1, "total": total_count, "id": p.id }),
            );
            let res = dump_inner(&app, p.id, out);
            if res.is_err() {
                failed += 1;
            }
            emit_done(&app, p.id, "dump", res);
        }

        let _ = app.emit(
            "partition-batch",
            serde_json::json!({ "state": "done", "total": total_count, "failed": failed }),
        );
    });
}

// ---------------------------------------------------------------------------

fn dump_inner(app: &AppHandle, id: u8, out: PathBuf) -> Result<u64, String> {
    let p = partitions::lookup(id).ok_or_else(|| format!("unknown partition id {id}"))?;
    let total = p.size_bytes();

    if out.exists() {
        return Err(format!(
            "Backup already exists: {}. Choose a new filename.",
            out.display()
        ));
    }
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;

    emit_start(app, id, "dump", total);

    let started = Instant::now();
    dump_partition(p.device_path, total, &mut file, |transferred| {
        emit_progress(
            app,
            id,
            "dump",
            transferred,
            total,
            started.elapsed().as_millis() as u64,
        );
    })
    .map_err(|e| format!("nc dump {}: {e}", p.device_path))?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist_noclobber(&out).map_err(|e| e.to_string())?;
    Ok(total)
}

fn restore_inner(
    app: &AppHandle,
    id: u8,
    input: PathBuf,
    selection: ImageEntry,
) -> Result<u64, String> {
    let p = partitions::lookup(id).ok_or_else(|| format!("unknown partition id {id}"))?;
    let target = p.size_bytes();
    emit_start(app, id, "checking image", target);
    let preparing = Instant::now();
    let mut last_update = Instant::now();
    let data = load_image(&input, target, false, Some(&selection), |current| {
        if current == target || last_update.elapsed().as_millis() >= 100 {
            emit_progress(
                app,
                id,
                "checking image",
                current,
                target,
                preparing.elapsed().as_millis() as u64,
            );
            last_update = Instant::now();
        }
    })
    .map_err(|e| format!("{}: {e}", input.display()))?;

    emit_start(app, id, "restore", data.len() as u64);

    let started = Instant::now();
    let total = data.len() as u64;
    restore_partition(p.device_path, &data, |sent| {
        emit_progress(
            app,
            id,
            "restore",
            sent,
            total,
            started.elapsed().as_millis() as u64,
        );
    })
    .map(|_hash| total)
    .map_err(|e| format!("nc restore {}: {e}", p.device_path))
}

// ---------------------------------------------------------------------------

fn emit_start(app: &AppHandle, id: u8, kind: &str, total: u64) {
    let _ = app.emit(
        "partition-progress",
        serde_json::json!({ "id": id, "kind": kind, "current": 0, "total": total, "elapsed_ms": 0 }),
    );
}

fn emit_progress(app: &AppHandle, id: u8, kind: &str, current: u64, total: u64, elapsed_ms: u64) {
    let _ = app.emit(
        "partition-progress",
        serde_json::json!({ "id": id, "kind": kind, "current": current, "total": total, "elapsed_ms": elapsed_ms }),
    );
}

fn emit_done(app: &AppHandle, id: u8, kind: &str, res: Result<u64, String>) {
    let payload = match res {
        Ok(bytes) => serde_json::json!({ "id": id, "kind": kind, "ok": true, "bytes": bytes }),
        Err(msg) => serde_json::json!({ "id": id, "kind": kind, "ok": false, "msg": msg }),
    };
    let _ = app.emit("partition-done", payload);
}
