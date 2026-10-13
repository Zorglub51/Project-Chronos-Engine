# Changelog

## Unreleased

- Add a home-screen console partition manager, with verified individual,
  selected-partition and full-MMC backups and raw/ZIP restoration.
- Stage restore images with bounded memory, validate their full size/CRC and
  checksum, and require confirmation of the selected partition and image.
- Reuse the recovery transport, with RAM-recovery and mounted-device checks,
  single-operation locking and transfer progress. Recovery startup currently
  uses the existing recovery utility.

## 1.0 alpha — editor revision 6 (2026-10-12)

### Fixed

- Persist each game's genre during automatic library saves. Previously the
  backend discarded this field, so reopening the library lost genre assignments.
- Preserve genre assignments and the associated saved sort indices without
  requiring an export, including JP/US lineups and nested game folders.
- Keep older game metadata without a genre compatible; clearing a genre also
  survives reopening.

### Validation and packages

- Added regression tests for save/reopen without publication and legacy metadata.
- Windows x64 portable ZIP, macOS universal app (Apple Silicon and Intel),
  Linux x86_64 AppImage, and Arch Linux x86_64 package revision 6.
- No console firmware update is required. This fix does not reconstruct genre
  assignments already discarded by an older editor.

Earlier release notes are available on the
[GitHub release page](https://github.com/Zorglub51/Project-Chronos-Engine/releases/tag/v1.0.0-alpha).
