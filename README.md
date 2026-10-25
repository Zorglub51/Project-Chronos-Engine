# Project Chronos Engine

A modding toolkit for the PC Engine Mini / TurboGrafx-16 Mini console that lets you ship a custom game library on a USB stick — drop the stick in, the console boots into your library; pull the stick out, you're back to factory stock.

The development tree now uses a **source-driven library**: the editor saves
JSON metadata and prepares individual native images automatically, without a
Publish step. Native menus and folder navigation have been validated in the
isolated Linux VM and on the physical A33 console. Extended navigation and
long-session validation are ongoing. The
[2.0.0-alpha.1 editor prerelease](https://github.com/Zorglub51/Project-Chronos-Engine/releases/tag/v2.0.0-alpha.1)
includes the V2 USB runtime and requires a compatible Chronos console bootstrap.
See [the implementation and validation status](docs/V2-runtime-library.md).

Windows users can download the
[2.0.0-alpha.3 deletion and title-editing fixes](https://github.com/Zorglub51/Project-Chronos-Engine/releases/tag/v2.0.0-alpha.3).
The macOS, Linux AppImage and Arch Linux downloads remain in the 2.0.0-alpha.1 release.

## What's in this repo

| Directory | Role |
|---|---|
| `editor/` | Tauri + Rust GUI app to edit game libraries and manage console MMC backups and partition restores. |
| `console-mod/` | The shell scripts + udev rule + `gameapp` replacement that get installed on the console once. Provides USB detection, hot-plug, bind-mount swap, and the conditional `LD_PRELOAD` of the hook. |
| `console-mod/hook-src/` | C source for `m2hook_print.so` — the LD_PRELOAD shim that intercepts Squirrel scripting calls inside `m2engage` to enable runtime path overrides, SRAM splicing, and the lineup-switch UX. |
| `mod-assets/` | Bundled ARM hook, eight packed script overrides and their readable sources; installed on the USB key when the editor creates or opens a complete library. |
| [`test-environment/`](./test-environment/README.md) | Run the original ARM32 M2 engine in an isolated Linux VM, with source-library and legacy-pack test fixtures; includes a Parallels launcher for Apple Silicon Macs. |
| `installer/` | *(future)* Automated installation of the console bootstrap, with preservation of original partitions and startup files. |

## High level flow

The home screen also offers **Console backups and partitions…**. It supports
verified selected-partition or full-MMC backups and raw/ZIP restoration with
explicit destination confirmation. Start RAM recovery and connect its USB
network using the existing recovery utility first; the transfer code is linked
directly into the editor. See [console partition management](editor/docs/console-partitions.md).

1. **One-time on the console:** install the files documented in [console-mod](console-mod/README.md), preserving the original `gameapp`. The automated installer is still planned.
2. **On your computer:** create a library from a dump or open a complete USB library. The editor installs its bundled runtime under `game/`. Add games, ROMs and covers; metadata and individual image resources are saved automatically, without publishing folder packs.
3. **Use:** plug the stick into the console.
   - **Cold boot with stick:** `/bin/mount-usb-drives` mounts the stick, binds `/mnt/usb/game/` over `/usr/game/`, then binds `library/published/roms/` over `/usr/game/system/roms/`. The engine boots into your custom lineup.
   - **Cold boot without stick:** the original console application and its stock lineups run without the Chronos hook.
   - **Hot-plug while running:** udev fires `pce-usb-attach`, kills the engine, applies the bind, restarts the engine with your lineup.
   - **Hot-unplug:** udev fires `pce-usb-detach`, kills the engine, drops the bind, restores ext4 mount on `/usr/game/`, restarts on stock.
   - **Power switch off:** engine exits, `gameapp` calls `/usr/bin/shutdown-detection`, the "sleeping PC Engine" image displays, kernel powers off.

The original game partition remains intact. P7 supplies startup and mount
helpers; the hook and eight menu overrides come from the USB key. There is no
P7 runtime bundle overriding the key's editor-installed files.

## Preparing the USB library

Game details include an **Expert** section for per-game emulation options,
screen framing and audio gain. The **i** buttons explain effects and possible
values in English. See the [per-game parameter reference](docs/game-parameters.md)
for the original engine behavior and the settings managed by Chronos.

Use **Delete this game** to remove a game and its directory, including its save
data. The shared ROM is removed only when no other game uses it. Remaining
`GAME` directory names stay stable, so gaps in their numbering are expected.
`library/published/roms` is the live shared ROM store in V2 and must be kept.

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

Select the **USB drive itself** or a dedicated destination folder, then choose
an empty library or the original games. Both options recover the original menu
resources, the original PSB/font templates, and both CD BIOS images when
available. The original-games option also imports the ROMs, covers (including
composite and wide covers), metadata and sort orders. Both choices prepare the
source runtime immediately; the empty library needs no export after adding games.

The assistant creates `game/` and `library/` directly in the selected destination.
For a console key, these must be at the USB root, without an enclosing `Chronos/`
folder. The console launcher and hook use this layout.
Source files remain read-only. Filesystem and archive extraction run inside
the editor, without mounting the dump, a Linux VM, Python or external commands.
Work is staged on the destination filesystem before installation. Existing
`game/`, `library/` or `source.json` entries are never replaced; unrelated files
are retained. The filesystem reader supports the console's
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
  one `.pcd` in the shared ROM store, then updates and saves the ROM field.
  Keep all BIN/WAV tracks at the paths described by the CUE. Both configured
  BIOS images are required. Conversion is single-disc and uses the
  [pce-pcd-rust](https://github.com/Zorglub51/pce-pcd-rust) library, pinned by
  revision; CD audio is encoded with lossy Opus compression.
- **`.pcd`**: the archive structure is checked and the file copied byte for byte;
  no separate BIOS setup or re-encoding is needed.

CD conversions include the 2026-09-20 audio fix: a continuous Opus encoder
preserves sound across one-second PCD chunks, silence and the final audio tail,
and filtered resampling preserves treble. Reconvert CUE/BIN sources imported
with older editor builds if their CD audio clicks or distorts. Importing an
already converted `.pcd` copies its audio unchanged and cannot repair it.

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
    system/script/*.nut.m       Eight overrides installed on creation/open
    system/motion/              Fixed JP/US cover templates and original UI
    system/font/                Automatically prepared console bitmap fonts
    system/roms/                Empty mount point; no ROM copies here
    lib/m2hook_print.so         Installed on creation/open
    save/                       Empty native-working-save mount point
  library/
    .chronos-library-v2         Source-runtime marker
    .current-v2                Last successfully loaded lineup/folder
    jp/                         Editable games and folders
    us/
    templates/                  Preserved original PSBs/fonts
    runtime/back/               Shared navigation artwork
    published/
      roms/                     One ROM store shared by all games
      save/                     Native settings/SRAM working bank
```

The editor prepares and updates the ARM hook and packed `const`, `mode_demo`,
`mode_title_select`, `utils`, `emulator`, `systemdata_saveload`, `pause_main`
and `backup` scripts when creating/opening a complete library. These assets are
embedded in the application; the source checkout is not needed. Source mode is
enabled only after resource/save preparation succeeds. Old folder exports are
retained for recovery, but the console reads the editable JSON directly.

Each game/folder's `runtime/assets.json` selects revisioned Image PSBs prepared
after cover edits. Fixed native motion templates replace the original cover
atlas. The console holds seven large covers and up to fifty 48×48 thumbnails
for its current folder. Packaging and dimension edits refresh only the affected
entry, with errors kept visible and failed image preparation retried.
The console does not decode PNGs or build PSBs while navigating.
Fonts are maintained automatically when titles, folders or genres need new
glyphs. These bounds describe asset ownership, not a measured A33 memory total.

New keys also use an empty `game/save/` mount point: the launcher mounts
`library/published/save/` there so the editor and engine share the same files.
For old keys, existing live saves must be migrated explicitly; the launcher
refuses to hide a nonempty `game/save/`.

The original `m2engage`, `libopus.so.0`, `version`, `shutdown.png`, `system/`
and model-specific `040/` or `041/` resources come from the creation assistant
or an existing complete `game/`. They are not bundled commercial content.
Opening checks missing/mismatched resources before enabling the runtime; this
does not certify every firmware revision. Console bootstrap installation is
separate from library preparation.

**Update the console scripts too:** install the current `mount-usb-drives`,
`pce-usb-lib.sh`, `pce-usb-attach`, `pce-usb-detach`, and `S11chronos-usb`. Remove
the old USB line from `inittab` if present: USB mounting must follow stock rcS's
read-only remount, before the engine starts. An older launcher cannot
use an empty `game/system/roms/`. The updated launcher checks for published ROMs,
mounts them after `game/`, and removes child mounts before the parent on detach.
Hot-plug now stops the old engine before replacing its mounts; removal still
uses a forced stop and is not a safe-eject guarantee for pending saves.

Bare editing trees and standalone exports retain the legacy export workflow;
they do not silently create a sibling console runtime. Legacy publication still
generates folder PSBs and compacts eligible stock atlases. Source libraries
do not need that publication step.

Opening an older library validates/migrates saves using its last published
catalogue rather than its current edited order. Source libraries store SRAM
and four permanent state/metadata pairs inside each game's own directory.
The **Save Data** section shows the four native save slots with their
thumbnails, plus a separate SRAM file indicator. A present SRAM file does not
necessarily contain in-game progress. Legacy Mac JSON save previews remain
supported. Stable `game_id` and native tags preserve ownership through
reordering, folder moves and JP/US moves. Slots stay numbered 1–4 even when
some are empty, and their protection bits move with the game. See
[per-game saves](editor/docs/per-game-saves.md).

## Build the editor locally

Requires Rust, Node.js and a C/C++ toolchain (Xcode Command Line Tools on macOS).
The CD converter links libopus and liblz4; CMake is needed when building the
bundled Opus library. The initial Cargo build fetches the pinned converter
source and requires read access to the private `pce-pcd-rust` repository.
The dump reader uses [ext4-view](https://docs.rs/ext4-view/0.9.3/ext4_view/)
in read-only mode.

```sh
cd editor
npm ci
npm run build
```

Output: `editor/target/release/bundle/macos/PCE Game Editor.app` on macOS.
Windows builds also produce an NSIS `*-setup.exe` installer in
`editor/target/release/bundle/nsis/`, alongside `editor/target/release/pce-game-editor.exe`.
See [Windows installation and builds](docs/WINDOWS.md).

The public [GitHub Actions workflow](./.github/workflows/build.yml) tests the
frontend. Native Windows/macOS builds and Rust tests run in **Build Chronos
editor** in the private converter repository, preserving its confidentiality.
That workflow accepts an editor commit/tag/branch and produces downloadable
artifacts. The previous public release workflow has been removed: public tags
do not automatically publish installers. An owner can publish selected,
validated binaries separately after downloading the private build artifacts.

Checks for source preparation, legacy publication and mount lifecycle
(no console required):

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
console hook, with isolated source-library or legacy `.psb.m` fixtures and
private test saves.

Hook and script updates must travel together. Opening a complete source library
installs the bundled set; legacy exports require republishing. For older pack
diagnostics, see the [settings-return checks](./test-environment/README.md#black-screen-when-returning-from-settings-after-a-pack-change).

For end-user documentation (how the mod works in detail, install/uninstall procedures, troubleshooting), see the [Project Chronos Engine wiki](../../wiki).

## License

[GPLv3](./LICENSE) — see LICENSE file for full text.

## Related projects

- [Project-Chronos-Editor](https://github.com/Zorglub51/Project-Chronos-Editor) — earlier editor iteration (archived)
- [Project-Chronos](https://github.com/Zorglub51/Project-Chronos) — companion docs / planning
- [sntool](https://github.com/Zorglub51/sntool) (or community fork) — D-lang toolchain that the m2hook design borrows from
