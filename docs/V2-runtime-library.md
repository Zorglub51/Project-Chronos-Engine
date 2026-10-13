# V2: filesystem-driven runtime library

Status: proposed implementation contract. V1 remains the working console
runtime; this document does not claim that V2 is already implemented.

## Goal

The console reads the editable library directly when entering a lineup or
folder. A custom Squirrel `ChronosLibrary` class replaces PSB-backed library
configuration. The C hook fills its data from the filesystem. Editing or
reordering games no longer requires publishing folder packs, and navigation
must not rename or copy save-state files.

## Runtime data contract

- Read `gamelist.json` and each listed `game.json` in the requested folder.
  Their explicit order and sort values remain authoritative. Folder discovery
  does not replace the editor's ordering with filesystem enumeration order.
- Preserve the JP/US lineup, display names, genres, titlebar, emulator options,
  ROM source path and folder entries. ROM paths are resolved directly against
  the shared ROM store; moving a game does not move its ROM.
- Assign each game a persistent `game_id`, independent of folder name, lineup,
  carousel position or current sort. Preserve it when moving or editing a game.
  IDs must be unique and survive closing the editor.
- Keep separate `menu_index`, `image_index` and `game_id`. An array position is
  never a persistent save identity.
- Keep a separate stable ASCII `native_region_tag`, at most 14 bytes, because
  native save payloads embed and validate this tag. Preserve existing tags
  during migration; a full UUID cannot replace this native field.
- Use a new versioned library format and an explicit migration from V1. Existing
  published packs remain available until the V2 migration and console tests pass.

## Class and hook responsibilities

`ChronosLibrary` owns the current folder snapshot and exposes the configuration
shape the menu and emulator scripts require. The hook supplies indexed game
records through native closures; the Squirrel class builds the typed view.
This avoids depending immediately on the native Squirrel class ABI.

Use the existing asynchronous worker pattern: filesystem reads and JSON parsing
run off the menu thread, and all Squirrel VM calls run on the menu thread.
Requests have a generation number. A failed or stale load retains the previous
folder; a successful load commits the new snapshot atomically. Retain only the
current folder and a small navigation stack, with bounded buffers and no
whole-library resource cache.

Initial native interface: `beginReadFolder(lineup, folderId)` returns a ticket;
`pollReadFolder(ticket)` reports busy/ready/error; `takeFolder(ticket)` supplies
the completed snapshot on the VM thread. Scope invalidation to dynamic
catalogue/profile data and retain the static UI resource caches.

The hook already registers native closures using `sq_pushroottable`,
`sq_newclosure`, `sq_pushstring` and `sq_newslot`. Filling records requires
additional verified APIs, initially `sq_newtable` and `sq_pushinteger`. Resolve
and validate them against the original binary before any live VM changes;
unknown binaries must refuse the V2 hook.

## Save states and SRAM

V1 uses positional native save identifiers. The pause scripts compute
`file_id = save_slot + 4 * menu_game_index`; menu indices include lineup offsets.
The publisher and hook currently translate those identifiers into folder-pack
save files. The same positional dependency affects existence checks and previews.

V2 needs one routing layer for save, load, existence and preview operations:
`(game_id, save_slot)` resolves to permanent data/metadata files. The engine can
retain its temporary file numbers internally, but the persistent storage path
must come from the game ID. Reordering, changing a folder or switching a lineup
must not rename, copy or remap the persistent files.

SRAM requires its own adapter. The native aggregate has a fixed 150-slot layout;
its indices cannot simply become UUID-derived integers. Keep per-game SRAM
files and map the currently needed game data into the bounded native working
set, with digest checks and flush/load timing preserved. Do not infer SRAM
identity from `image_index` or menu position.

Route existence checks, previews, deletion and temporary/atomic writes as well
as save/load calls. A filename override that only handles the launched game
does not correctly identify other games' menu previews. The native filename
construction or backup I/O hook point still needs to be identified and tested.

Migration must decode the last published pack's slot-to-game mapping rather
than assume the current edited `gamelist.json` still has that order. Preserve
original states/metadata, validate SRAM digest and active-pack ownership, and
record an idempotent migration manifest. Ambiguous mappings must not be guessed.

## Graphics without folder publication

Library metadata can be supplied by the custom class. Rendering still needs
native motion resources: `Layer.registerMotionResource` expects a PSB object,
not a metadata table. Keep the original UI animations, frames and font resources.

Prepare each game's small console cover resource automatically when its cover
is imported or changed in the editor. Load that per-game resource on demand.
This preserves the native rendering path while eliminating aggregated folder
cover packs and the manual Publish step. Treat these resources as reproducible
per-game assets, with bounded active texture residency.

Font bitmap resources also need automatic maintenance when a title gains new
characters. The A33 does not render the bundled OpenType font directly: glyph
generation must run on title edits, without a manual whole-library export.

## First vertical slice

1. Validate the missing Squirrel API bindings and implement the bounded folder
   reader behind an opt-in V2 build switch.
2. Connect `ChronosLibrary` to one folder's metadata, ROM launch and a few game
   cover resources in the existing Linux VM test environment.
3. Add stable save/SRAM routing, then test reordered games, moves between
   folders/lineups and four save slots with an unchanged persistent identity.
4. Migrate a copy of a V1 library and test on A33 before replacing the default
   runtime. Present script changes for review before deployment.

Acceptance: edit a title/genre/order, reopen the library, navigate on the console
and launch the game without an export; existing save-state and SRAM content
remain associated with the same game across moves and navigation. Long-session
checks must show bounded memory and no accumulating folder/texture snapshots.
