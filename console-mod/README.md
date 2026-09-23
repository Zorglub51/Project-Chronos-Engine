# Console-side PCE Mini USB library mod

Canonical scripts deployed on the console for the USB stick library workflow.

The bundled hook supports both Japanese (`040`, `1006JP`) and international
(`041`, `1006WW`) resources. See [console variants](../docs/CONSOLE-VARIANTS.md)
for template detection and model-specific PSB filenames.

## Files in this directory

| File | Console destination | Mode |
|---|---|---|
| `mount-usb-drives` | `/bin/mount-usb-drives` | `0755` |
| `gameapp` | `/etc/init.d/gameapp` | `0755` |
| `S11chronos-usb` | `/etc/init.d/S11chronos-usb` | `0755` |
| `pce-usb-lib.sh` | `/usr/bin/pce-usb-lib.sh` | `0755` |
| `pce-usb-attach` | `/usr/bin/pce-usb-attach` | `0755` |
| `pce-usb-detach` | `/usr/bin/pce-usb-detach` | `0755` |
| `99-pce-usb.rules` | `/etc/udev/rules.d/99-pce-usb.rules` | `0644` |

## Mod install procedure (for a fresh stock console)

1. **Back up stock `gameapp`** to `/etc/init.d/gameapp.orig`. The mod uninstaller restores from this.
2. **Install the 7 files above** at the listed paths/modes, and create `/mnt/usb` (root:root, `0755`) before the stock system makes its root filesystem read-only.
3. **Keep stock `/etc/inittab` and `rcS`.** If upgrading an older Chronos install, remove its `::sysinit:/bin/mount-usb-drives` line. Stock `rcS` remounts `/usr/game` read-only before running the `S??` scripts; mounting USB earlier would also make the USB filesystem read-only. USB setup now runs via S10udev cold-plug and S11chronos-usb, after this remount and before S99first_startup.
4. **Reload udev rules** (or just reboot):
   ```
   udevadm control --reload-rules
   ```

## Mod uninstall

1. Restore `/etc/init.d/gameapp` from `gameapp.orig`.
2. Remove `/usr/bin/pce-usb-*`, `/etc/udev/rules.d/99-pce-usb.rules`.
   Also remove `/etc/init.d/S11chronos-usb`.
3. Replace `/bin/mount-usb-drives` with stock (no-op or hakchi version).
4. Remove the inittab `::sysinit:/bin/mount-usb-drives` line if not stock.
5. Reboot.

## What stays on NAND vs what comes from USB

The mod leaves the original application partition intact. On USB, `/game/`
holds the engine, extracted resources, patched scripts and hook; published ROMs
and lineup packs live under `/library/published/`. `mount-usb-drives` binds
`/mnt/usb/game` over `/usr/game`, so the engine uses its usual paths.

Published ROMs live only in `/mnt/usb/library/published/roms/`. After the game
mount, both cold boot and hot-plug use the same helper to bind that directory
over `/usr/game/system/roms/`. The physical `game/system/roms/` directory may be
empty. No copy or symlink is needed on FAT32. A missing published ROM directory
rejects the stick; a failed ROM mount rolls back the partial game mount.

Install all updated USB scripts together, including `pce-usb-lib.sh`: the boot
script now calls the shared mount helper. On replacement, stop the engine before
changing mounts. On detach, remove the hook's per-file mounts and the ROM mount
before removing `/usr/game`; unrelated mounts/devices are left alone. Physical
unplug still forcibly stops the engine and cannot guarantee pending saves.

## What the engine actually picks up from USB

