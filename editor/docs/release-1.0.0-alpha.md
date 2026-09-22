# Project Chronos Engine 1.0 alpha

The first public prerelease combining the Chronos firmware and the PC Engine
Mini game library editor.

## Downloads

| File | Contents |
|---|---|
| `chronos-p6.zip` | P6 partition: custom Linux 3.4.113 kernel. |
| `chronos-p7.zip` | P7 partition: Chronos system, USB startup, and SSH/SFTP access. |
| `PCE-Game-Editor-1.0.0-alpha-windows-x64.zip` | Windows x64 editor: executable and installer. |
| `PCE-Game-Editor-1.0.0-alpha-macos-arm64.zip` | `PCE Game Editor.app` for Apple Silicon Macs. |
| `SHA256SUMS.txt` | Checksums for verifying the downloads. |
| `manifest.json` | Firmware provenance, source commits, and application validation results. |

The P6/P7 images are unchanged copies of a fresh dump from the working console,
captured on September 21, 2026. The kernel configuration and detailed
instructions are included in the firmware archives.

## Console installation

1. Back up the console partitions with PCE Mini Recovery.
2. Put the console into recovery mode, restore `chronos-p6.zip` to **P6**
   and `chronos-p7.zip` to **P7**, and let verification finish.
3. Keep **P8 and P9**: these partitions are not included in this release.
4. Prepare a FAT32 USB stick with the editor. Place the published `game/` and
   `library/` folders at its root, then restart the console with the stick inserted.

The firmware has been validated on PC Engine Mini JP / Allwinner A33. Other
console variants have not yet been validated. SSH and SFTP are enabled at
**169.254.13.37:22**, with username **root** and an empty password. Each console's
SSH host keys are generated in P8; no private SSH keys are included in the images.

## Editor

- Create an empty library or import the original games from a full dump,
  separate partitions, or an extracted P9 directory.
- Organize games into JP/US lineups and folders, edit covers, metadata, and
  styles, then publish the Chronos USB stick.
- Import `.pce`, `.sgx`, and `.pce.m` HuCards. Packed `.pce.m` files are copied
  without decompression, preserving their names and contents.
- Import a `.pcd` directly or convert a `.cue` and its tracks to `.pcd`, with
  conversion progress and BIOS configuration in Settings.
- View game save states and SRAM.
- Choose **No titlebar**, including in the US lineup, as used by Neutopia II JP.

**Windows:** extract the ZIP and run `pce-game-editor.exe`, or use the included
installer. The standalone executable requires Microsoft WebView2; the installer
downloads it if needed.

**Apple Silicon Mac:** extract the ZIP and copy `PCE Game Editor.app` to
Applications. This archive contains only the ARM64 build.

The editors include no games or BIOS files. Creating a library uses the user's
own original console dump. The applications do not yet have a Windows publisher
certificate or Apple notarization.

This is an **alpha release**. Keep backups of the console and library before
testing. A Linux edition is not included.
