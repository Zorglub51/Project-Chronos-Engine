# Changelog

## 2.0.0-alpha.2 (2026-10-25)

- Bind the editor's Japanese title field and game heading to the native Japanese
  `tname`, preserving the separate internal `name`. Use each lineup's original
  `tname` from items 0–99 for Japanese and 100–199 for other menu languages,
  including differences in capitalization and spelling between the two views.

Windows-only update: Windows x64 portable ZIP and NSIS installer. The macOS,
Linux AppImage and Arch Linux packages remain available in 2.0.0-alpha.1.

## 2.0.0-alpha.1 (2026-10-17)

- Read the editor's JSON library directly in the native console runtime, with
  asynchronous current-folder snapshots and remembered lineup/folder selections.
  Source libraries no longer require Publish or generated folder PSB packs.
- Prepare the USB hook and eight script overrides automatically when creating
  or opening a complete library. P7 remains the mount/startup bootstrap; runtime
  code and assets come from the key. Retain legacy archives during migration.
- Prepare revisioned individual cover Image PSBs after cover edits and fixed
  JP/US motion templates without the original cover atlas. Bound the live pool
  to seven large covers and at most fifty 48×48 active-folder thumbnails.
- Prime large covers once for the final sorted selection, batch thumbnail
  loading, and prefetch neighbouring covers without waiting in the carousel.
  Keep seven resident large covers plus at most one pending replacement.
  Continue character movement during folder artwork loading and fades while
  preserving the original motion layouts and launch animations.
- Bind the final cover window before constructing native carousel motions.
  Prevent the leftmost cover from retaining the transparent placeholder when
  opening a menu or returning from a folder, without an extra cover preload.
- Preserve original SGX sleeve strips/geometry, use center-only inner cases,
  thumbnails and labels, and provide a neutral notched sleeve for custom SGX
  artwork lacking strip inputs. Correct native bitmap-thumbnail attributes.
- Maintain console bitmap fonts automatically as titles, folders and genre
  names change, with a glyph-set cache and serialized generation.
- Move games and update both source/destination lists before refreshing console
  fonts. Preserve sort orders, unknown metadata and per-game saves; roll back
  failed writes and report font-refresh failures separately from a successful
  move. Block concurrent editing while a move is in progress.
- Hide Publish until a legacy library is explicitly identified, and refuse
  legacy publication over a V2 library's live storage before changing files.
  V2 changes remain available directly to the console without publication.
- Recover missing game identities from verified per-game save-owner markers.
  Reject conflicting or malformed ownership without changing identities or saves.
- Create new libraries directly in the selected USB drive or destination folder,
  with `game/` and `library/` at that level. Do not add a `Chronos/` wrapper that
  the console launcher cannot find. Preserve unrelated files and refuse to
  replace an existing library.
- Serialize cover imports/mosaics and save their PNGs atomically before console
  asset preparation. Packaging/size edits refresh only the affected cover;
  failed preparation is reported and retried without losing the previous asset.
- Add explicit empty/small-menu handling and a private unowned bootstrap ROM,
  while retaining the native 50-entry menu limit and guarding navigation cards
  from game launch or save ownership.
- Store each game's SRAM, four fixed save-state slots and slot locks inside its
  game directory. Stable identities preserve these files across reordering,
  folder moves and JP/US lineup changes.
- Route native state reads, writes, previews and deletion directly to that
  storage. Use one common SRAM working object with verified ownership and
  immediate persistence after native autosaves.
- Validate and migrate old published saves using their last published game
  mapping, with recoverable retagging and the corrected SRAM offset. Report
  ambiguous or incompatible files visibly without discarding the originals.
- Preserve empty save slots in the editor's four-slot preview. Stable identity
  and save paths come directly from the source catalogue; legacy publication
  remains available for independent editing trees.
- Add a home-screen console partition manager, with verified individual,
  selected-partition and full-MMC backups and raw/ZIP restoration.
- Stage restore images with bounded memory, validate their full size/CRC and
  checksum, and require confirmation of the selected partition and image.
- Reuse the recovery transport, with RAM-recovery and mounted-device checks,
  single-operation locking and transfer progress. Recovery startup currently
  uses the existing recovery utility.

Packages: Windows x64 portable ZIP with installer, universal macOS application
(Apple Silicon and Intel, macOS 11+), Linux x86_64 AppImage and native Arch Linux
x86_64 package. Each download includes or is accompanied by source provenance
and SHA-256 verification.

Validation: source menus with 0, 1, 2, 3, 4 and 10 entries, JP-folder/US-root
navigation, direct JSON title updates and SGX launch/four state slots pass with
the original ARM32 engine in the isolated Linux VM. Independent save tests cover
native HuCard/CD/SGX pairs and a JP-to-US move. Physical A33 testing confirms
22-entry root navigation, RPG/Shooters folder changes, smoother scrolling and
the corrected partly visible left cover. No measured A33 frame-time gain or
long-session memory plateau is claimed. Complete source navigation,
glyph/extended-transition tests and hardware memory validation remain ongoing.
This is an editor prerelease with its bundled USB runtime; no new P6/P7
firmware images are included.

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
