# Project Chronos — Installer Plan

**Date**: 2026-05-25
**Foundation**: `~/dev/pce_recovery_mac` (existing FEL-boot recovery toolkit)
**Mod payload**: pre-built `.mod` files from `~/dev/pce-mini/USB/tools/USB_partitions/`
**Target**: a one-shot installer that takes a stock console to a fully-modded
console (and supports reversal back to stock), all without ever touching
NAND from the running OS.

**Critical simplification (2026-05-25 update)**: the user has pointed at
existing pre-built community `.mod` files. We do NOT patch ext4 on macOS.
We flash byte-for-byte canonical images. This is a macOS port of the
Windows `setup.bat` workflow that already exists at
`~/dev/pce-mini/USB/setup.bat`.

## 0. Why route through FEL recovery (not SSH)

The existing `console-mod/README.md` install path uses SSH against a
running console to drop scripts into the live filesystem. That works but
has three problems:

1. **Bricks easily** — a typo in `/etc/inittab` or a bad `gameapp` kills
   boot; recovery then needs FEL anyway.
2. **Requires SSH already enabled** — stock consoles don't ship with SSH;
   that's a separate one-time hakchi prerequisite.
3. **No offline diagnostics** — can't tell a never-modded console apart
   from a previously-modded one with leftovers.

Going via FEL + recovery initrd solves all three: every console enters
FEL on USB-cable + button-hold regardless of state, the recovery image
gives us read/write block-level access to NAND with full safety nets,
and we can diff partitions byte-for-byte to detect prior mods.

## 1. What `pce_recovery_mac` already gives us

### Working FEL boot path (`pce-cli boot-recovery`)
Single command brings the console up at `169.254.13.37`:
- Auto-triggers FEL if console is in BootRom
- Writes FES1 + boot.img + U-Boot via libusb (`crates/sunxi-fel`)
- Hands off to `boota 0x43800000` → custom kernel + initramfs runs as PID 1
- Initramfs brings up RNDIS gadget, ssh-server, tftp-server
- Console comes up at `169.254.13.37` ready for commands

### Partition I/O over TFTP
`crates/pce-recovery/{nc,tftp,partitions}.rs` plus `pce-cli partitions
{list,dump,restore}` — hardcoded partition table, exact-size validation,
streaming with progress events. Already used successfully to dump p1, p2,
p5, p10 (BACKUP/ in the repo).

| id | device                | role     | size       |
|----|----------------------|----------|------------|
| 1  | /dev/mmcblk0p1       | System   | 658 MiB    |
| 2  | /dev/mmcblk0p2       | System   | 8 MiB      |
| 5  | /dev/mmcblk0p5       | System   | 2 MiB      |
| **6** | **/dev/mmcblk0p6** | **Kernel** | **8 MiB** |
| **7** | **/dev/mmcblk0p7** | **Linux** | **100 MiB** |
| 8  | /dev/mmcblk0p8       | Saves    | 672 MiB    |
| 9  | /dev/mmcblk0p9       | Games    | 2.19 GiB   |
| 10 | /dev/mmcblk0p10      | System   | 4 MiB      |

### SSH against the recovery image (`pce-cli ssh "cmd"`)
`crates/pce-recovery/ssh.rs` (russh-based). Connects to root@169.254.13.37
with empty password, runs an arbitrary command, returns stdout/stderr/rc.
Used today for ad-hoc poking; we'll lean on it heavily for diagnostics
and offline filesystem manipulation.

### Tauri GUI shell (`apps/pce-gui/`)
Has progress events, partition table panel, SSH terminal, serial console
viewer. We extend this rather than build a separate installer UI.

### Built dist
A working `dist/PCE Mini Recovery.app` macOS bundle proves the toolchain
is functional end-to-end.

## 2. The mod payload (what we're installing)

**A note on terminology**: throughout this doc, `console-mod.tar` is just a
plain tar bundle of the files under `Project-Chronos-Engine/console-mod/`.
There is no OverlayFS / union-mount / layered-FS mechanism involved. We
mount the existing ext4 p7 read-write and modify files in it directly;
the tarball is just a convenient transport for the bundle of files to
add or replace.

The two partitions get fundamentally different treatments.

