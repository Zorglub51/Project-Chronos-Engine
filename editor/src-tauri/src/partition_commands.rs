//! Console partition operations share the recovery utility's transport and
//! validation. No console traffic occurs until an explicit frontend action.

use pce_recovery::{
    console::Environment,
    image::{copy_image, inspect_image, ImageEntry},
    partitions,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, Write},
    net::{SocketAddr, TcpStream},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{ipc::Channel, State};

const ADDRESS: &str = "169.254.13.37";

#[derive(Default, Clone)]
pub struct PartitionState(Arc<Inner>);
#[derive(Default)]
struct Inner {
    busy: AtomicBool,
    next_preview: AtomicU64,
    prepared: Mutex<Option<PreparedRestore>>,
}
pub struct Operation(PartitionState);
impl Drop for Operation {
    fn drop(&mut self) {
        self.0 .0.busy.store(false, Ordering::Release);
    }
}
impl PartitionState {
    pub fn is_busy(&self) -> bool {
        self.0.busy.load(Ordering::Acquire)
    }
    fn acquire(&self) -> Result<Operation, String> {
        self.0
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "Another console partition operation is already running.".to_owned())?;
        Ok(Operation(self.clone()))
    }
    fn take_preview(&self, token: &str) -> Result<PreparedRestore, String> {
        let mut prepared = self
            .0
            .prepared
            .lock()
            .map_err(|_| "Restore preview state is unavailable.")?;
        if prepared.as_ref().is_none_or(|p| p.preview.token != token) {
            return Err(
                "This restore preview has expired. Select and check the image again.".into(),
            );
        }
        Ok(prepared.take().unwrap())
    }
}