The bind makes `/usr/game/lib/m2hook_print.so` exist (it's `/mnt/usb/game/lib/m2hook_print.so`). Our `gameapp` checks for this file and conditionally sets `LD_PRELOAD` before exec'ing `./m2engage`. So:
- Bind active (USB plugged): hook + probe_gl loaded → custom lineup behaviour.
- Bind absent (stock boot): no preload, vanilla engine.

## Test matrix

Run `python3 -m unittest discover -s console-mod/tests -v` from the repository
root to exercise mount ordering, rollback and removal scope with simulated
mounts. Cold boot and hot-plug with these updated scripts still need validation
on the console; the Linux VM engine test does not exercise console startup.

`library/published/save/` is mounted over `/usr/game/save/` too. The editor reads
settings and active SRAM from this same location. New keys have an empty
`game/save/` directory. Existing live saves in that directory are preserved by
publication, and the launcher refuses to hide them: migrate them explicitly
before upgrading an old key. No personal saves are imported by the test-kit
builder.

## Build a P7 image and example key

[`tools/build-test-kit.py`](../tools/build-test-kit.py) copies a verified stock
JP P7 image, installs the USB mod, SSH/SFTP utilities, `gameapp.orig` and `/mnt/usb`, checks
file hashes/ownership/modes and runs read-only e2fsck. It keeps the image at
104,857,600 bytes for PCE Mini Recovery's P7 size check. No partition is flashed.
The updated PCE Mini Recovery accepts the ZIP directly; older versions need
the extracted `.bin`.

The same tool extracts fresh resources and six games from the owner's original
`alldata.bin`, copies the reference library's game metadata/covers, publishes
three folders and both lineups, and packages a FAT32-compatible example key.
Run `python3 tools/build-test-kit.py --help` for the explicit input paths. Original
images, engines and ROMs are not committed to this repository.

Inside the Linux VM, `sudo python3 test-environment/validate-console-kit.py KIT`
tests temporary FAT32/ext4 images with the P7's ARM BusyBox mount commands,
per-file overlays, settings persistence and stock engine/hook dependencies.
It uses a private mount namespace and never opens a physical console or disk.

## SSH/SFTP in generated P7 images

Generated P7 images also include [USB SSH/SFTP](remote-access/README.md), enabled
at `169.254.13.37:22` with root's password field empty in `/etc/shadow`. The stock
BusyBox remains in place. Static ARM binaries provide Dropbear and the SFTP
helper; `S30chronos-remote` and `pce-remote-lib.sh` configure/start them. Host keys
are created on the console, never shipped in the image. The seven-file manual
USB mod installation above does not by itself install these remote services.

Use `--p7-only` to rebuild this image without rebuilding a prepared USB library.

## Mount diagnostics

Create `/run/chronos-debug` before starting `gameapp` to enable launcher and
USB-script diagnostics for the current boot. Remove it to disable them. The
hook's detailed event traces are controlled by `M2HOOK_DEBUG=1` (already set by
the USB launcher). No per-frame mount or file scans are performed.

- `/tmp/.game.log`: engine/script output, startup file SHA-256 values, timestamped
  hook events, bind/unbind return values and errno, device/inode comparisons,
  mount snapshots and the actual count of pack setup failures. Bounded at 8 MiB
  by the hook.
- `/tmp/pce-usb.log`: USB mount lifecycle, source/destination paths, failed
  unmounts, underlying NAND/USB layers, memory snapshots. Rotates at 1 MiB,
  retaining one previous file in RAM.

`EINVAL` while unbinding a file for the first time means there was no previous
bind; it is expected. A successful bind should report `same_inode=yes`.
Failures are logged even when detailed tracing is disabled. Diagnostics never
print save contents. No `.nut` changes are required for these traces.

### Folder IO and SRAM persistence

The menu saves the outgoing context before `beginGameFolderSwap(tag)`, then
polls `pollGameFolderSwap()` once per rendered frame. One transient worker
(256 KiB stack) performs only filesystem operations. Squirrel, graphics and
emulator APIs stay on the menu thread. Input/demo/shutdown processing waits
until the worker completes and the script has loaded the incoming SRAM.
The old synchronous natives remain available for older scripts.

SRAM export compares disk bytes in bounded blocks and skips identical copies.
Changed exports still use a temporary file, fsync, rename and directory fsync.
Import writes only changed blocks; the save digest is refreshed only when its
value changes. No per-game SRAM cache or persistent worker is retained.

`save/data_008_0000.bin` is authoritative for the active pack and may be newer
than `folders/<lineup>/<pack>/saves/sram.bin`. Startup preserves valid active
saves instead of overwriting them with an older pack snapshot. `save/.sram-pack`
records ownership; `pending` marks a swap whose import is not yet committed.
On restart after an interrupted swap, the hook restores the snapshot belonging
to `folders/.current`. Existing installations without the ownership marker
adopt a valid live save for `.current` on first launch. Do not edit `.current`
by hand without preserving the matching SRAM ownership/snapshot.

Regression coverage: `python3 -m unittest -v test_save_slice test_save_digest`
from `test-environment/`. VM integration tests cover a slow worker, folder and
lineup changes, active SRAM preservation at startup and interrupted imports.
