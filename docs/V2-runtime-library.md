# V2: filesystem-driven runtime library

Status: included in the 2.0.0-alpha.1 editor prerelease, with the runtime
installed on the USB key. A compatible Chronos console bootstrap is required;
this editor release does not provide new P6/P7 firmware images.
The original ARM32 M2 engine has displayed source-driven menus and
passed initial navigation tests in the isolated Linux VM and on the physical
A33 console. Full-limit and long-session checks are still being validated;
the Mac rewrite does not validate native M2 behavior.

## User workflow

Create a library from a console dump, choosing the original games or an empty
library, or open a complete existing USB library. The editor prepares the
matching runtime in its sibling `game/` directory automatically. Edit titles,
genres, ordering, ROMs and covers normally: changes are saved without Publish,
and no folder catalogue or cover pack needs to be exported.

```text
USB/
  game/
    m2engage                           Original native engine
    lib/m2hook_print.so                 Bundled ARM hook
    system/script/*.nut.m               Eight bundled overrides
    system/motion/chronos_titleselect_jp.psb.m
    system/motion/chronos_titleselect_us.psb.m
    system/font/                       Prepared console bitmap fonts
    system/roms/                       Empty ROM bind-mount point
    save/                              Empty working-save bind-mount point
  library/
    .chronos-library-v2                 Source schema marker: "2\n"
    .current-v2                        Last successful lineup/folder
    settings_games.json
    jp/
      gamelist.json
      GAME000/
        game.json
        cover.png
        runtime/assets.json            Derived image descriptor
        runtime/*.image.psb.m           Revisioned cover and thumbnail
        sram.bin                       Canonical in-game memory, when present
        saves/                         Four permanent save-state slots
    us/
    runtime/back/                      Shared BACK artwork
    templates/                         Preserved original PSBs/fonts
    published/
      .per-game-saves-v2                Save schema marker: "2\n"
      roms/                            One shared ROM store
      save/                            Native settings/SRAM working bank
```

`published/roms/` and `published/save/` retain their names for compatibility
with the console launcher. They are not a second library or duplicate per-game
save store. Old `published/folders/` archives are retained during migration for
recovery, but the source runtime does not read them.

Preparation checks the original JP (`040`, `1006JP`) or international
(`041`, `1006WW`) runtime/templates, imports eligible legacy saves, normalizes
the shared ROM store, prepares images/fonts, and installs the hook plus eight
overrides. Both schema markers are committed last. Incomplete or mismatched
resources produce an error before source mode is enabled. Bare editing trees
without a complete sibling `game/` retain the legacy export workflow.

## P7 bootstrap and USB ownership

P7 contains startup, USB detection and mount helpers. The current hook and menu
overrides belong to the USB key, where the editor installs and updates them.
P7 does not overlay an authoritative runtime bundle over the key. Without a
key, the launcher runs the original engine without the Chronos preload.

The launcher binds `game/` over `/usr/game`, then the shared ROM and native
working-save directories over `/usr/game/system/roms` and `/usr/game/save`.
The hook reads `/mnt/usb/library`. Physical unplug forces the engine to stop;
it does not guarantee that an in-progress save has reached storage.

The eight packed overrides are `const`, `mode_demo`, `mode_title_select`,
`utils`, `emulator`, `systemdata_saveload`, `pause_main` and `backup`.
`chronos_library.nut`, `chronos_covers.nut` and `chronos_saves.nut` are source
helpers appended to utils by `mod-assets/build-scripts.py`, not extra runtime
loads. Nested loading while native boot executes `Resource.exec` deadlocks
the original loader.

## Current-folder catalogue

The C hook reads `gamelist.json`, its listed `game.json` files, `folder.json`
and `settings_games.json`. Explicit order, sort indices, genres, alphabetic-sort
exclusions, language, titlebars and emulator options come from the existing
editor JSON. No second editable metadata format is introduced.

`ChronosLibrary` constructs typed configuration views for the menu/emulator.
Filesystem work runs in the hook worker; Squirrel and graphics calls stay on
the menu thread. `beginReadFolder`, `pollReadFolder` and `takeFolder` use a
generation ticket, reject stale results and retain the previous snapshot when
preparation fails. Only the current folder snapshot is retained. Navigation
memory stores folder names/cursors, not previous game tables or image pools.

The last successful lineup/folder is written atomically to `.current-v2`.
JP and US also have separate in-session navigation locations. A removed
remembered folder falls back to that lineup's root. An empty lineup is not a
reason to silently select the other lineup.