### Partition 6 — Custom kernel (FULL REPLACE)
Source: `~/dev/pce-mini/USB/tools/USB_partitions/mmcblk0p6.zip`
- Contains: `mmcblk0p6.mod` (8 388 608 bytes = exactly the p6 size).
- Built by the pce-mini community 2025-01-27.
- Replaces the stock kernel with one supporting USB-Ethernet gadget +
  the input/mass-storage drivers the mod relies on.
- **Flashed byte-for-byte via TFTP-PUT**, no manipulation. Compiled
  binary, would be impossible to patch in-place anyway.
- Shipped: 8 MB in the installer .app.

### Partition 7 — Linux rootfs (MINIMAL IN-SITU PATCH)
We do NOT ship a 100 MB p7 image. The stock p7 stays in place; we
apply minimal modifications on top of it using the recovery initrd as
our "trusted Linux environment" — it can mount ext4 R/W over the
console's NAND while the stock OS isn't running.

**Why this is better than shipping a 100 MB .mod:**
- Installer is tiny (just the 8 MB kernel + recovery initrd).
- `console-mod/` is the literal source of truth for every change. Diff
  a single file; the install matches the diff exactly.
- Audit-friendly: nothing hidden inside a binary blob.
- Update path: change a script, ship a new installer build, no kernel
  rebuild.
- Reproducible: any user can rebuild what the installer applies just
  by reading `console-mod/`.

**The patch payload** (sourced from `Project-Chronos-Engine/console-mod/`,
tarred at installer build time into `console-mod.tar`):

| Source                | Console path                          | Mode |
|-----------------------|---------------------------------------|------|
| `mount-usb-drives`    | `/bin/mount-usb-drives`               | 0755 |
| `gameapp`             | `/etc/init.d/gameapp` (rename current to `gameapp.orig`) | 0755 |
| `pce-usb-lib.sh`      | `/usr/bin/pce-usb-lib.sh`             | 0755 |
| `pce-usb-attach`      | `/usr/bin/pce-usb-attach`             | 0755 |
| `pce-usb-detach`      | `/usr/bin/pce-usb-detach`             | 0755 |
| `99-pce-usb.rules`    | `/etc/udev/rules.d/99-pce-usb.rules`  | 0644 |

**Plus one inittab edit**: idempotently ensure
`::sysinit:/bin/mount-usb-drives` is present in `/etc/inittab`.

**Plus optional dropbear keys**: drop persistent host keys at
`/etc/dropbear/dropbear_rsa_host_key` etc. so SSH-into-running-console
works without hakchi having pre-seeded them. (Generated once at build
time, shipped with the installer.)

Total patch payload size: a few KB.

### How the in-situ patch works
The recovery initrd's busybox already has `mount`, `tar`, `chmod`,
`mv`, `cat`, `sync`. The installer pushes the console-mod tarball via
ssh-stdin and runs a small remote script:

```sh
set -e
mount -t ext4 /dev/mmcblk0p7 /mnt
[ -f /mnt/etc/init.d/gameapp.orig ] || \
    cp /mnt/etc/init.d/gameapp /mnt/etc/init.d/gameapp.orig
tar -x -C /mnt -f -                          # stdin = our console-mod.tar
chmod 755 /mnt/bin/mount-usb-drives \
          /mnt/etc/init.d/gameapp \
          /mnt/usr/bin/pce-usb-{lib.sh,attach,detach}
chmod 644 /mnt/etc/udev/rules.d/99-pce-usb.rules
grep -q '^::sysinit:/bin/mount-usb-drives' /mnt/etc/inittab || \
    printf '::sysinit:/bin/mount-usb-drives\n' >> /mnt/etc/inittab
sync
umount /mnt
```

Single ssh-exec, runs in seconds. The recovery initrd's kernel
supports ext4 (we'll verify in Phase 0 of the first install attempt; if
not, we add `e2fsprogs` userspace tooling to the initrd — small change).

### What we do NOT touch
- **p1** (System — game data on NAND): never modified.
- **p2, p5, p8, p9, p10**: untouched.
- The bootloader (FES1, U-Boot in p1 or elsewhere): untouched.
- **p7 free space**: untouched. We only ADD/REPLACE specific paths.

## 3. Workflow phases — what the installer does

