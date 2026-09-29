use std::{
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{AppHandle, Manager};

pub fn connect(app: &AppHandle, interface: &str) -> Result<(), String> {
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
    let out = Command::new(pkexec)
        .arg(helper)
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
    Ok(())
}
