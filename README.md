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

1. **One-time on the console:** run the installer. It backs up the stock `/usr/game/` tree and `/etc/init.d/gameapp` to a `BACKUP/` directory on a USB stick, then deploys `console-mod/` files to `/bin`, `/etc/init.d`, `/usr/bin`, `/etc/udev/rules.d`, and adds a line to `/etc/inittab`.
2. **On the Mac (or any computer):** open the editor, build a custom library (add games, ROMs, covers), click Publish. The editor writes the library tree under `library/published/` on the USB.
3. **Use:** plug the stick into the console.
   - **Cold boot with stick:** `/bin/mount-usb-drives` mounts the stick, bind-mounts `/mnt/usb/game/` over `/usr/game/`, engine boots into your custom lineup.
   - **Cold boot without stick:** stock JP lineup, console behaves identically to factory.
   - **Hot-plug while running:** udev fires `pce-usb-attach`, kills the engine, applies the bind, restarts the engine with your lineup.
   - **Hot-unplug:** udev fires `pce-usb-detach`, kills the engine, drops the bind, restores ext4 mount on `/usr/game/`, restarts on stock.
   - **Power switch off:** engine exits, `gameapp` calls `/usr/bin/shutdown-detection`, the "sleeping PC Engine" image displays, kernel powers off.

The console NEVER has its NAND modified at the content level — only init scripts and a single `gameapp` replacement. Pulling the stick reverts everything to factory behaviour.

## Build the editor locally

```sh
cd editor
npm install
npm run build
```

Output: `editor/src-tauri/target/release/bundle/macos/PCE Game Editor.app` on macOS, or `editor/src-tauri/target/release/pce-game-editor.exe` on Windows. CI builds for both via [GitHub Actions](./.github/workflows/build.yml).

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