The native limit is **50 entries per lineup root**, or **49 games plus BACK
inside a folder**. The editor keeps oversized metadata editable but warns;
the runtime rejects the snapshot rather than truncating it or misassigning
saves. Empty/small-menu guards hide unused carousel positions and skip invalid
selected-index or demo lookups. Navigation cards never launch or own a game.
A completely empty catalogue uses the private `CHRONOS_EMPTY` bootstrap
profile/ROM, without a fabricated visible entry. The editor recommends four
games per folder for the usual five-card layout with BACK; it is not a minimum.
The complete zero-to-many navigation matrix remains part of native validation.

## Stable per-game saves

`game.json` carries a permanent 32-hex `game_id` and a separate ASCII
`native_region_tag` of at most 14 bytes. A directory name, lineup or carousel
index is not a persistent save identity. Moving a game moves its whole
directory while retaining those fields.

Each game has `sram.bin` and `saves/state_0.bin` through `state_3.bin`, with
matching `state_N_meta.bin`. These are displayed as **slots 1–4**; missing slots
remain empty. One-byte `saves/locks.bin` stores their protection bits. The hook
routes native reads, writes, previews, deletion and temporary/rename operations
to these permanent files. Navigation does not rename save states.

SRAM uses one common native working object. Its complete per-game block is
**8,448 bytes**, at native aggregate offset **0x5e84**. Capture precedes the
stock outgoing title-context change. Incoming ownership is committed after
successful initialization/SRAM application; native autosaves flush the verified
active owner. Navigation profiles disable SRAM, and `chronosSaveActivate("")`
flushes/deactivates ownership so bootstrap writes cannot reach the old game.
Recovery copies for a temporarily absent USB owner live on NAND at
`/rootfs_data/.chronos-sram-recovery`, outside the USB save bind. Unknown or
malformed ownership is rejected.

Migration uses the **last published catalogue**, not the current edited order.
State layout, tags and full metadata digests are checked; sparse slots remain
sparse. Permitted retagging uses recoverable pair replacement. Ambiguous or
incomplete pairs are retained and reported. Shifted legacy SRAM is recovered
from a verified aggregate, never guessed from an unmarked standalone block.
Markers/fingerprints prevent stale archives from replacing newer or deleted
canonical saves. See [per-game save storage](../editor/docs/per-game-saves.md).

## Individual native images

The editor decodes/resizes source PNGs and creates minimal Image PSBs when a
cover changes. This also covers stock extraction, new-game placeholders,
folder mosaics and shared BACK artwork. Canonical PNGs remain editable inputs.
Title, genre, folder-order and lineup edits do not rebuild unchanged pixels.
Changing cover filename, packaging or dimensions refreshes only that entry.
PNG writes and image preparation are serialized with metadata geometry changes;
failed preparation retains the old descriptor and a persistent retry signal.

`runtime/assets.json` is a derived image descriptor, not a catalogue. It records
content-revision paths, dimensions, origins and optional SGX strips. The revision
includes pixels, geometry and generator version. Packed images are round-trip
validated and written atomically before the descriptor is committed. Failure
keeps its previous valid revision. Current/previous generated revisions remain
on disk. Native paths omit the physical file's final `.m` suffix.

Each PSB uses `id = image`, `spec = and`, `version = 1.0`, and one or three
`imageList` entries containing a complete RGBA8 texture part/pixel stream.
`Image(psbObject, "cover")` uses the original loader/renderer. The console
does not decode PNGs, build PSBs or intercept OpenGL during navigation.

The live pool holds **seven large covers**: five visible cards plus a neighbor
in either direction. Each active-folder entry has a **48×48 square thumbnail**,
with at most 50 thumbnails. Full images register only on the carousel/launch
layer, thumbnails only on the small layer. Resource, PSB and Image ownership
remain explicit until registrations are removed. Replacement prepares the new
resource first and retains the old image for rollback. At most one offscreen
replacement is pending at a time, in addition to the seven resident covers.
Old folder pools are released.

The final sorted selection primes its cover window once, before native motion
resources are registered and motions are created. This prevents the first
visible card from caching the transparent placeholder. Thumbnails load in one
native Resource batch. Subsequent neighbour reads are polled by the menu loop;
they do not introduce a loading wait at every completed cursor move. Explicit
sorts or jumps may still wait when a newly visible image is not ready. Character
movement and background updates continue during resource waits and folder fades.

Seven standard 240×240 SGX covers with 20/12-pixel strips represent about
1.83 MB of raw pixels, or 2.09 MB with one replacement; fifty thumbnails add
0.46 MB. These are pixel budgets,
not process-memory measurements. Native parsing, buffers, texture allocation
and driver overhead still require measurement.

### Reusable motion sheets and SGX

The desktop prepares fixed JP/US motion sheets from preserved originals,
without the original cover atlas. Original frames, masks, stencil operations,
disc/System Card sprites and animation transforms remain in the static UI.
Dynamic source name `chronos_cover` supplies:

