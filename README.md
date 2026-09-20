# Project Chronos Engine

A modding toolkit for the PC Engine Mini / TurboGrafx-16 Mini console that lets you ship a custom game library on a USB stick — drop the stick in, the console boots into your library; pull the stick out, you're back to factory stock.

## What's in this repo

| Directory | Role |
|---|---|
| `editor/` | Tauri + Rust GUI app to assemble a custom game lineup, manage covers, set arch/country/players, and publish a library to a USB stick. |
| `console-mod/` | The shell scripts + udev rule + `gameapp` replacement that get installed on the console once. Provides USB detection, hot-plug, bind-mount swap, and the conditional `LD_PRELOAD` of the hook. |
| `console-mod/hook-src/` | C source for `m2hook_print.so` — the LD_PRELOAD shim that intercepts Squirrel scripting calls inside `m2engage` to enable runtime path overrides, SRAM splicing, and the lineup-switch UX. |
| `mod-assets/` | Pre-built artefacts the editor and the installer pull in at publish/install time: the cross-compiled hook `.so`, patched `.nut.m` scripts, and human-readable `.nut` sources. |
| [`test-environment/`](./test-environment/README.md) | Run the original ARM32 M2 engine and Chronos folder packs in an isolated Linux desktop VM; includes a Parallels launcher for Apple Silicon Macs. |
| `installer/` | *(future)* The console-side install tool that backs up stock NAND, deploys `console-mod/` to the right paths, edits `inittab`, and prepares the USB stick's `BACKUP/` directory. |

## High level flow

1. **One-time on the console:** install the files documented in [console-mod](console-mod/README.md), preserving the original `gameapp`. The automated installer is still planned.
2. **On the Mac (or any computer):** open the editor, build a custom library (add games, ROMs, covers), click Publish. The editor writes the library under `library/published/` and installs its bundled Chronos scripts and hook under `game/`.
3. **Use:** plug the stick into the console.
   - **Cold boot with stick:** `/bin/mount-usb-drives` mounts the stick, binds `/mnt/usb/game/` over `/usr/game/`, then binds `library/published/roms/` over `/usr/game/system/roms/`. The engine boots into your custom lineup.
   - **Cold boot without stick:** stock JP lineup, console behaves identically to factory.
   - **Hot-plug while running:** udev fires `pce-usb-attach`, kills the engine, applies the bind, restarts the engine with your lineup.
   - **Hot-unplug:** udev fires `pce-usb-detach`, kills the engine, drops the bind, restores ext4 mount on `/usr/game/`, restarts on stock.
   - **Power switch off:** engine exits, `gameapp` calls `/usr/bin/shutdown-detection`, the "sleeping PC Engine" image displays, kernel powers off.

The console NEVER has its NAND modified at the content level — only init scripts and a single `gameapp` replacement. Pulling the stick reverts everything to factory behaviour.

## Publishing the USB layout

### Create a library from a backup

At launch, choose **New library from a console dump…**, **Open an existing
library…**, or continue with the last library. New library is also available
in Settings. The creation assistant accepts:

- A full, raw NAND/eMMC image with the console's MBR/EBR partition table.
- A raw P9 image, or a folder containing separate partitions (one
  `mmcblk0p9.bin`, `p9.bin`, or `partition9.bin`). Only P9 is needed.
- An extracted P9/game directory containing `m2engage`, `libopus.so.0`,
  `version`, `shutdown.png`, and either `alldata.bin` + `alldata.psb.m` or
  the extracted menu resources.

Select a destination parent and a **new** folder name, then choose an empty
library or the original games. Both options recover the original menu
resources, the four publishing templates, and both CD BIOS images when
available. The original-games option also imports the ROMs, covers (including
composite and wide covers), metadata and sort orders, then publishes them.
The empty option waits for you to add games and click Publish.

The created folder contains `game/` and `library/`: copy both to the USB root.
Source files remain read-only. Filesystem and archive extraction run inside
the editor, without mounting the dump, a Linux VM, Python or external commands.
Work is staged next to the destination; failures remove incomplete output and
never replace an existing library. The filesystem reader supports the console's
ext2/ext4 P9 images; ZIP archives must be unpacked first. Validation currently
covers the original JP 1006JP dump (58 games); other firmware variants have not
been validated. New libraries start with fresh saves; creation does not import P8.

### Import a CD game and configure BIOS

In **Settings → CD conversion**, use **Extract BIOS from an original PCD…**
to recover its Super System Card (`bios`) and System Card (`bio2`) sections.
Alternatively select both existing 256 KiB BIOS files and click **Use selected
BIOS files**. They are checked for size/nonempty content and copied to the
application's data directory, so removing the source drive does not break
conversion. This is not a firmware identity/authenticity check. No BIOS or
commercial game data is bundled with the application.

In a game's ROM field, choose:

