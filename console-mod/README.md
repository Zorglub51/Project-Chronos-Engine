# Console-side PCE Mini USB library mod

Canonical scripts deployed on the console for the USB stick library workflow.

## Files in this directory

| File | Console destination | Mode |
|---|---|---|
| `mount-usb-drives` | `/bin/mount-usb-drives` | `0755` |
| `gameapp` | `/etc/init.d/gameapp` | `0755` |
| `pce-usb-lib.sh` | `/usr/bin/pce-usb-lib.sh` | `0755` |
| `pce-usb-attach` | `/usr/bin/pce-usb-attach` | `0755` |
| `pce-usb-detach` | `/usr/bin/pce-usb-detach` | `0755` |
| `99-pce-usb.rules` | `/etc/udev/rules.d/99-pce-usb.rules` | `0644` |

## Mod install procedure (for a fresh stock console)

1. **Back up stock `gameapp`** to `/etc/init.d/gameapp.orig`. The mod uninstaller restores from this.
2. **Install the 6 files above** at the listed paths/modes.
3. **Add to `/etc/inittab`** (before `::sysinit:/etc/init.d/rcS`):
   ```
   ::sysinit:/bin/mount-usb-drives
   ```
   The stock inittab usually has a `::sysinit:/bin/mount-usb-drives` line already (hakchi mod legacy) — overwrite our content over the existing `/bin/mount-usb-drives`.
4. **Reload udev rules** (or just reboot):
   ```
   udevadm control --reload-rules
   ```

## Mod uninstall

1. Restore `/etc/init.d/gameapp` from `gameapp.orig`.
2. Remove `/usr/bin/pce-usb-*`, `/etc/udev/rules.d/99-pce-usb.rules`.
3. Replace `/bin/mount-usb-drives` with stock (no-op or hakchi version).
4. Remove the inittab `::sysinit:/bin/mount-usb-drives` line if not stock.
5. Reboot.

## What stays on NAND vs what comes from USB

The mod doesn't touch `/usr/game/` content. NAND remains pure stock (`alldata_original`). All custom content (patched scripts, hook libs, custom ROMs, lineup PSBs) lives on the USB stick under `/game/`. When the stick is plugged, `mount-usb-drives` wholesale-binds `/mnt/usb/game` over `/usr/game`. Engine loads from USB transparently.

## What the engine actually picks up from USB

The bind makes `/usr/game/lib/m2hook_print.so` exist (it's `/mnt/usb/game/lib/m2hook_print.so`). Our `gameapp` checks for this file and conditionally sets `LD_PRELOAD` before exec'ing `./m2engage`. So:
- Bind active (USB plugged): hook + probe_gl loaded → custom lineup behaviour.
- Bind absent (stock boot): no preload, vanilla engine.

## Test matrix

See `/Users/vincentaycirieix/.claude/projects/-Users-vincentaycirieix-dev-pce/memory/usb_library_workflow.md` — `## Test matrix (all working)` section.
