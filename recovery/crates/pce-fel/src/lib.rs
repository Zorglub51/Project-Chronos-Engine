//! PC Engine Mini-specific FEL layer: USB trigger CBW + recovery boot.
//!
//! Boot flow (trampoline-based, separate kernel + initrd, no Android wrapper):
//!
//! ```text
//!   write 0x00002000  fes1.bin       ; DRAM init
//!   exe   0x00002000
//!   write 0x40000100  atags.bin      ; ATAG list (CORE / MEM / INITRD2 / CMDLINE / NONE)
//!   write 0x40008000  kernel.bin     ; ARM kernel (zImage or Image)
//!   write 0x42000000  initrd.cpio.xz ; ramdisk
//!   write 0x46000000  trampoline.bin ; ARM stub: cleanup + jump
//!   exe   0x46000000                 ; r0=0, r1=mach_id, r2=ATAGs, jump to kernel
//! ```

use futures_lite::future::block_on;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use sunxi_fel::{DeviceState, FelDevice, FEL_PID, FEL_VID};
use thiserror::Error;
use tracing::{debug, info, trace};

// =============================================================================
// USB trigger (PCE Mini Boot ROM stub → FEL mode)
// =============================================================================

pub const TRIGGER_CBW: [u8; 31] = [
    0x55, 0x53, 0x42, 0x43, // "USBC"
    0x4a, 0x53, 0x48, 0x4b, // tag "JSHK"
    0x20, 0x00, 0x00, 0x00, // dCBWDataTransferLength = 32
    0x00, 0x00, 0x02, 0xf8, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

const BOOTROM_EP_OUT: u8 = 0x02;

#[derive(Debug, Error)]
pub enum TriggerError {
    #[error("no PCE Mini found at VID:PID {vid:04x}:{pid:04x}", vid = FEL_VID, pid = FEL_PID)]
    NotFound,

    #[error("device is in {actual:?} state, trigger only valid in BootRom")]
    NotInBootRom { actual: DeviceState },

    #[error("USB error: {0}")]
    Usb(String),

    #[error("device did not transition to FEL mode within {0:?}")]
    Timeout(Duration),
}

pub fn trigger() -> Result<(), TriggerError> {
    let mut matches = nusb::list_devices()
        .map_err(|e| TriggerError::Usb(e.to_string()))?
        .filter(|d| d.vendor_id() == FEL_VID && d.product_id() == FEL_PID);
    let info = matches.next().ok_or(TriggerError::NotFound)?;
    if matches.next().is_some() {
        return Err(TriggerError::Usb(
            "Connect only one PCE Mini at a time.".into(),
        ));
    }

    debug!(
        bus = info.bus_number(),
        addr = info.device_address(),
        "opening for trigger"
    );
    let device = info.open().map_err(|e| TriggerError::Usb(e.to_string()))?;

    let state = sunxi_fel::detect_state(&device)
        .map_err(|e| TriggerError::Usb(e.to_string()))?
        .0;
    if state != DeviceState::BootRom {
        return Err(TriggerError::NotInBootRom { actual: state });
    }

    let interface = device
        .detach_and_claim_interface(0)
        .map_err(|e| TriggerError::Usb(e.to_string()))?;
    trace!("sending trigger CBW");
    let comp = block_on(interface.bulk_out(BOOTROM_EP_OUT, TRIGGER_CBW.to_vec()));
    comp.status
        .map_err(|e| TriggerError::Usb(format!("trigger OUT: {e}")))?;
    Ok(())
}

pub fn trigger_and_wait(timeout: Duration) -> Result<(), TriggerError> {
    trigger()?;
    let deadline = Instant::now() + timeout;
    let remaining = deadline.saturating_duration_since(Instant::now());
    sunxi_fel::wait_for_state(DeviceState::Fel, remaining)
        .map_err(|_| TriggerError::Timeout(timeout))
}

// =============================================================================
// Recovery boot orchestration
// =============================================================================

/// Two boot paths we support:
///
/// - **Chronos** — the proven Project-Chronos sequence: factory `uboot.bin`
///   reads our `boot.img.tftp.cpio.xz` (Android-wrapped kernel + initrd)
///   from RAM and `boota`s into it. No ATAGs, no trampoline. This is what
///   `setup.bat` ships and what hundreds of users have run; it works on
///   any factory NAND state.
/// - **Trampoline** — our standalone path: kernel + initrd + ATAGs at
///   discrete addresses, with an asm trampoline that does the v7 cache
///   flush + register setup + jump. Cleaner conceptually but boots only
///   reliably with specific NAND/CPU states; falls through to NAND OS
///   on a lot of factory consoles.
///
/// `from_dir` picks Chronos if both `boot.img.tftp.cpio.xz` and
/// `uboot.bin` are present in the dir, otherwise falls back to
/// Trampoline. So a `payloads/` dir gets you Chronos out of the box.
pub enum RecoveryPayloads {
    Chronos {
        fes1: PathBuf,
        boot_img: PathBuf,
        uboot: PathBuf,
    },
    Trampoline {
        fes1: PathBuf,
        atags: PathBuf,
        kernel: PathBuf,
        initrd: PathBuf,
        trampoline: PathBuf,
    },
}

// Chronos addresses (must match Project-Chronos `setup.bat`).
pub const FES1_ADDR: u32 = 0x0000_2000;
pub const BOOTIMG_ADDR: u32 = 0x4380_0000;
pub const UBOOT_ADDR: u32 = 0x4700_0000;

// Trampoline addresses (legacy path).
pub const ATAGS_ADDR: u32 = 0x4000_0100;
pub const KERNEL_ADDR: u32 = 0x4000_8000;
pub const INITRD_ADDR: u32 = 0x4200_0000;
pub const TRAMPOLINE_ADDR: u32 = 0x4600_0000;
pub const R16_SCRATCH_ADDR: u32 = 0x0000_1000;

impl RecoveryPayloads {
    pub fn from_dir(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        let boot_img = dir.join("boot.img.tftp.cpio.xz");
        let uboot = dir.join("uboot.bin");
        if boot_img.is_file() && uboot.is_file() {
            return Self::Chronos {
                fes1: dir.join("fes1.bin"),
                boot_img,
                uboot,
            };
        }
        Self::Trampoline {
            fes1: dir.join("fes1.bin"),
            atags: dir.join("atags.bin"),
            kernel: dir.join("kernel.bin"),
            initrd: dir.join("initrd.cpio.xz"),
            trampoline: dir.join("trampoline.bin"),
        }
    }

    pub fn check(&self) -> Result<(), RecoveryError> {
        let paths: Vec<&PathBuf> = match self {
            Self::Chronos {
                fes1,
                boot_img,
                uboot,
            } => vec![fes1, boot_img, uboot],
            Self::Trampoline {
                fes1,
                atags,
                kernel,
                initrd,
                trampoline,
            } => {
                vec![fes1, atags, kernel, initrd, trampoline]
            }
        };
        for p in paths {
            if !p.is_file() {
                return Err(RecoveryError::MissingPayload(p.clone()));
            }
        }
        Ok(())
    }

    pub fn mode_label(&self) -> &'static str {
        match self {
            Self::Chronos { .. } => "chronos",
            Self::Trampoline { .. } => "trampoline",
        }
    }
}

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("payload file not found: {0}")]
    MissingPayload(PathBuf),

    #[error("read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("FEL error: {0}")]
    Fel(#[from] sunxi_fel::FelError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootPhase {
    // shared
    Fes1Write,
    Fes1Exec,
    // chronos
    BootImgWrite,
    UbootWrite,
    UbootExec,
    // trampoline
    AtagsWrite,
    KernelWrite,
    InitrdWrite,
    TrampolineWrite,
    KernelExec,
}