#[derive(Debug, Serialize)]
pub struct PartitionInfo {
    pub id: u8,
    pub device_path: &'static str,
    pub role: &'static str,
    pub size_bytes: u64,
}
#[derive(Debug, Serialize)]
pub struct ConsoleStatus {
    pub address: &'static str,
    pub reachable: bool,
    pub environment: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct PartitionProgress {
    pub id: u8,
    pub kind: String,
    pub current: u64,
    pub total: u64,
    pub elapsed_ms: u64,
    pub message: String,
    pub index: usize,
    pub count: usize,
}
#[derive(Debug, Clone, Serialize)]
pub struct BackupFile {
    pub id: u8,
    pub path: String,
    pub bytes: u64,
    pub sha1: String,
}
#[derive(Debug, Serialize)]
pub struct BackupReport {
    pub directory: String,
    pub files: Vec<BackupFile>,
    pub manifest_path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePreview {
    pub token: String,
    pub id: u8,
    pub device_path: String,
    pub role: String,
    pub size_bytes: u64,
    pub image_name: String,
    pub sha256: String,
}
struct PreparedRestore {
    preview: RestorePreview,
    file: File,
}
#[derive(Debug, Serialize)]
pub struct TransferReport {
    pub id: u8,
    pub bytes: u64,
    pub sha256: String,
    pub sha1: String,
}

#[tauri::command]
pub fn console_partitions() -> Vec<PartitionInfo> {
    partitions::PARTITIONS
        .iter()
        .chain(std::iter::once(&partitions::FULL))
        .map(|p| PartitionInfo {
            id: p.id,
            device_path: p.device_path,
            role: if p.id == 0 { "Complete MMC" } else { p.role },
            size_bytes: p.size_bytes(),
        })
        .collect()
}

#[tauri::command]
pub async fn console_status() -> Result<ConsoleStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let address: SocketAddr = format!("{ADDRESS}:22").parse().unwrap();
        if TcpStream::connect_timeout(&address, Duration::from_millis(600)).is_err() {
            return ConsoleStatus { address: ADDRESS, reachable: false, environment: "disconnected".into(), message: "The console is not reachable. Start it in RAM recovery with the recovery utility, configure the computer's USB network, then check the connection again.".into() };
        }
        let (environment, message) = match pce_recovery::console::inspect(ADDRESS) {
            Ok(Environment::RamRecovery) => ("ram_recovery", "RAM recovery verified. Backups and restores check the actual MMC layout and refuse mounted targets.".to_owned()),
            Ok(Environment::NotRamRecovery) => ("not_ram_recovery", "The console is running its normal system. Start RAM recovery with the recovery utility before backing up or restoring partitions.".to_owned()),
            Ok(Environment::Unknown) => ("unknown", "The console answered, but RAM recovery could not be verified. Partition transfers are unavailable.".to_owned()),
            Err(e) => ("unknown", format!("The console answered, but inspection failed: {e}")),
        };
        ConsoleStatus { address: ADDRESS, reachable: true, environment: environment.into(), message }
    }).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn console_inspect_image(id: u8, in_path: String) -> Result<Vec<ImageEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let partition = target(id)?;
        inspect_image(Path::new(&in_path), partition.size_bytes(), false).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn console_prepare_restore(
    state: State<'_, PartitionState>,
    id: u8,
    in_path: String,
    selection: ImageEntry,
    on_progress: Channel<PartitionProgress>,
) -> Result<RestorePreview, String> {
    let operation = state.acquire()?;
    tauri::async_runtime::spawn_blocking(move || {
        let partition = target(id)?;
        // Discard the previous snapshot before allocating another, including
        // when the new image subsequently fails validation.
        operation
            .0
             .0
            .prepared
            .lock()
            .map_err(|_| "Restore preview state is unavailable.")?
            .take();
        let mut file =
            tempfile::tempfile().map_err(|e| format!("Create temporary restore snapshot: {e}"))?;
        let mut progress = Progress::new(on_progress, id, "prepare", partition.size_bytes(), 1, 1);
        progress.emit(0, "Checking and staging image…", true);
        let mut hasher = Sha256::new();
        {
            let mut output = HashingWriter {
                writer: &mut file,
                hasher: &mut hasher,
            };
            copy_image(
                Path::new(&in_path),
                partition.size_bytes(),
                false,
                Some(&selection),
                &mut output,
                |current| {
                    progress.emit(
                        current,
                        "Checking and staging image…",
                        current == partition.size_bytes(),
                    )
                },
            )
            .map_err(|e| format!("Image validation failed; no console data was written: {e}"))?;
        }
        file.sync_all()
            .map_err(|e| format!("Save temporary restore snapshot: {e}"))?;
        file.rewind().map_err(|e| e.to_string())?;
        let preview = RestorePreview {
            token: format!(
                "{}-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos(),
                operation.0 .0.next_preview.fetch_add(1, Ordering::Relaxed)
            ),
            id,
            device_path: partition.device_path.into(),
            role: if id == 0 {
                "Complete MMC".into()
            } else {
                partition.role.into()
            },
            size_bytes: partition.size_bytes(),
            image_name: selection.name,
            sha256: hex(&hasher.finalize()),
        };
        *operation
            .0
             .0
            .prepared
            .lock()
            .map_err(|_| "Restore preview state is unavailable.")? = Some(PreparedRestore {
            preview: preview.clone(),
            file,
        });
        progress.emit(
            preview.size_bytes,
            "Image verified. Ready for explicit restore confirmation.",
            true,
        );
        Ok(preview)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn console_cancel_restore(
    state: State<'_, PartitionState>,
    token: String,
) -> Result<(), String> {
    let mut prepared = state
        .0
        .prepared
        .lock()
        .map_err(|_| "Restore preview state is unavailable.")?;
    if prepared.as_ref().is_some_and(|p| p.preview.token == token) {
        prepared.take();
    }
    Ok(())
}

#[tauri::command]
pub async fn console_restore(
    state: State<'_, PartitionState>,
    token: String,
    confirmed: bool,
    on_progress: Channel<PartitionProgress>,
) -> Result<TransferReport, String> {
    if !confirmed {
        return Err("Confirm the target and verified image before restoring.".into());
    }
    let operation = state.acquire()?;
    let mut prepared = operation.0.take_preview(&token)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let partition = target(prepared.preview.id)?;
        require_recovery()?;
        let mut progress = Progress::new(
            on_progress,
            partition.id,
            "restore",
            partition.size_bytes(),
            1,
            1,
        );
        progress.emit(0, "Rechecking verified snapshot…", true);
        verify_snapshot(&mut prepared.file, &prepared.preview, |n| {
            progress.emit(n, "Rechecking verified snapshot…", false)
        })?;
        progress.emit(0, "Restoring partition…", true);
        let sha1 = pce_recovery::nc::restore_partition_reader(
            partition.device_path,
            &mut prepared.file,
            partition.size_bytes(),
            |n| {
                progress.emit(
                    n,
                    if n == partition.size_bytes() {
                        "Synchronizing and verifying console checksum…"
                    } else {
                        "Restoring partition…"
                    },
                    n == partition.size_bytes(),
                );
            },
        )
        .map_err(|e| format!("Restore failed: {e}"))?;
        progress.emit(
            partition.size_bytes(),
            "Restore verified successfully.",
            true,
        );
        Ok(TransferReport {
            id: partition.id,
            bytes: partition.size_bytes(),
            sha256: prepared.preview.sha256,
            sha1: hex(&sha1),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn console_backup(
    state: State<'_, PartitionState>,
    ids: Vec<u8>,
    directory: String,
    on_progress: Channel<PartitionProgress>,
) -> Result<BackupReport, String> {
    let operation = state.acquire()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        backup(&ids, Path::new(&directory), on_progress)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn backup(
    ids: &[u8],
    directory: &Path,
    channel: Channel<PartitionProgress>,
) -> Result<BackupReport, String> {
    let selected = validate_ids(ids)?;
    if !directory.is_dir() {
        return Err("Choose an existing backup directory.".into());
    }
    let directory = directory.canonicalize().map_err(|e| e.to_string())?;
    let manifest_path = directory.join("chronos-backup.json");
    validate_outputs(&directory, &selected, &manifest_path)?;
    require_recovery()?;
    let mut files = Vec::new();
    for (index, partition) in selected.iter().enumerate() {
        let path = directory.join(backup_name(partition.id));
        let mut file = tempfile::Builder::new()
            .prefix(".chronos-backup-")
            .tempfile_in(&directory)
            .map_err(|e| e.to_string())?;
        let mut progress = Progress::new(
            channel.clone(),
            partition.id,
            "backup",
            partition.size_bytes(),
            index + 1,
            selected.len(),
        );
        progress.emit(0, "Backing up partition…", true);
        let hash = pce_recovery::dump_partition(
            partition.device_path,
            partition.size_bytes(),
            &mut file,
            |n| {
                progress.emit(
                    n,
                    if n == partition.size_bytes() {
                        "Verifying console checksum…"
                    } else {
                        "Backing up partition…"
                    },
                    n == partition.size_bytes(),
                );
            },
        )
        .map_err(|e| {
            format!(
                "Backup of {} failed; previously completed files are preserved: {e}",
                partition.device_path
            )
        })?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist_noclobber(&path)
            .map_err(|e| format!("Save verified backup without overwriting existing files: {e}"))?;
        files.push(BackupFile {
            id: partition.id,
            path: path.to_string_lossy().into_owned(),
            bytes: partition.size_bytes(),
            sha1: hex(&hash),
        });
        progress.emit(partition.size_bytes(), "Backup verified and saved.", true);
    }
    let manifest = serde_json::json!({ "format": "chronos-mmc-backup", "version": 1, "address": ADDRESS,
        "created_unix_seconds": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
        "environment": "ram_recovery", "integrity_algorithm": "sha1", "files": files });
    let mut file = tempfile::NamedTempFile::new_in(&directory).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut file, &manifest).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist_noclobber(&manifest_path)
        .map_err(|e| format!("Backups are saved, but the manifest could not be written: {e}"))?;
    Ok(BackupReport {
        directory: directory.to_string_lossy().into_owned(),
        files,
        manifest_path: manifest_path.to_string_lossy().into_owned(),
    })
}

fn target(id: u8) -> Result<&'static partitions::Partition, String> {
    partitions::lookup(id).ok_or_else(|| format!("Unknown MMC partition {id}."))
}
fn validate_ids(ids: &[u8]) -> Result<Vec<&'static partitions::Partition>, String> {
    if ids.is_empty() {
        return Err("Select at least one partition to back up.".into());
    }
    if ids.len() > partitions::PARTITIONS.len() + 1 {
        return Err("Too many partition selections.".into());
    }
    if ids.contains(&0) && ids.len() != 1 {
        return Err("Back up the complete MMC separately from individual partitions.".into());
    }
    let mut selected = Vec::new();
    for id in ids {
        if selected
            .iter()
            .any(|p: &&partitions::Partition| p.id == *id)
        {
            return Err("A partition was selected more than once.".into());
        }
        selected.push(target(*id)?);
    }
    Ok(selected)
}
fn backup_name(id: u8) -> String {
    if id == 0 {
        "mmcblk0.bin".into()
    } else {
        format!("mmcblk0p{id}.bin")
    }
}
fn validate_outputs(
    directory: &Path,
    selected: &[&partitions::Partition],
    manifest: &Path,
) -> Result<(), String> {
    for path in selected
        .iter()
        .map(|p| directory.join(backup_name(p.id)))
        .chain(std::iter::once(manifest.to_owned()))
    {
        // symlink_metadata also catches dangling symlinks: never replace an
        // earlier backup or follow an alias chosen by another process.
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                return Err(format!(
                    "Backup destination already exists: {}. Choose an empty or new directory.",
                    path.display()
                ))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
fn require_recovery() -> Result<(), String> {
    match pce_recovery::console::inspect(ADDRESS)? {
        Environment::RamRecovery => Ok(()),
        _ => Err("Start the console in RAM recovery before backing up or restoring partitions. No transfer was started.".into()),
    }
}
fn verify_snapshot(
    file: &mut File,
    preview: &RestorePreview,
    mut progress: impl FnMut(u64),
) -> Result<(), String> {
    if file.metadata().map_err(|e| e.to_string())?.len() != preview.size_bytes {
        return Err("The staged image size changed. Prepare the image again.".into());
    }
    file.rewind().map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut count = 0;
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > preview.size_bytes {
            return Err("The staged image size changed. Prepare the image again.".into());
        }
        hash.update(&buffer[..n]);
        progress(count);
    }
    if count != preview.size_bytes || hex(&hash.finalize()) != preview.sha256 {
        return Err("The staged image checksum changed. Prepare the image again.".into());
    }
    file.rewind().map_err(|e| e.to_string())?;
    Ok(())
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
struct HashingWriter<'a, W> {
    writer: &'a mut W,
    hasher: &'a mut Sha256,
}
impl<W: Write> Write for HashingWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let count = self.writer.write(bytes)?;
        self.hasher.update(&bytes[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}
struct Progress {
    channel: Channel<PartitionProgress>,
    id: u8,
    kind: String,
    total: u64,
    started: Instant,
    last: Instant,
    index: usize,
    count: usize,
}
impl Progress {
    fn new(
        channel: Channel<PartitionProgress>,
        id: u8,
        kind: &str,
        total: u64,
        index: usize,
        count: usize,
    ) -> Self {
        Self {
            channel,
            id,
            kind: kind.into(),
            total,
            started: Instant::now(),
            last: Instant::now(),
            index,
            count,
        }
    }
    fn emit(&mut self, current: u64, message: &str, force: bool) {
        if !force && self.last.elapsed() < Duration::from_millis(100) {
            return;
        }
        // Disconnecting the UI does not abort a write half-way through. The
        // operation remains locked until the device has finished verification.
        let _ = self.channel.send(PartitionProgress {
            id: self.id,
            kind: self.kind.clone(),
            current,
            total: self.total,
            elapsed_ms: self.started.elapsed().as_millis() as u64,
            message: message.into(),
            index: self.index,
            count: self.count,
        });
        self.last = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn single_operation_guard_survives_panics_and_refuses_overlap() {
        let state = PartitionState::default();
        let copy = state.clone();
        let _ = std::panic::catch_unwind(move || {
            let _operation = copy.acquire().unwrap();
            assert!(copy.acquire().is_err());
            assert!(copy.is_busy());
            panic!("simulated worker failure");
        });
        assert!(!state.is_busy());
        assert!(state.acquire().is_ok());
    }
    #[test]
    fn selections_are_validated_without_connecting_to_a_console() {
        assert!(validate_ids(&[]).is_err());
        assert!(validate_ids(&[3]).is_err());
        assert!(validate_ids(&[7, 7]).is_err());
        assert!(validate_ids(&[0, 7]).is_err());
        assert_eq!(validate_ids(&[0]).unwrap()[0].size_bytes(), 3_909_091_328);
        assert_eq!(validate_ids(&[6, 7]).unwrap().len(), 2);
        assert_eq!(backup_name(0), "mmcblk0.bin");
        assert_eq!(backup_name(8), "mmcblk0p8.bin");
    }
    #[test]
    fn existing_backup_or_manifest_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("chronos-backup.json");
        let selected = validate_ids(&[7]).unwrap();
        assert!(validate_outputs(dir.path(), &selected, &manifest).is_ok());
        std::fs::write(dir.path().join("mmcblk0p7.bin"), b"previous backup").unwrap();
        assert!(validate_outputs(dir.path(), &selected, &manifest).is_err());
        assert_eq!(
            std::fs::read(dir.path().join("mmcblk0p7.bin")).unwrap(),
            b"previous backup"
        );
        std::fs::remove_file(dir.path().join("mmcblk0p7.bin")).unwrap();
        std::fs::write(&manifest, b"previous manifest").unwrap();
        assert!(validate_outputs(dir.path(), &selected, &manifest).is_err());
    }
    #[test]
    fn preview_is_a_private_immutable_snapshot_and_token_is_single_use() {
        let state = PartitionState::default();
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"image").unwrap();
        let preview = RestorePreview {
            token: "one".into(),
            id: 7,
            device_path: "/dev/mmcblk0p7".into(),
            role: "Linux".into(),
            size_bytes: 5,
            image_name: "p7.bin".into(),
            sha256: hex(&Sha256::digest(b"image")),
        };
        *state.0.prepared.lock().unwrap() = Some(PreparedRestore {
            preview: preview.clone(),
            file,
        });
        assert!(state.take_preview("other").is_err());
        let mut staged = state.take_preview("one").unwrap();
        assert!(state.take_preview("one").is_err());
        assert!(verify_snapshot(&mut staged.file, &preview, |_| {}).is_ok());
        staged.file.rewind().unwrap();
        staged.file.write_all(b"other").unwrap();
        assert!(verify_snapshot(&mut staged.file, &preview, |_| {}).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn dangling_backup_symlink_is_also_a_collision() {
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("missing", dir.path().join("mmcblk0p7.bin")).unwrap();
        assert!(validate_outputs(
            dir.path(),
            &validate_ids(&[7]).unwrap(),
            &dir.path().join("chronos-backup.json")
        )
        .is_err());
    }
}
