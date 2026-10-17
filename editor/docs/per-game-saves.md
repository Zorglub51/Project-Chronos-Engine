# Game identities and save storage

Each game keeps its own save files in its editable library directory:

```text
library/jp/[folder/]GAME001/
  game.json
  sram.bin
  saves/
    state_0.bin
    state_0_meta.bin
    state_1.bin
    state_1_meta.bin
    state_2.bin
    state_2_meta.bin
    state_3.bin
    state_3_meta.bin
    locks.bin                 # optional four-slot lock mask
    .chronos-game-id           # guards the save directory's owner
    .chronos-sram-schema       # "2\n" once SRAM alignment is verified
```

The four native slots are numbered `0` through `3` on disk and displayed as
**Slot 1** through **Slot 4**. Empty slots remain in place: saves in slots 2 and
4 never become slots 1 and 2. SRAM is the game's in-game memory, independently
of these emulator save states.

Each canonical `sram.bin` is one complete **8,448-byte** native SRAM block.
The native aggregate in `data_008_0000.bin` starts its SRAM array at **0x5e84**;
the old 0x5e80 offset included four bytes before the array. Migration recovers
those complete blocks from a validated original bank, retains an existing
misaligned file as `sram.bin.pre-chronos-v2`, and writes
`saves/.chronos-sram-schema` with `2` followed by a newline. Once marked, the
per-game file is authoritative and old published banks cannot replace it.
Empty, previously absent SRAM blocks do not allocate a file.

The lock mask in `saves/locks.bin` is a single byte with bits 0–3 corresponding
to displayed slots 1–4. Locks belong to the game, not its position in a menu.

`game.json` stores a permanent `game_id` (32 lowercase hexadecimal characters)
and a permanent `native_region_tag`, which fits the native engine's 14-byte
identifier limit. New primary tags use `G` plus 11 hexadecimal characters,
leaving room for native alternate-region suffixes. The editor retains all other
profile fields, including an existing `rom.regionTag`.

Opening an older library adds missing identities atomically. Existing safe
native save tags are retained. The migration validates identities and native
tag collisions before writing metadata. Ambiguous save tags, duplicate IDs,
or old tags that cannot be migrated safely produce an error and preserve the
existing save files.

Legacy save migration uses the **last published catalogue**, including
`game_id` when available, rather than the current edited menu order. Native
state layouts, headers and metadata digests are verified before importing or
retagging a state. Original state/metadata pairs are retained during explicit
retagging, with a journal allowing an interrupted pair replacement to recover.
Files with ambiguous ownership are retained and stop opening the library with
a visible error. Other unmatched files remain in the original archives and
are reported after opening; they are never presented as successfully imported.

Changing the title, ROM, ordering, folder or lineup does not change a game's
identity. Moving a game moves the entire directory, including SRAM, save states
and metadata. When a destination already has the same directory name, the
editor adds a directory suffix while retaining the identities and save slots.
A conflicting native tag in the destination requires an explicit migration;
the editor does not silently substitute another game or overwrite its saves.

Back up the complete game directories together with `game.json`; there is no
separate central save store. Runtime save routing uses these canonical files,
while native positional files are compatibility views. Retain the hidden
schema and migration files when backing up a library.

## Source library workflow

Creating a library from a console dump prepares the USB runtime for both
choices in the assistant: an empty library or the original games. Opening a
complete older USB library also prepares it and checks for runtime updates.
The editor installs its bundled hook and menu scripts in the sibling `game/`
directory, prepares console image assets, and maintains the bitmap fonts as
game titles, folder names and genre names change. An incomplete console
runtime is reported before the library is enabled.

The console reads `game.json` and `gamelist.json` directly. Changes are saved
automatically; a source library has no Publish step and does not create folder
PSB packs. `library/.chronos-library-v2` contains `2` followed by a newline and
is written only after preparation succeeds. The existing console bootstrap
still uses `library/published/roms/` as the shared ROM store and
`library/published/save/` for the native settings working bank. These folders
are compatibility storage, not a second copy of the editable library or its
per-game saves.

`published/.per-game-saves-v2`, also containing `2` followed by a newline,
prevents old positional save archives from being imported over newer per-game
saves. The first upgrade imports legacy saves using the last published
catalogue and retains the original archives for recovery. Older editing trees
without a sibling console runtime remain usable through the legacy export
workflow; they are not silently turned into USB libraries.

The native menu still supports **50 entries per lineup root** and **49 games
plus Back per folder**. The editor warns about larger menus while keeping all
metadata editable and saved, so games can be reorganized without losing work.
The console refuses an oversized folder and retains the previous menu.

## Validation

The isolated ARM Linux VM runs the original M2 engine with its existing VM
platform patch. Native tests created four HuCard slots, a CD slot and a
SuperGrafx slot, verified their headers and full data/metadata digests, and
confirmed the four HuCard previews plus locks on slots 2 and 4 after moving
that game from JP to US. All four state payloads remained byte-identical.
Dracula X produced a native `STATRAM0` block; after relaunch the native SRAM
API reported a valid image and successful application.

The console hook is compiled for ARM32 with the existing GLIBC 2.4/2.7/2.17
requirements. Automated validation covers publisher/migration, editor backend,
frontend, portable save-runtime and native-harness/digest/slice/layout tests.
Linux libc routing
also completed 1,000 open/stat cycles without a file-descriptor increase.
Physical A33 validation remains necessary before distributing this adapter
as a console release. This does not claim a long-session hardware soak test.