impl BootPhase {
    pub fn label(self) -> &'static str {
        match self {
            BootPhase::Fes1Write => "FES1 write       ",
            BootPhase::Fes1Exec => "FES1 exec        ",
            BootPhase::BootImgWrite => "boot.img write   ",
            BootPhase::UbootWrite => "U-Boot write     ",
            BootPhase::UbootExec => "U-Boot exec      ",
            BootPhase::AtagsWrite => "ATAGs write      ",
            BootPhase::KernelWrite => "kernel write     ",
            BootPhase::InitrdWrite => "initrd write     ",
            BootPhase::TrampolineWrite => "trampoline write ",
            BootPhase::KernelExec => "kernel exec      ",
        }
    }

    /// Phases that don't return — caller shouldn't expect a status read.
    pub fn is_terminal_exec(self) -> bool {
        matches!(self, BootPhase::KernelExec | BootPhase::UbootExec)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Progress {
    Start { phase: BootPhase, total: u64 },
    Advance { phase: BootPhase, delta: u64 },
    Finish { phase: BootPhase },
}

pub fn boot_recovery<F: FnMut(Progress)>(
    dev: &FelDevice,
    payloads: &RecoveryPayloads,
    mut on_progress: F,
) -> Result<(), RecoveryError> {
    payloads.check()?;
    info!(mode = payloads.mode_label(), "starting recovery boot");

    match payloads {
        RecoveryPayloads::Chronos {
            fes1,
            boot_img,
            uboot,
        } => boot_chronos(dev, fes1, boot_img, uboot, &mut on_progress),
        RecoveryPayloads::Trampoline {
            fes1,
            atags,
            kernel,
            initrd,
            trampoline,
        } => boot_trampoline(
            dev,
            fes1,
            atags,
            kernel,
            initrd,
            trampoline,
            &mut on_progress,
        ),
    }
}

/// Project-Chronos sequence: FES1 → boot.img → uboot → exec uboot.
/// Factory U-Boot's compiled-in `bootcmd` does the work after that.
fn boot_chronos<F: FnMut(Progress)>(
    dev: &FelDevice,
    fes1_path: &Path,
    boot_img_path: &Path,
    uboot_path: &Path,
    on_progress: &mut F,
) -> Result<(), RecoveryError> {
    let fes1 = read(fes1_path)?;
    on_progress(Progress::Start {
        phase: BootPhase::Fes1Write,
        total: fes1.len() as u64,
    });
    dev.write_progress(FES1_ADDR, &fes1, |n| {
        on_progress(Progress::Advance {
            phase: BootPhase::Fes1Write,
            delta: n as u64,
        });
    })?;
    on_progress(Progress::Finish {
        phase: BootPhase::Fes1Write,
    });

    on_progress(Progress::Start {
        phase: BootPhase::Fes1Exec,
        total: 0,
    });
    info!(
        addr = format!("{:#010x}", FES1_ADDR),
        "executing FES1 (DRAM init)"
    );
    dev.exe(FES1_ADDR)?;
    std::thread::sleep(Duration::from_secs(2));
    on_progress(Progress::Finish {
        phase: BootPhase::Fes1Exec,
    });

    let boot_img = read(boot_img_path)?;
    on_progress(Progress::Start {
        phase: BootPhase::BootImgWrite,
        total: boot_img.len() as u64,
    });
    dev.write_progress(BOOTIMG_ADDR, &boot_img, |n| {
        on_progress(Progress::Advance {
            phase: BootPhase::BootImgWrite,
            delta: n as u64,
        });
    })?;
    on_progress(Progress::Finish {
        phase: BootPhase::BootImgWrite,
    });

    let uboot = read(uboot_path)?;
    on_progress(Progress::Start {
        phase: BootPhase::UbootWrite,
        total: uboot.len() as u64,
    });
    dev.write_progress(UBOOT_ADDR, &uboot, |n| {
        on_progress(Progress::Advance {
            phase: BootPhase::UbootWrite,
            delta: n as u64,
        });
    })?;
    on_progress(Progress::Finish {
        phase: BootPhase::UbootWrite,
    });

    on_progress(Progress::Start {
        phase: BootPhase::UbootExec,
        total: 0,
    });
    info!(addr = format!("{:#010x}", UBOOT_ADDR), "executing U-Boot");
    dev.exe_no_return(UBOOT_ADDR)?;
    on_progress(Progress::Finish {
        phase: BootPhase::UbootExec,
    });
    Ok(())
}

/// Trampoline sequence: FES1 → ATAGs → kernel → initrd → trampoline → exec.
fn boot_trampoline<F: FnMut(Progress)>(
    dev: &FelDevice,
    fes1_path: &Path,
    atags_path: &Path,
    kernel_path: &Path,
    initrd_path: &Path,
    trampoline_path: &Path,
    on_progress: &mut F,
) -> Result<(), RecoveryError> {
    let fes1 = read(fes1_path)?;
    on_progress(Progress::Start {
        phase: BootPhase::Fes1Write,
        total: fes1.len() as u64,
    });
    dev.write_progress(FES1_ADDR, &fes1, |n| {
        on_progress(Progress::Advance {
            phase: BootPhase::Fes1Write,
            delta: n as u64,
        });
    })?;
    on_progress(Progress::Finish {
        phase: BootPhase::Fes1Write,
    });

    on_progress(Progress::Start {
        phase: BootPhase::Fes1Exec,
        total: 0,
    });
    info!(
        addr = format!("{:#010x}", FES1_ADDR),
        "executing FES1 (DRAM init)"
    );
    dev.exe(FES1_ADDR)?;
    std::thread::sleep(Duration::from_secs(2));
    on_progress(Progress::Finish {
        phase: BootPhase::Fes1Exec,
    });

    let atags = read(atags_path)?;
    on_progress(Progress::Start {
        phase: BootPhase::AtagsWrite,
        total: atags.len() as u64,
    });
    dev.write_progress(ATAGS_ADDR, &atags, |n| {
        on_progress(Progress::Advance {
            phase: BootPhase::AtagsWrite,
            delta: n as u64,
        });
    })?;
    on_progress(Progress::Finish {
        phase: BootPhase::AtagsWrite,
    });

    let kernel = read(kernel_path)?;
    on_progress(Progress::Start {
        phase: BootPhase::KernelWrite,
        total: kernel.len() as u64,
    });
    dev.write_progress(KERNEL_ADDR, &kernel, |n| {
        on_progress(Progress::Advance {
            phase: BootPhase::KernelWrite,
            delta: n as u64,
        });
    })?;
    on_progress(Progress::Finish {
        phase: BootPhase::KernelWrite,
    });

    let initrd = read(initrd_path)?;
    on_progress(Progress::Start {
        phase: BootPhase::InitrdWrite,
        total: initrd.len() as u64,
    });
    dev.write_progress(INITRD_ADDR, &initrd, |n| {
        on_progress(Progress::Advance {
            phase: BootPhase::InitrdWrite,
            delta: n as u64,
        });
    })?;
    on_progress(Progress::Finish {
        phase: BootPhase::InitrdWrite,
    });

    let trampoline = read(trampoline_path)?;
    on_progress(Progress::Start {
        phase: BootPhase::TrampolineWrite,
        total: trampoline.len() as u64,
    });
    dev.write_progress(TRAMPOLINE_ADDR, &trampoline, |n| {
        on_progress(Progress::Advance {
            phase: BootPhase::TrampolineWrite,
            delta: n as u64,
        });
    })?;
    on_progress(Progress::Finish {
        phase: BootPhase::TrampolineWrite,
    });

    info!(
        scratch = format!("{:#010x}", R16_SCRATCH_ADDR),
        "flushing I-cache via SRAM thunk"
    );
    dev.disable_icache(R16_SCRATCH_ADDR)?;

    on_progress(Progress::Start {
        phase: BootPhase::KernelExec,
        total: 0,
    });
    info!(
        addr = format!("{:#010x}", TRAMPOLINE_ADDR),
        "executing trampoline -> kernel"
    );
    dev.exe_no_return(TRAMPOLINE_ADDR)?;
    on_progress(Progress::Finish {
        phase: BootPhase::KernelExec,
    });
    Ok(())
}

fn read(path: &Path) -> Result<Vec<u8>, RecoveryError> {
    std::fs::read(path).map_err(|source| RecoveryError::Io {
        path: path.to_owned(),
        source,
    })
}
