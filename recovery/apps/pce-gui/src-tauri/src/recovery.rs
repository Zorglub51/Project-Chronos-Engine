use std::path::PathBuf;
use std::time::{Duration, Instant};

use pce_fel::{BootPhase, Progress, RecoveryPayloads};
use serde::Serialize;
use sunxi_fel::{wait_for_device, DeviceState, FelDevice};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Copy, Serialize)]
pub enum PhaseId {
    Connect,
    Trigger,
    Version,
    Fes1Write,
    Fes1Exec,
    BootImgWrite,
    UbootWrite,
    UbootExec,
    AtagsWrite,
    KernelWrite,
    InitrdWrite,
    TrampolineWrite,
    KernelExec,
}

impl From<BootPhase> for PhaseId {
    fn from(p: BootPhase) -> Self {
        match p {
            BootPhase::Fes1Write => PhaseId::Fes1Write,
            BootPhase::Fes1Exec => PhaseId::Fes1Exec,
            BootPhase::BootImgWrite => PhaseId::BootImgWrite,
            BootPhase::UbootWrite => PhaseId::UbootWrite,
            BootPhase::UbootExec => PhaseId::UbootExec,
            BootPhase::AtagsWrite => PhaseId::AtagsWrite,
            BootPhase::KernelWrite => PhaseId::KernelWrite,
            BootPhase::InitrdWrite => PhaseId::InitrdWrite,
            BootPhase::TrampolineWrite => PhaseId::TrampolineWrite,
            BootPhase::KernelExec => PhaseId::KernelExec,
        }
    }
}

fn emit_start(app: &AppHandle, phase: PhaseId, label: &str, total: u64) {
    let _ = app.emit(
        "phase-start",
        serde_json::json!({ "phase": phase, "label": label, "total": total }),
    );
}
fn emit_advance(app: &AppHandle, phase: PhaseId, delta: u64) {
    let _ = app.emit(
        "phase-advance",
        serde_json::json!({ "phase": phase, "delta": delta }),
    );
}
fn emit_finish(app: &AppHandle, phase: PhaseId, elapsed_ms: u128) {
    let _ = app.emit(
        "phase-finish",
        serde_json::json!({ "phase": phase, "elapsed_ms": elapsed_ms }),
    );
}
fn emit_log(app: &AppHandle, level: &str, msg: impl Into<String>) {
    let _ = app.emit(
        "log",
        serde_json::json!({ "level": level, "msg": msg.into() }),
    );
}

pub fn run(app: AppHandle, payloads_dir: PathBuf, wait: Duration) {
    let res = run_inner(app.clone(), payloads_dir, wait);
    match res {
        Ok(()) => {
            let _ = app.emit(
                "recovery-done",
                serde_json::json!({ "ok": true, "msg": "Recovery boot sent. Wait for the USB network interface, then choose Connect USB network." }),
            );
        }
        Err(e) => {
            emit_log(&app, "error", e.clone());
            let _ = app.emit(
                "recovery-done",
                serde_json::json!({ "ok": false, "msg": e }),
            );
        }
    }
}