```
┌─ Phase 0  Pre-flight (host-side, no console needed) ────────────────┐
│  - Check macOS / libusb / FEL prereqs                                │
│  - Locate payloads: kernel.uImage + mod-payload (tarball of console-mod files)│
│  - SHA-256 verify payloads against shipped manifest                  │
│  - Pick install destination for backups: ./BACKUP/<console-serial>/  │
└──────────────────────────────────────────────────────────────────────┘

┌─ Phase 1  Enter recovery (existing pce-cli boot-recovery) ──────────┐
│  - Prompt: hold L+R+SELECT, plug USB cable, power on                 │
│  - Auto-detect BootRom -> FEL -> FES1 -> boot.img -> U-Boot -> kernel│
│  - Wait for 169.254.13.37 to answer (ssh ping)                       │
│  - Read /proc/cmdline + uname over ssh — confirm we're in recovery   │
└──────────────────────────────────────────────────────────────────────┘

┌─ Phase 2  Diagnose ─────────────────────────────────────────────────┐
│  Goal: classify the console as STOCK / MODDED-OURS / MODDED-OTHER /  │
│  UNKNOWN, before touching anything.                                  │
│                                                                       │
│  Probes (all read-only over TFTP + SSH):                             │
│   a. SHA-256 of p6 (kernel) vs known stock-uImage hash               │
│      -> matches STOCK_KERNEL_SHA           → stock                    │
│      -> matches our USB-noDebug SHA        → modded-ours              │
│      -> matches a hakchi kernel SHA        → modded-other (hakchi)    │
│      -> anything else                      → unknown                  │
│                                                                       │
│   b. SSH `mount -t ext4 -o ro /dev/mmcblk0p7 /mnt; ls /etc/init.d/   │
│      gameapp.orig; ls /bin/mount-usb-drives;                          │
│      grep 'pce-usb' /etc/udev/rules.d/* ; umount /mnt`                │
│      -> If gameapp.orig present + our scripts present → modded-ours  │
│      -> If hakchi-* scripts present, no gameapp.orig → modded-other  │
│                                                                       │
│   c. Inittab probe: grep mount-usb-drives /etc/inittab               │
│                                                                       │
│  Output to GUI: a clean state card                                   │
│   ┌────────────────────────────────────┐                              │
│   │ Console state: MODDED (Chronos v0.1)│                             │
│   │ Kernel: uImage-USB-noDebug a1b2…    │                             │
│   │ Rootfs scripts: 6/6 present         │                             │
│   │ Inittab line: present               │                             │
│   │ Last backup: 2026-05-04 20:34 (you) │                             │
│   └────────────────────────────────────┘                              │
└──────────────────────────────────────────────────────────────────────┘

┌─ Phase 3  Choose action ────────────────────────────────────────────┐
│  Branch on Phase 2 result, present options:                          │
│   STOCK              → [Install Mod]  [Cancel]                       │
│   MODDED-OURS        → [Re-flash]  [Uninstall]  [Cancel]             │
│   MODDED-OTHER       → [⚠ Will overwrite OTHER mod] [Cancel]         │
│   UNKNOWN            → [Backup Only] [Force install] [Cancel]        │
└──────────────────────────────────────────────────────────────────────┘

┌─ Phase 4  Backup ───────────────────────────────────────────────────┐
│  Always run before any write, regardless of detected state.         │
│  Destination: ./BACKUP/<console-id>/<ISO-date>/                     │
│   - mmcblk0p6.bin   (8 MiB, current kernel)                          │
│   - mmcblk0p7.bin   (100 MiB, current rootfs)                        │
│   - manifest.json   (sha-256, partition sizes, source state,         │
│                      pce-cli version, recovery-image version)        │
│  Progress: bytes streamed (chunked TFTP-GET, ~1.4 MB/s typical)      │
│  After both files: re-read first 4 KiB of each from device           │
│  and re-compare hashes — guard against transport corruption.         │
└──────────────────────────────────────────────────────────────────────┘

┌─ Phase 5  Prepare payloads (host-side, near-instant) ───────────────┐
│  1. Locate shipped payloads:                                         │
│     - <app>/Contents/Resources/install/mmcblk0p6.mod (8 MiB)         │
│     - <app>/Contents/Resources/install/console-mod.tar (~10 KB)          │
│     - <app>/Contents/Resources/install/manifest.json                 │
│  2. sha-256 each payload and compare to manifest. Abort if mismatch. │
│  3. Confirm mmcblk0p6.mod is exactly 8 388 608 bytes. Abort if not.  │
│  4. (Optional) verify console-mod.tar contents match the manifest's      │
│     per-file hashes (defence-in-depth).                              │
└──────────────────────────────────────────────────────────────────────┘

┌─ Phase 6a  Patch p7 in-situ via recovery initrd (live mount + tar) ─┐
│  Run this BEFORE writing p6, so a failure leaves the console with    │
│  stock kernel + (already-backed-up) p7 — full recovery from a single │
│  TFTP partition restore.                                             │
│                                                                       │
│   a. ssh: `mount -t ext4 /dev/mmcblk0p7 /mnt` — abort if fails.      │
│   b. ssh: backup-rename `gameapp` → `gameapp.orig` (only if no       │
│      .orig already exists — preserves true stock).                   │
│   c. push console-mod.tar via ssh stdin to `tar -x -C /mnt -f -`.        │
│   d. ssh: `chmod` to set canonical modes per the file table.         │
│   e. ssh: append inittab line idempotently.                          │
│   f. ssh: `sync && umount /mnt` — abort if umount fails.             │
│   g. ssh: re-mount RO, sha256sum each touched file, compare to       │
│      manifest, re-umount.                                            │
│                                                                       │
│  All steps run as a single `ssh -tt <multi-line script>` so abort    │
│  semantics are atomic (script uses `set -e`).                        │
└──────────────────────────────────────────────────────────────────────┘

┌─ Phase 6b  Write kernel to p6 (TFTP-PUT) ───────────────────────────┐
│   a. tftp-put install/mmcblk0p6.mod → /dev/mmcblk0p6                 │
│      (size: 8 MiB exact, no --allow-size-mismatch)                   │
│   b. ssh: sha256sum /dev/mmcblk0p6 → compare with manifest           │
└──────────────────────────────────────────────────────────────────────┘

┌─ Phase 7  Reboot + verify ──────────────────────────────────────────┐
│  - ssh `reboot` (or `sync; reboot -f` to bypass init's signal trap)  │
│  - The console disconnects from FEL/recovery, normal boot begins.    │
│  - Wait ~25 s for the bind-mount workflow to come up (USB stick      │
│    optional at this point — without stick, console boots stock JP).  │
│  - Re-probe at 169.254.13.37 over the USB-RNDIS gadget — should now  │
│    answer because new kernel has CONFIG_USB_ETHERNET_RNDIS=y         │
│    (proves p6 was written).                                          │
│  - SSH `ls -la /bin/mount-usb-drives /etc/init.d/gameapp* ; grep     │
│    mount-usb-drives /etc/inittab` — confirm mod-files applied.         │
│  - Emit a final ✅ Install complete card.                            │
└──────────────────────────────────────────────────────────────────────┘

┌─ Phase 8  (Optional) Initialise USB stick ──────────────────────────┐
│  If user has a stick plugged into the host machine, offer to format  │
│  it as FAT32 and lay down the canonical Chronos layout:              │
│   /Volumes/CHRONOS/                                                   │
│   ├── PCE Game Editor.app/         ← shipped with installer          │
│   ├── BACKUP/                      ← stock backups, copied from host │
│   ├── library/published/jp/        ← empty, editor populates         │
│   ├── library/published/us/                                          │
│   └── chronos.version              ← marker                          │
│  This is exactly the layout the editor expects (Stick-layout.md      │
│  wiki page).                                                          │
└──────────────────────────────────────────────────────────────────────┘
```

## 4. Uninstall path (same UI, branch from Phase 3 → "Uninstall")

Same recovery-mode entry, same backup, then:
1. **Kernel**: TFTP-PUT
   `./BACKUP/<console-id>/<date>/mmcblk0p6.bin` → `/dev/mmcblk0p6`.
   Restores stock kernel exactly.
2. **Rootfs**: TWO options, presented to user:
   - **Surgical (preferred)**: ssh into recovery, mount p7 R/W, remove
     each file the installer added, restore `gameapp.orig` →
     `gameapp`, remove the inittab line. Leaves user data (saves,
     custom files) intact. Idempotent and verifiable against the
     mod-files manifest.
   - **Full restore from backup**: TFTP-PUT
     `./BACKUP/<console-id>/<date>/mmcblk0p7.bin` → `/dev/mmcblk0p7`.
     Bit-for-bit revert. Used when user wants a guaranteed factory state
     or when surgical uninstall reports drift (some file was modified
     in ways we don't track).
3. Reboot + verify (Phase 7, but checking that the mod is gone).

Note: backups are per-console (we use the serial from `cat
/sys/.../mmc0/cid` over ssh as the directory name) so a uninstall on a
different console will fail loud rather than silently apply someone
else's backup.

## 5. New crate(s) we need to add to `pce_recovery_mac`

### `crates/pce-installer` (new)
Library crate. Exposes:
- `diagnose(ssh, partition_io) → ConsoleState`
- `backup_partitions(io, dir, ids) → BackupManifest`
- `patch_rootfs_image(in_path, out_path, mod_files) → Result<Hash>`
- `install(plan: InstallPlan) → StreamingProgress`
- `uninstall(plan: UninstallPlan) → StreamingProgress`

The library is what binary front-ends call. Keeps FFI-friendly shape
for both the CLI and the Tauri GUI.

### `bins/pce-cli` (extend)
Add subcommands:
- `pce-cli diagnose` — emit JSON state report
- `pce-cli install [--from <payloads-dir>] [--backup-dir <dir>]`
- `pce-cli uninstall --backup-dir <dir>`
- `pce-cli init-stick <mount-path>`  (Phase 8)

### `apps/pce-gui` (extend)
Add a new top-level tab/panel "Install":
- State card (from `diagnose`)
- Action buttons (Install / Uninstall / Backup-only)
- Progress timeline mirroring the Phase 4–7 events
- "Initialise USB stick" subpanel

### Payloads delivered with the installer
Bundled into the macOS .app (and Win .exe) at build time:
```
PCE Mini Recovery.app/Contents/Resources/payloads/
├── recovery/                ← existing FEL recovery image
│   ├── fes1.bin
│   ├── uboot.bin
│   ├── kernel.bin
│   ├── initrd.cpio.xz
│   └── trampoline.bin
└── install/                 ← new
    ├── mmcblk0p6.mod        (8 MiB, custom kernel partition image)
    ├── console-mod.tar          (~10 KB, console-mod files)
    ├── console-mod.manifest.json  per-file paths/modes/sha-256
    └── manifest.json        ({ kernel_sha: "...", mod_files_sha: "...",
                                version: "0.1.0",
                                kernel_source: "pce-mini USB_partitions
                                  mmcblk0p6.zip 2025-01-27",
                                mod_files_source: "Project-Chronos-Engine
                                  console-mod/ @ <git-sha>" })
```

**At installer build time**:
1. Unzip `mmcblk0p6.zip` → `mmcblk0p6.mod`; hash it.
2. Run `mod-assets/build-console-mod.sh` which walks
   `Project-Chronos-Engine/console-mod/` and produces:
   - `console-mod.tar` (paths laid out for `tar -x -C /` extraction)
   - `console-mod.manifest.json` listing each file's destination,
     intended mode, and sha-256
3. (Optional) generate persistent dropbear host keys, include in
   console-mod.tar under `etc/dropbear/`.
4. Emit top-level `manifest.json` with both payloads' hashes + version
   metadata.
5. Bundle all four files into the .app's Resources/install/.

When we update any `console-mod/` script, we rebuild console-mod.tar — the
kernel payload stays unchanged. When the kernel itself changes, we
swap `mmcblk0p6.mod` — the rootfs mod-files stay unchanged. The two are
independently versioned.

## 6. Unknowns we need to nail down BEFORE coding

Most of the original questions disappeared when we switched to flashing
pre-built `.mod` files instead of patching ext4 on macOS. Remaining
decisions:

1. **State diagnosis strategy** — with both p6 and p7 being whole-image
   replacements, the cleanest state probe is just two SHA-256 reads:
   - `sha256(p6) == STOCK_P6_SHA` → stock kernel
   - `sha256(p6) == MOD_P6_SHA`   → our (community) modded kernel
   - else → some other mod / unknown
   - Same for p7.
   We need the STOCK hashes. **Action**: at first install time on any
   stock console, dump `/dev/mmcblk0p6` and `/dev/mmcblk0p7` and add
   those hashes to the installer's bundled stock-hash list. (Easy to do
   in the field on the very first console we install onto.)

2. **Console serial / "console-id"** — same as before: probe
   `/sys/.../mmc0/.../cid` over ssh. Default: yes; falls back to a
   bootrom-reported chip ID hash.

3. **Editor `.app` shipping** — does the installer ship the editor too,
   or is the editor a separate download? Default: ship editor INSIDE
   the installer .app under `Resources/payloads/editor/` so a single
   download bootstraps everything.

4. **Manifest signing** — the .mod files are critical. Should
   `manifest.json` be signed (e.g. minisign) and verified at runtime,
   to protect against payload tampering? Default for v0.1: ship a
   plain manifest, document that users should verify the installer .app
   notarisation is intact. Add signing in v0.2.

5. **What does `inject_hack` in setup.bat actually do?** — **VERIFIED
   2026-05-25**. The entire section is literally:
   ```
   nand_dump.exe restore 6 USB_partitions\mmcblk0p6.mod
   nand_dump.exe restore 7 USB_partitions\mmcblk0p7.mod
   ```
   Two TFTP-PUT operations against partitions 6 and 7. Nothing else.
   No ssh commands, no host-key seeding, no inittab editing — those
   are all already baked into `mmcblk0p7.mod`. Our installer is
   functionally a 1-to-1 port; the only thing we add on top is the
   Backup phase (which the Windows setup.bat does as a separate
   menu option, not as part of inject_hack).

## 7. Build / release pipeline

- `Project-Chronos-Engine` produces `console-mod/` (mod scripts) and
  `editor/` (editor app).
- A small `mod-assets/build-console-mod.sh` script walks `console-mod/` and
  emits `console-mod.tar` + `console-mod.manifest.json`. Idempotent.
- `pce_recovery_mac` builds the macOS `.app`, embeds:
  - `recovery/` payloads (existing)
  - `install/` payloads (new, pulled from `mod-assets/`)
  - The shipped editor `.app` (optional — gated by a build flag)
- GitHub Actions: a single matrix job (macos-14, windows-latest) that
  builds the GUI + bundles. Output is a notarised .app on macOS and a
  signed .exe on Windows.

## 8. Safety invariants (must hold at every step)

- **Never write a partition without backing it up first** — Phase 4
  always runs, even on re-installs.
- **TFTP writes are size-equal-only** by default; the only exception is
  the kernel partition (Phase 6 c), and that path is explicit.
- **Every write is hash-verified by reading back** — Phase 6 b, d.
- **Console is left in either fully-stock or fully-modded state**,
  never half-applied. If Phase 6 a (p7 write) fails, abort before Phase
  6 c — old kernel still boots stock rootfs.
- **Backups are immutable** — installer never writes into an existing
  `BACKUP/<console-id>/<date>/` directory; always new timestamp.
- **Recovery exit is automatic** — the installer's "Cancel" at any
  point issues `ssh reboot` to drop FEL/recovery and the console comes
  back to normal boot in <30 s.

## 9. First milestone (1-day target)

Skip the GUI initially. Get the CLI end-to-end working:

```
pce-cli boot-recovery
pce-cli diagnose             # prints "STOCK" or our state
pce-cli install \
  --kernel ./install/kernel-USB-noDebug.uImage \
  --mod-files ./install/console-mod.tar \
  --backup ./BACKUP/
# console reboots, comes back at 169.254.13.37 (new kernel),
# bind-mounts /usr/game from USB if stick present
```

Then wrap in Tauri. The GUI is wholly cosmetic over the CLI commands.

## 10. Open question for you

**The "new 6" — which kernel do you want as the permanent installed
kernel?** I'm assuming `uImage-USB-noDebug` from `~/dev/pce-mini/Kernel/`
because that's what the recovery image already uses and proves out.
If you have a different specific kernel target (e.g. a custom-built one
with additional patches), point me at it and I'll use that.

And: anything beyond the 6 `console-mod/` files we should also drop
into p7? (Persistent SSH host keys? A pre-installed `htop`? `tcpdump`?
Anything else "we have identified" that I'm missing?)
