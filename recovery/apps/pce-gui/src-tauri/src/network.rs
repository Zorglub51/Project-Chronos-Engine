use std::{
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{AppHandle, Manager};

#[derive(serde::Serialize)]
pub struct ConnectionReport {
    environment: pce_recovery::console::Environment,
    message: String,
}

pub fn connect(app: &AppHandle, interface: &str) -> Result<ConnectionReport, String> {
    if !linux_network::discover()
        .map_err(|e| e.to_string())?
        .iter()
        .any(|i| i.name == interface)
    {
        return Err("The selected USB network interface is no longer connected.".into());
    }
    let candidates = [
        Some(PathBuf::from("/usr/libexec/pce-recovery-network")),
        app.path()
            .resource_dir()
            .ok()
            .map(|p| p.join("pce-recovery-network")),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("pce-recovery-network"))),
    ];
    let helper = candidates
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
        .ok_or(
        "Network setup helper is missing; install pce-recovery-network alongside the application.",
    )?;
    let pkexec = Path::new("/usr/bin/pkexec");
    if !pkexec.is_file() {
        return Err("Install polkit to configure the USB network, or use the manual commands in the README.".into());
    }
    // Root cannot normally read another user's FUSE mount. Stage the helper
    // outside the AppImage before pkexec; keep the private directory alive
    // through authentication and execution, then remove it automatically.
    let (_staging, executable) = stage_helper(&helper)?;
    let out = Command::new(pkexec)
        .arg(executable)
        .arg("configure")
        .arg(interface)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "USB network setup cancelled or failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    use pce_recovery::console::{self, Environment};
    let (environment, message) = match console::inspect(linux_network::PEER) {
        Ok(Environment::RamRecovery) => (Environment::RamRecovery, "RAM recovery verified. The console is ready for partition backups and restores.".to_string()),
        Ok(Environment::NotRamRecovery) => (Environment::NotRamRecovery, "USB network connected, but the console is not running RAM recovery. Switch it OFF, unplug USB, reconnect while OFF, then click Start recovery and switch ON when prompted.".to_string()),
        Ok(Environment::Unknown) => (Environment::Unknown, "USB network connected, but the console's root filesystem could not be identified. Recovery has not been verified.".to_string()),
        Err(e) => (Environment::Unknown, format!("USB network configured, but recovery could not be verified: {e}. Wait for the console to finish starting, then click Connect USB network again.")),
    };
    Ok(ConnectionReport {
        environment,
        message,
    })
}

fn stage_helper(source: &Path) -> Result<(tempfile::TempDir, PathBuf), String> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("pce-recovery-network-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let dir = builder
        .tempdir()
        .map_err(|e| format!("Prepare network helper: {e}"))?;
    let target = dir.path().join("pce-recovery-network");
    std::fs::copy(source, &target).map_err(|e| format!("Copy network helper: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("Network helper permissions: {e}"))?;
    }
    Ok((dir, target))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn privileged_helper_is_copied_outside_the_bundle_and_cleaned_up() {
        let source_dir = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("helper");
        std::fs::write(&source, b"bundled helper").unwrap();
        let (staging, target) = stage_helper(&source).unwrap();
        assert!(!target.starts_with(source_dir.path()));
        assert_eq!(std::fs::read(&target).unwrap(), b"bundled helper");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(staging.path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        drop(staging);
        assert!(!target.exists());
        assert!(source.exists());
    }
}