- `pkg/front`: `slot_N_cover`, `slot_N_left`, `slot_N_right` for slots 0–6,
  with a coherent child tree, layer map and priorities.
- `pkg/center` and `pkg/sg`: `slot_N_center`, without R/L children.
- `pkg/thumb`: `thumb_N`, a genuine bitmap layer with `objTriPriority` and the
  original on-screen scale. Changing only a nested layer's type leaves an
  invalid native schema and crashes M2.

Case frames, SGX's inner CD case and ordinary HuCard labels use center-only
artwork. SGX's final label uses `sg`, rotated 180 degrees by the native animation;
pixels are not pre-rotated. CD launch retains generic disc art.

`ccolor == 3` selects the wide sleeve. Original strips/geometry are recovered
independently of current order or lineup. Daimakaimura uses a 240×240 center
plus 20×240/12×240 strips; Aldynes uses 26×240/6×240 strips with different origins.
A custom SGX cover without strips gets a neutral 20/12-pixel sleeve with a black
semicircular notch, without invented artwork/branding. Thumbnails and labels
remain center-only. Packaging `ccolor` is independent of launch `csize`.

`registerMotionSourceIconImage` binds an Image to a source/icon key absent from
the static source map. Existing keys are not overwritten automatically:
unregister before replacement. Each association gets a distinct registration
ID while Image/GPU texture ownership can be shared. IDs may be recycled, so
cleanup stays associated with its pool generation.

## Automatic font maintenance

The A33 uses bitmap glyph PSBs, not an OpenType font directly. The editor
maintains console fonts from Noto Sans CJK JP Medium when titles, folders or genre
names introduce characters, preserving controller symbols and the font license.
A glyph-set/content cache skips unchanged generation. The complete
read/generate/install sequence is serialized across editor workers, preventing
an older generation from replacing a newer glyph set. Original inputs remain
in `library/templates/`, generated fonts under `game/system/font/`; no manual
export is involved.

## Validation and remaining checks

Private VM prototypes proved generated RGBA8 Image construction, dynamic keys
absent from the source map, faithful sleeve surfaces, square/center-only views,
HuCard/SGX labels and successful game launch. The VM display adapter supplies
depth/stencil storage required by the original masks.

The integrated source runtime has read editor JSON and displayed native menus
with 0, 1, 2, 3, 4 and 10 entries. JP-folder to US-root and back to the same
JP-folder navigation passed, retaining the selected game in that folder. A direct JSON title edit appeared without folder
packs. Native SGX launch displayed its inverted label and wrote four canonical
state slots. Cloning a true bitmap layer corrected the thumbnail schema crash.
Native M2 also accepts a fixed sheet with an empty static source dictionary.
Independent per-game-save VM tests created four HuCard states, CD/SGX states,
verified headers/digests, and preserved HuCard payloads/locks after a JP-to-US
move. These results do not establish complete source navigation or a memory
plateau during long sessions. A 100-move instrumented scroll test kept ARM32
mappings at exactly 77,592 KiB resident from move 10 to move 100. Native live
allocations remained identical (20,582 objects, 2,917,727 bytes), and guest libc
grew by eight bytes. The RSS increase was entirely in QEMU host mappings above
4 GiB. This is a short VM result, not a hardware soak or driver-cache bound. The
settings screen also returned successfully to the source carousel. Malformed
folder JSON displayed an error while retaining the previous menu; reopening
after correction succeeded. The empty-folder message is visible.

On 2026-10-17, physical A33 hot-plug logs confirmed the game, shared-ROM and
save mounts, hook startup, a 22-entry JP source menu, and entry into a folder
with four games plus Back and its individual cover pool. The failed first
insertion exposed a wizard layout error: `Chronos/game` and `Chronos/library`
were one level below the paths expected by P7 and the hook. Creation now places
`game/` and `library/` directly in the chosen USB root or destination folder.

The same hardware session validated root/folder navigation with 22 JP root
entries, 11 Shooters entries and five RPG entries. The user confirmed smoother
scrolling after asynchronous neighbour loading, then confirmed that the partly
visible left cover remains correct after the motion-initialization fix. Native
VM checks also cover rapid direction reversals, RPG-to-root returns and a jump
that reuses the same physical cover slot for another game. These results are
functional checks and qualitative feedback, not measured hardware frame times.

Remaining checks include the full 0–50 matrix, both resource variants,
extended folder/lineup/launch cycles, glyph updates, artwork-error rollback,
and physical A33 latency/memory measurements. No measured A33 timing or hardware soak is claimed.

Automated checks include `cargo test -p m2-publish --lib`, editor/import and
frontend tests, and portable hook catalogue/save regression suites. Private
fixtures are in `research/v2-cover-vm-20261013` and
`research/v2-source-runtime-20261013` in the development workspace. Original
firmware-derived inputs are not distributed with this repository.
