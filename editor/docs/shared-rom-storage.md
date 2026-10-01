# Shared ROM storage

`library/published/roms/` is now the authoritative ROM store, not a disposable
build cache. Keep this directory when backing up or moving a library. Game
folders contain metadata, covers and saves; their ROM filenames refer to the
shared store. Moving a game between folders or JP/US lineups does not move its ROM.

The console already mounts this exact directory onto `/usr/game/system/roms`.
No firmware, P6/P7 or launcher update is required. Menus retain the original
native ROM path convention.

## Import and publication

- CUE files are converted directly into a shared PCD; BIOS settings are unchanged.
- PCD files and packed HuCards are imported without conversion.
- Raw PCE/SGX HuCards are packed once during import, using the final archive name.
- An identical file with the same name is reused across games and lineups.
- Different raw HuCards or CDs with the same name receive a free filename.
  Packed HuCards cannot be silently renamed because their encryption depends on
  the name. Conflicting packed archives are rejected without replacing anything.
- Publication to the library's canonical `published/` directory generates menus
  and saves without copying ROMs. An explicitly separate export still copies ROMs
  so the exported library is usable independently.

## Existing libraries

Opening a library runs migration in a background operation with progress. The
publisher also supports migration for command-line clients. Legacy game-local
ROMs are packed/copied into the pool and verified before local copies are removed.
PCDs are compared with bounded buffers, avoiding loading a whole CD into RAM.
All game sources are checked before any duplicate is deleted; conflicting names
stop migration and preserve the originals. Retrying after an interruption reuses
verified files already in the pool. Game metadata and save files are unchanged.

Verified identical physical leftovers in the USB's `game/system/roms/` are also
removed. Unique or different files are retained. On Linux a bind-mounted alias
of the shared pool is detected by file identity and is never unlinked.

Existing game filenames remain valid: a reference such as `Game.pce` resolves to
`published/roms/Game.pce.m` after migration. New imports return the actual archive
filename. The ROM selector lists the shared store.

## Replacement and cleanup

After the replacement is saved, the old ROM becomes a cleanup candidate. It is
kept while any game or published menu profile still references it. Successful
publication retries cleanup. This prevents an unpublished edit from breaking the
console's current menu and preserves ROMs shared by several entries. Unmanaged
files, BIOS files and unrelated ROMs are not swept. Failed cleanup is retryable.

Older editor versions expect per-game ROM files; use an updated editor to edit or
republish a migrated library. The console itself remains compatible.
