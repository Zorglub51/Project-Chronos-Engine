# Packed HuCards and titlebars

The ROM picker accepts `.pce.m` HuCards, including original names such as
`Neutopia_II_J.PCE.m`. Import copies the archive byte for byte, with its original
name, without decompression, conversion or BIOS configuration. An identical
archive already present in the shared ROM pool is reused. A different archive
with the same name produces an error: MZS encryption depends on the filename,
so the importer must not silently rename it. Other `.m` formats are rejected.

Imports store these archives unchanged in `library/published/roms/`. Raw `.pce`
and `.sgx` ROMs are packed once during import. Publication reuses these files. A raw and a packed ROM targeting the same output name
cannot silently overwrite one another. The native profile omits the final `.m`
because m2engage appends that suffix itself.

The first titlebar choice, **White bar**, stores the native value `0`, as used
by the original JP Neutopia II (`GAME043`). It survives saving, moving between
folders/lineups, and publication to `title_mode_top.psb.m`. All 13 styles are
available in both JP and US lineups, regardless of the game's platform. Moving
a game or changing its platform preserves the chosen style. The former setting
that forced US banners is removed; old settings files no longer enable it.

The console title preview keeps the previous image visible while rendering and
decoding its replacement. Typing is debounced, obsolete results are ignored, and
the preview and error row retain their height when changing language or text.

## Replacing a ROM

Importing a replacement ROM saves the new selection before scheduling the old
shared file for cleanup. Cleanup preserves files referenced by another game,
a BIOS field, or any existing published menu profile. If a published profile
still uses the old ROM, cleanup waits until a successful publication replaces
that profile. Unrelated files are never swept.

Failed imports or saves keep the old ROM. Identical imports reuse the existing
file. Cleanup failures leave a retryable queue in `.rom-cleanup.json`.

See [shared ROM storage](shared-rom-storage.md) for migration and compatibility.