fn run_inner(app: AppHandle, payloads_dir: PathBuf, wait: Duration) -> Result<(), String> {
    let payloads = RecoveryPayloads::from_dir(&payloads_dir);
    payloads.check().map_err(|e| format!("{e}"))?;
    emit_log(
        &app,
        "info",
        format!("payloads OK: {}", payloads_dir.display()),
    );

    // Connect phase: wait for the console to appear on USB.
    let t0 = Instant::now();
    emit_start(
        &app,
        PhaseId::Connect,
        "Ready: switch the console ON now (waiting for startup USB probe 1f3a:efe8)",
        0,
    );
    let state = wait_for_device(wait).map_err(|e| format!("Startup USB detection failed: {e}. Switch the console OFF, check the data cable, click Start recovery again, then switch it ON when prompted."))?;
    emit_finish(&app, PhaseId::Connect, t0.elapsed().as_millis());

    // Detection only inspected descriptors; trigger() takes ownership once.
    match state {
        DeviceState::Fel => {
            emit_log(
                &app,
                "info",
                "device already in FEL mode (skipping trigger)",
            );
        }
        DeviceState::BootRom => {
            let t = Instant::now();
            emit_start(&app, PhaseId::Trigger, "Send FEL trigger CBW", 0);
            pce_fel::trigger_and_wait(Duration::from_secs(15))
                .map_err(|e| format!("trigger: {e}"))?;
            emit_finish(&app, PhaseId::Trigger, t.elapsed().as_millis());
        }
    }

    // Open in FEL state and read version.
    let dev = FelDevice::open().map_err(|e| format!("re-open: {e}"))?;
    if dev.state() != DeviceState::Fel {
        return Err(format!(
            "device did not transition to FEL (state={:?})",
            dev.state()
        ));
    }
    let t = Instant::now();
    emit_start(&app, PhaseId::Version, "Read FEL version (AWUSBFEX)", 0);
    let v = dev.version().map_err(|e| format!("version: {e}"))?;
    emit_finish(&app, PhaseId::Version, t.elapsed().as_millis());
    // Copy packed-struct fields out before borrowing them — `FelVersion` is
    // `#[repr(C, packed)]` and references to its fields would be unaligned.
    let soc_code = v.soc_code();
    if soc_code != 0x1667 {
        return Err(format!(
            "Unsupported SoC 0x{soc_code:04x}; expected PC Engine Mini A33/R16."
        ));
    }
    let soc_name = v.soc_name().unwrap_or("unknown");
    let soc_id_raw = v.soc_id;
    let protocol = v.protocol;
    let board = v.board;
    let _ = app.emit(
        "fel-version",
        serde_json::json!({
            "soc_code": soc_code,
            "soc_name": soc_name,
            "soc_id_raw": soc_id_raw,
            "protocol": protocol,
            "board": board,
        }),
    );

    // Run the actual boot recovery, forwarding pce-fel progress events.
    let mut started: Option<Instant> = None;
    let mut current_label: Option<&'static str> = None;
    pce_fel::boot_recovery(&dev, &payloads, |p| match p {
        Progress::Start { phase, total } => {
            started = Some(Instant::now());
            let label = match phase {
                BootPhase::Fes1Write => "Write FES1 SPL → 0x00002000",
                BootPhase::Fes1Exec => "Exec FES1 (DRAM init)",
                BootPhase::BootImgWrite => "Write boot.img → 0x43800000",
                BootPhase::UbootWrite => "Write U-Boot → 0x47000000",
                BootPhase::UbootExec => "Exec U-Boot → boota (no return)",
                BootPhase::AtagsWrite => "Write ATAGs → 0x40000100",
                BootPhase::KernelWrite => "Write kernel → 0x40008000",
                BootPhase::InitrdWrite => "Write initrd → 0x42000000",
                BootPhase::TrampolineWrite => "Write trampoline → 0x46000000",
                BootPhase::KernelExec => "Exec trampoline → kernel (no return)",
            };
            current_label = Some(label);
            emit_start(&app, phase.into(), label, total);
        }
        Progress::Advance { phase, delta } => {
            emit_advance(&app, phase.into(), delta);
        }
        Progress::Finish { phase } => {
            let elapsed = started.map(|t| t.elapsed().as_millis()).unwrap_or(0);
            emit_finish(&app, phase.into(), elapsed);
        }
    })
    .map_err(|e| format!("boot_recovery: {e}"))?;

    let _ = current_label;
    drop(dev);

    // ---- Post-exec liveness: did the CPU actually jump? ----
    //
    // If our `exe_no_return` got an AWUS back but the SoC never jumped (e.g.
    // firmware waiting for a fel_status drain we skipped), the FEL device
    // would still be on USB. If it jumped, the USB device disconnects within
    // ~1 s as the kernel takes over the USB controller.
    std::thread::sleep(Duration::from_millis(1500));
    let still_on_usb = sunxi_fel::is_present();
    if still_on_usb {
        emit_log(
            &app,
            "warn",
            "device still on USB 1.5 s after exec — CPU likely did NOT jump (firmware may need fel_status drain).",
        );
    } else {
        emit_log(
            &app,
            "info",
            "device left USB after exec — CPU jumped, U-Boot is running.",
        );
    }

    // No per-recovery probe needed — `netprobe::spawn_persistent` runs for
    // the lifetime of the app from `main.rs::setup`.
    Ok(())
}