- **`.cue`**: the editor reads all referenced tracks and converts the disc into
  one `.pcd` in that game's directory, then updates and saves the ROM field.
  Keep all BIN/WAV tracks at the paths described by the CUE. Both configured
  BIOS images are required. Conversion is single-disc and uses the
  [pce-pcd-rust](https://github.com/Zorglub51/pce-pcd-rust) library, pinned by
  revision; CD audio is encoded with lossy Opus compression.
- **`.pcd`**: the archive structure is checked and the file copied byte for byte;
  no separate BIOS setup or re-encoding is needed.

The editor displays conversion stages and elapsed time, or byte progress for
copies. Errors identify missing tracks, invalid archives or BIOS files, and
failed writes. Work runs off the UI thread; editing is paused until it finishes.
An existing ROM is never overwritten: an unused numbered filename is chosen.
Selecting a CD for a HuCard entry switches it to Super CD-ROM2; an existing
CD-ROM2/Super CD-ROM2/Arcade CD-ROM2 selection is retained. Adjust Platform
when the game needs another System Card. Individual CD BIN tracks are not
accepted as complete games.

Settings are stored in the standard application configuration directory
(`~/Library/Application Support/com.pce.game-editor/` on macOS); settings from
older app bundles are migrated on the first launch. The conversion and dump
reader are part of the application, not separately installed utilities.

Use the canonical layout below (select the USB root in the editor):

```text
USB/
  game/                         Original native M2 engine and extracted resources
    system/script/*.nut.m       Four Chronos scripts installed by Publish
    system/roms/                Empty mount point; no published ROM copies
    lib/m2hook_print.so         Installed by Publish
    save/                       Empty mount point for published saves
  library/
    jp/                         Editable games and folders
    us/
    templates/                  Four stock PSB templates
    published/
      roms/                     One published ROM set shared by all folders
      folders/                  Generated menu/cover packs and their saves
      save/                     Live settings/SRAM, mounted at game/save
```

Publish first saves pending edits, generates the library, and installs the four
packed scripts (`const`, `mode_demo`, `mode_title_select`, `utils`) and the ARM
hook. These assets are embedded in the editor; the source checkout is not needed
when using the built application. It creates `game/system/roms/` as a normal
directory, so the layout works on FAT32 without symbolic links. Republishing
updates the Chronos assets but does not remove existing files from `game/`,
including legacy ROM copies or live saves.

New keys also use an empty `game/save/` mount point: the launcher mounts
`library/published/save/` there so the editor and engine share the same files.
For old keys, existing live saves must be migrated explicitly; the launcher
refuses to hide a nonempty `game/save/`.

The original JP 1006JP `m2engage`, `libopus.so.0`, `version`, `shutdown.png`, and
the extracted original `system/` and `040/` resources are supplied by the new
library assistant, or must already be present in `game/`. Publishing itself
does not extract `alldata.bin`, copy the original engine,
install the console mod, or migrate live saves from another layout. The editor
reports missing original files instead of claiming that an incomplete key is
ready. That presence check is not a complete runtime compatibility check.

**Update the console scripts too:** install the current `mount-usb-drives`,
`pce-usb-lib.sh`, `pce-usb-attach`, `pce-usb-detach`, and `S11chronos-usb`. Remove
the old USB line from `inittab` if present: USB mounting must follow stock rcS's
read-only remount, before the engine starts. An older launcher cannot
use an empty `game/system/roms/`. The updated launcher checks for published ROMs,
mounts them after `game/`, and removes child mounts before the parent on detach.
Hot-plug now stops the old engine before replacing its mounts; removal still
uses a forced stop and is not a safe-eject guarantee for pending saves.

Standalone exports outside `<USB>/library/published/` retain their export-only
behaviour and do not create a sibling `game/` directory.

Publication also compacts the stock RGBA8 texture atlases in each generated
cover PSB. It keeps every sprite referenced by the retained animations,
including SuperGrafx labels and auxiliary card graphics, with its original
pixels and filtering border. Unreferenced stock atlases are removed; unknown
formats or references are left unchanged. This reduces archive size and texture
memory without a runtime cache or additional menu-script changes. Existing
exports benefit after republishing with a rebuilt editor.

Opening a library imports its console save data into the per-game folders,
including newly created save states still in the active save directory.
The **Save Data** section shows the four native save slots with their
thumbnails, plus a separate SRAM file indicator. A present SRAM file does not
necessarily contain in-game progress. Legacy Mac JSON save previews remain
supported. Save imports preserve folder-card positions and the JP/US slot
offsets so a save stays associated with its game.

## Build the editor locally

Requires Rust, Node.js and a C/C++ toolchain (Xcode Command Line Tools on macOS).
The CD converter links libopus and liblz4; CMake is needed when building the
bundled Opus library. The initial Cargo build fetches the pinned converter
source. The dump reader uses [ext4-view](https://docs.rs/ext4-view/0.9.3/ext4_view/)
in read-only mode.

```sh
cd editor
npm install
npm run build
```

Output: `editor/target/release/bundle/macos/PCE Game Editor.app` on macOS, or `editor/target/release/pce-game-editor.exe` on Windows. CI builds for both via [GitHub Actions](./.github/workflows/build.yml).

Checks for USB publication and mount lifecycle (no console required):

```sh
cd editor
cargo test -p m2-publish --lib
cargo test -p pce-game-editor --lib
cargo test -p m2-import
node --test tests/*.test.cjs
cd ..
python3 -m unittest discover -s console-mod/tests -v
```

## Wiki

To test native M2 folder navigation on a Mac without modifying a console, see
the [Linux VM test environment](./test-environment/README.md). It reuses the
console hook and published `.psb.m` packs, with private test saves.

The settings-return fix after folder/lineup changes requires both the updated
`m2hook_print.so` and `mode_title_select.nut.m`. Republish with these assets while
keeping existing saves; see the [diagnosis and checks](./test-environment/README.md#black-screen-when-returning-from-settings-after-a-pack-change).

For end-user documentation (how the mod works in detail, install/uninstall procedures, troubleshooting), see the [Project Chronos Engine wiki](../../wiki).

## License

[GPLv3](./LICENSE) — see LICENSE file for full text.

## Related projects

- [Project-Chronos-Editor](https://github.com/Zorglub51/Project-Chronos-Editor) — earlier editor iteration (archived)
- [Project-Chronos](https://github.com/Zorglub51/Project-Chronos) — companion docs / planning
- [sntool](https://github.com/Zorglub51/sntool) (or community fork) — D-lang toolchain that the m2hook design borrows from
