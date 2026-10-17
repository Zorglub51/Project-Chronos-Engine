# Per-game parameters and Expert settings

The Expert section edits settings that the ordinary game details do not expose.
Audio gain (`preamp`) is here. Changes are saved in `game.json` and apply at the
next game initialization or catalogue reload on the console. Editing a key on a
running console does not mutate an already-running machine.

Every Expert field has an **i** button describing its effect and possible values
in English. The same help is attached to ordinary game detail fields. Open it
with hover, keyboard focus or click; Escape or clicking outside closes it.
Unavailable/ignored PC Engine effects are identified explicitly.

This catalogue covers **every stock per-item key in `title_mode_top`** and every
per-version key read from `title_prof` by the shipped PC Engine scripts. The
machine-readable source is [game-parameters.json](../editor/src/game-parameters.json),
which supplies the UI descriptions, types and backend validation.
Root package/controller/session settings are shared template settings, not
per-game overrides. They are described separately below.

## Storage and application

| Editable source | Native destination | Application |
| --- | --- | --- |
| `display.*` | `title_mode_top.items[]` | Menu text, packaging, demo duration and retained descriptive metadata |
| `emulator.*` | `title_mode_top.items[]` | Menu calls `SetScreenMode`, `SetEmuScreenOffsetX/Y` before launch |
| `rom.*` | `title_prof.root.m2epi.version[tag]` | Stock `init_emulator` and input/SRAM utility readers |
| `rom_alt.*` | Item variant tags and profile aliases | SELECT/hidden-variant lookup; aliases currently inherit the primary ROM/profile |
| Editor order and genre | Item `sor_*` | Dense active-folder sort permutations |

Source mode: the C hook reads and emits the **entire typed game JSON**, including
unknown nested ROM fields. `ChronosLibrary` creates the two language views,
normalizes the public VM array adapter and builds complete per-version profiles.
Only managed file paths, identities and save routing are replaced. Optional nulls
from older editors are removed; zero and false remain explicit overrides.
Legacy export now retains all extra ROM fields through PSB generation as well.
No editor action silently adds all the displayed default values to old games.

Deleting an optional override restores the shared template's value. `attr`
replaces its entire global table; `string` merges names over the global table.
The root `sram_common` and working SRAM slot remain managed by Chronos V2.

## Expert parameters

Values below describe the supported original ARM engine build. UI-only limits
are identified explicitly. An inactive option is still preserved and forwarded;
that does not make an ignored backend command effective.

### `ScreenMode` — Screen framing

- Storage: `emulator.ScreenMode`.
- Values: integer; 0 = 0 · Standard (272 → 256), 1 = 1 · Full raster, 2 = 2 · 272 → 256, 352 → 320, 3 = 3 · Same crop as 2, 4 = 4 · 272 → 256, 352 → 336, 512 → 256, 5 = 5 · 272 → 256, 512 → 256; range 0…2147483647.
- Default: Required; fallback 0.

Selects a crop before display scaling. Heights are unchanged. The crop depends on the core; see the exact tables below. Other integers retain the full raster.

Evidence: `TG16 0x7612c; TG16CD 0x7e344; SGX 0x87e64`.

### `EmuOfsX` — Horizontal offset

- Storage: `emulator.EmuOfsX`.
- Values: integer.
- Default: Required; fallback 0.

Adds a signed pixel offset to the crop's source position. A positive value moves the sampled window right/down; a negative value moves it left/up. This does not move the on-screen UI. The native renderer ignores this addition when no explicit crop is active.

Evidence: `TG16 video descriptor 0x761a0–0x761e6`.

### `EmuOfsY` — Vertical offset

- Storage: `emulator.EmuOfsY`.
- Values: integer.
- Default: Required; fallback 0.

Adds a signed pixel offset to the crop's source position. A positive value moves the sampled window right/down; a negative value moves it left/up. This does not move the on-screen UI. The native renderer ignores this addition when no explicit crop is active.

Evidence: `TG16 video descriptor 0x761a0–0x761e6`.

### `preamp` — Audio gain (dB)

- Storage: `rom.preamp`.
- Values: number.
- Default: Required; fallback 0.

Relative audio gain in decibels: 0 keeps the normal level, +6 is approximately twice the signal amplitude, −6 approximately half. Large positive values can clip. This adds to the engine's driver gain.

Evidence: `emulator.nut init_emulator; native mixer 0x7480c / 0x748a8`.

### `overdrive` — Input latency reduction

- Storage: `rom.overdrive`.
- Values: integer.
- Default: Inherited; fallback 0.

0 or less disables run-ahead. N > 0 displays a frame simulated N frames ahead using current inputs, then restores the normal state. It can make controls feel quicker, costs extra CPU, and does not permanently speed up the game.

Evidence: `emulator.nut init_emulator; native task 0x1ed238`.

### `brightness` — Brightness multiplier

- Storage: `rom.brightness`.
- Values: number; range 0…2³¹−1.
- Default: Inherited; fallback 1.

Multiplies all displayed RGB channels. 1 is unchanged; 0 is black; values above 1 brighten and may saturate. The emulated VDC palette is unchanged.

Evidence: `emulator.nut init_emulator; EmuView 0x2fe78`.

### `country` — Hardware region

- Storage: `rom.country`.
- Values: string; jp = Japan, us = USA.
- Default: Required.

The original core recognizes jp and us. jp sets the controller region bit; us clears it. This can affect games that inspect the region, but does not select a PAL refresh rate. Other strings use the Japanese fallback.

Evidence: `TG16 constructor 0x76d30, controller read 0x76528`.

### `tg16pad_type` — Controller protocol

- Storage: `rom.tg16pad_type`.
- Values: integer; 0 = 0 · Two buttons, 1 = 1 · Two buttons + multitap, 2 = 2 · Six buttons, 3 = 3 · Six buttons + multitap, 4 = 4 · Two buttons + per-port auxiliary, 5 = 5 · Two buttons + multitap + per-port auxiliary, 6 = 6 · Six buttons + per-port auxiliary, 7 = 7 · Six buttons + multitap + per-port auxiliary; range 0…7.
- Default: Inherited; fallback 1.

Bit mask: 1 enables the five-port multitap; 2 enables the six-button protocol; 4 keeps auxiliary input per port instead of sharing it. Value 5 still uses TWO buttons.

Evidence: `emulator.nut _init_emulator_get_arch_sub_info; 0x77b58 / 0x77b9c`.

### `tg16_option` — Game timing options

- Storage: `rom.tg16_option`.
- Values: integer; range 0…15.
- Default: Inherited; fallback 0.

Bit 0 (value 1) moves VBlank IRQ and sprite-table transfer to the start of the next frame (TG16/TG16CD). Bit 2 (value 4) doubles CPU work on scanlines 0–29 and 249–262, TG16 HuCard only. Values 2 and 8 have no identified effect. 5 combines 1 and 4. SGX ignores these options.

Evidence: `0x349a8; VDC 0x7af60; TG16 frame 0x762c8`.

### `tg16_hblank` — Horizontal timing

- Storage: `rom.tg16_hblank`.
- Values: integer; range 0…255.
- Default: Inherited; fallback 0.

Adjusts horizontal sequencing, not pixel width. Values 0–9 select the native default of 87; 10–255 use that value. Used by TG16/TG16CD; ignored by SGX.

Evidence: `VDC constructor 0x7a050; horizontal sequencer 0x7b5bc`.

### `use_analog` — Keep analog input separate

- Storage: `rom.use_analog`.
- Values: boolean.
- Default: Inherited; fallback false.

True disables the normal conversion/merging of the left stick into digital directions. It does not add analog controls to a PC Engine game.

Evidence: `emulator.nut _init_emulator_get_arch_sub_info; native 0x240a2`.

### `direction` — Stick directions

- Storage: `rom.direction`.
- Values: integer; 4 = Four directions, 8 = Eight directions.
- Default: Inherited; fallback 8.

4 converts the stick into four cardinal sectors; every other value uses eight-direction handling, allowing diagonals. This affects stick conversion, not the game's controller button count.

Evidence: `emulator.nut _init_emulator_get_arch_sub_info; native 0x240b8 / 0x158b8`.

### `player_num` — Runtime player count

- Storage: `rom.player_num`.
- Values: integer; range 0…10.
- Default: Inherited.

Overrides the profile's runtime player count. It is independent of the player count printed on the menu card.

Evidence: `utils.nut util_get_player_real_num / util_get_player_input_num`.

### `player_input_num` — Input slot count

- Storage: `rom.player_input_num`.
- Values: integer; range 0…10.
- Default: Inherited.

Overrides the number of input slots initialized by the wrapper. Without an override it uses the profile input count, then the runtime player count. The engine has ten logical input slots; these are not ten physical ports.

Evidence: `utils.nut util_get_player_real_num / util_get_player_input_num`.

### `rapid_speed_list` — Rapid-fire speeds

- Storage: `rom.rapid_speed_list`.
- Values: array; positive integer frequencies (editor limit: 1–32 entries, 1–60 each).
- Default: Inherited.

List of offered rapid-fire frequencies. The retail list is [10, 12, 15, 20, 30]. Removing this override restores the controller template's list.

Evidence: `utils.nut util_get_controller_rapid_speed_list; controller rapid menu`.

### `view_split` — Render subdivisions

- Storage: `rom.view_split`.
- Values: array; [X,Y], positive integers (editor limit: 1–16 per axis).
- Default: Inherited; fallback [1, 1].

Two positive integers [X,Y] subdivide the display mesh. This does not split the screen between players. The retail Y setter has a comparison bug, so some transitions can skip rebuilding the mesh.

Evidence: `EmuView 0x311c4 / 0x311d8 / 0x30e24`.

### `sram_support` — In-game save memory

- Storage: `rom.sram_support`.
- Values: boolean.
- Default: Inherited; fallback true.

Enables the script SRAM adapter for this game. Chronos V2 enables it globally; false disables the adapter for this game. It does not create backup RAM in emulated hardware or enable save states.

Evidence: `utils.nut util_is_sram_support; chronos_library.nut build_profile`.

### `extra_info` — Extra machine information

- Storage: `rom.extra_info`.
- Values: integer.
- Default: Inherited.

Forwarded to the backend factory, but the TG16, TG16CD and SGX factories do not consume it.

Evidence: `emulator.nut init helpers; TG16/TG16CD/SGX native adapters`.

### `attr` — Native integer attributes

- Storage: `rom.attr`.
- Values: object; string keys to signed 32-bit integers.
- Default: Inherited.

Each key/value is sent to setAttribute. The PC Engine native callbacks return without applying it. A per-game table replaces the global table.

Evidence: `emulator.nut init helpers; TG16/TG16CD/SGX native adapters`.

### `string` — Native string attributes

- Storage: `rom.string`.
- Values: object; string keys to strings.
- Default: Inherited.

Per-name overrides are merged with global strings and sent to setString. The PC Engine backends ignore that command.

Evidence: `emulator.nut init helpers; TG16/TG16CD/SGX native adapters`.

### `stringGlobal` — Global string attributes

- Storage: `rom.stringGlobal`.
- Values: object; string keys to strings.
- Default: Inherited.

Read by the script, but its setter call is commented out; no runtime effect.

Evidence: `emulator.nut init helpers; TG16/TG16CD/SGX native adapters`.

### `disable_md_6B` — Disable Mega Drive six-button mode

- Storage: `rom.disable_md_6B`.
- Values: boolean.
- Default: Inherited.

A Mega Drive flag. It does not change the PC Engine controller protocol.

Evidence: `emulator.nut init helpers; TG16/TG16CD/SGX native adapters`.

### `md_multitap` — Mega Drive multitap

- Storage: `rom.md_multitap`.
- Values: integer.
- Default: Inherited.

Its reader is commented out in this build; no runtime effect.

Evidence: `emulator.nut init helpers; TG16/TG16CD/SGX native adapters`.

### `use_subthread` — Emulator subthread

- Storage: `rom.use_subthread`.
- Values: boolean.
- Default: Inherited.

Its reader is commented out in this build; no runtime effect.

Evidence: `emulator.nut init helpers; TG16/TG16CD/SGX native adapters`.

### `action` — Menu action

- Storage: `display.action`.
- Values: string.
- Default: Inherited; fallback "Boot".

Preserved in title_mode_top. No consumer of this per-item field was found in the shipped selection/demo scripts; changing it has no identified visible effect.

Evidence: `Stock title_mode_top items; menu and demo read audit`.

### `copy` — Copyright text

- Storage: `display.copy`.
- Values: string.
- Default: Inherited; fallback "".

Preserved in title_mode_top. No consumer of this per-item field was found in the shipped selection/demo scripts; changing it has no identified visible effect.

Evidence: `Stock title_mode_top items; menu and demo read audit`.

### `desc` — Description

- Storage: `display.desc`.
- Values: string.
- Default: Inherited; fallback "".

Preserved in title_mode_top. No consumer of this per-item field was found in the shipped selection/demo scripts; changing it has no identified visible effect.

Evidence: `Stock title_mode_top items; menu and demo read audit`.

### `regionTagE` — SELECT variant

- Storage: `rom_alt.regionTagE`.
- Values: string; NO/empty or 1–14 ASCII letters, digits or underscores.
- Default: Inherited; fallback "NO".

NO or empty disables the shortcut. Otherwise use an ASCII native tag of at most 14 characters. The menu selects it with SELECT (E) or the hidden variant sequence (S1/S2). Chronos currently gives aliases the same ROM/profile as the primary game.

Evidence: `mode_title_select.nut _resolve_selected_regionTag; title_prof variant map`.

### `regionTagS1` — Hidden variant 1

- Storage: `rom_alt.regionTagS1`.
- Values: string; NO/empty or 1–14 ASCII letters, digits or underscores.
- Default: Inherited; fallback "NO".

NO or empty disables the shortcut. Otherwise use an ASCII native tag of at most 14 characters. The menu selects it with SELECT (E) or the hidden variant sequence (S1/S2). Chronos currently gives aliases the same ROM/profile as the primary game.

Evidence: `mode_title_select.nut _resolve_selected_regionTag; title_prof variant map`.

### `regionTagS2` — Hidden variant 2

- Storage: `rom_alt.regionTagS2`.
- Values: string; NO/empty or 1–14 ASCII letters, digits or underscores.
- Default: Inherited; fallback "NO".

NO or empty disables the shortcut. Otherwise use an ASCII native tag of at most 14 characters. The menu selects it with SELECT (E) or the hidden variant sequence (S1/S2). Chronos currently gives aliases the same ROM/profile as the primary game.

Evidence: `mode_title_select.nut _resolve_selected_regionTag; title_prof variant map`.

## ScreenMode: exact crop tables

Offsets are **source pixels before scaling**. Height is unchanged. **EmuOfsX/Y
are added only when the renderer uses an explicit crop**. In full-raster modes,
the native renderer computes its own centered offset and ignores those additions,
unless the separate GT-screen crop has enabled an explicit window.

The three backends do not share the same mode switch. The choices and information
bubble in Expert follow `rom.arch`, including after changing the platform.

### HuCard / TG16

| Mode | 272-pixel raster | 352-pixel raster | 512-pixel raster |
| --- | --- | --- | --- |
| 0 | 256 at X=8 | 352 at X=0 | 512 at X=0 |
| 1 | 272 at X=0 | 352 at X=0 | 512 at X=0 |
| 2 | 256 at X=8 | 320 at X=16 | 512 at X=0 |
| 3 | 256 at X=8 | 320 at X=16 | 512 at X=0 |
| 4 | 256 at X=8 | 336 at X=8 | 256 at X=160 |
| 5 | 256 at X=8 | 352 at X=0 | 256 at X=160 |

### CD-ROM / TG16CD

| Mode | 272-pixel raster | 352-pixel raster | 512-pixel raster |
| --- | --- | --- | --- |
| 0 | 256 at X=8 | 352 at X=0 | 512 at X=0 |
| 1 | 256 at X=16 | 352 at X=0 | 512 at X=0 |
| 2 | 256 at X=8 | 320 at X=24 | 512 at X=0 |
| 3 | 256 at X=8 | 336 at X=8 | 512 at X=0 |
| 4 | 256 at X=8 | 336 at X=8 | 512 at X=0 |
| 5 | 272 at X=0 | 352 at X=0 | 512 at X=0 |

### SuperGrafx

| Mode | 272-pixel raster | 352-pixel raster | 512-pixel raster |
| --- | --- | --- | --- |
| 0 | 256 at X=8 | 352 at X=0 | 512 at X=0 |
| 1 | 272 at X=0 | 352 at X=0 | 512 at X=0 |
| 2 | 272 at X=0 | 352 at X=0 | 512 at X=0 |
| 3 | 256 at X=8 | 320 at X=16 | 512 at X=0 |
| 4 | 272 at X=0 | 352 at X=0 | 512 at X=0 |
| 5 | 272 at X=0 | 352 at X=0 | 512 at X=0 |

Other integers retain the full raster. R-Type uses mode 4 in the normal
stock menu path. Its VCE control value 5 produces a 352×240 raster with 336
active pixels at X=8 or 320 at X=16 according to its VDC registers.
A shared `isGTScreen` setting can independently crop 16 vertical pixels; it is
not a per-game title_prof field.

Evidence: [63 native descriptor probes](evidence/screen-modes.json), executing
the original ARM functions at 0x7612c, 0x7e344 and 0x87e64 with modes 0–6 and
raster widths 272/352/512. The generic descriptor consumer at 0x348e8–0x3499a
selects explicit offsets only when descriptor flag +20 is set.

## Fields in ordinary details

- `name` (`display.name`): Menu bookkeeping/name and Japanese display fallback; the normal editor keeps tname in sync.
- `tname` (`display.tname`): Text actually drawn in the Japanese menu view.
- `tname (English view)` (`display.name_eng`): tname in the translated menu half; the two views share one game/profile.
- `players` (`display.players`): Player count displayed on the card and used by the player-count sort; not the runtime input-slot count.
- `titlebar` (`display.titlebar`): 0 hides the titlebar; 1–12 select a shipped titlebar style.
- `ccolor` (`display.ccolor`): 0/1 select ordinary case colors; 2 selects double-CD packaging; 3 selects the SGX wide sleeve. This changes artwork geometry, not the emulation core.
- `csize` (`display.csize`): 0 HuCard, 1 SGX, 2 CD, 3 Super CD, 4 Arcade CD; Chronos reserves 10/11 for folder/BACK navigation. The ordinary platform picker also sets arch and System Card.
- `demo_time` (`display.demo_time`): Number of menu/emulation ticks (approximately 60 per second), not milliseconds. The demo exits when its incremented counter exceeds this number.
- `arch` (`rom.arch`): tg16, tg16cd or sgx; derived from the ordinary platform picker. The editor marker folder is exported as a non-playable tg16 bootstrap.
- `tg16cd_systemcard` (`rom.tg16cd_systemcard`): super selects Super CD RAM; arcade adds 2 MiB Arcade Card RAM. Any other/absent value selects the base card. Derived from the ordinary platform picker.
- `rom` (`rom.rom`): Chosen shared ROM filename; the hook validates the file, rewrites its logical path and omits the encryption suffix .m.

## Fields owned by Chronos

These are explained in Expert but are not independently editable: changing an
image index or save identity would disconnect it from its allocated resources.

- `image`: Allocated current-folder artwork slot (0–49). Derived from menu order and cover assets.
- `dev_name`: Package identity that chooses the title session/resources. Derived from the original template; not a per-game emulation region.
- `regionTag`: Primary native profile key, at most 14 ASCII characters. Chronos keeps it stable for native initialization and save routing.
- `game_id`: Permanent Chronos save identity, not a menu index.
- `save_path`: Validated game directory used by the hook to route saves.
- `save_type`: Native save-state type, derived from the core: 11 for CD, 12 for HuCard/SGX, 0 for navigation.
- `systemdata_sram_data_no`: Retail per-version SRAM slot override. Chronos V2 uses its managed common working slot and per-game hook routing; this index is not editable.
- `sor_date`: Dense sort rank generated from the editor order, genre, name or player count. BACK remains first; ranks are rebuilt for each active folder.
- `sor_demo`: Dense sort rank generated from the editor order, genre, name or player count. BACK remains first; ranks are rebuilt for each active folder.
- `sor_genr`: Dense sort rank generated from the editor order, genre, name or player count. BACK remains first; ranks are rebuilt for each active folder.
- `sor_name`: Dense sort rank generated from the editor order, genre, name or player count. BACK remains first; ranks are rebuilt for each active folder.
- `sor_pnum`: Dense sort rank generated from the editor order, genre, name or player count. BACK remains first; ranks are rebuilt for each active folder.

The stock top-level keys `items`, `titleNum`, `titleNumTG`, motion resource names,
`scroll_frame` and `scroll_variable` describe the shared catalogue/animation.
They are not attributes of one game. The catalogue builder supplies counts and
fixed-size language tables; changing a game's settings does not replace them.

## title_prof settings that are not per-game fields

These settings are deliberately not copied into each `m2epi.version[tag]`: the
stock reader would ignore many of them there. They remain in the original shared
template, except explicit Chronos-managed save settings.

| Shared path/group | Values and function |
| --- | --- |
| `title.dev_name` | Package/session identity and resource path conversion; not hardware region. |
| `player_num`, `player_input_num` | Session/player/input defaults. The per-version fields above override the relevant utility readers. |
| `game_versions`, `game_versions_default`, `game_versions_layout` | Profile indices, DLC/text mapping, default version and selectable version ordering. Chronos generates these from the live catalogue. |
| `m2epi.sram_hot_swap` | Boolean permitting SRAM/memory-card setting changes while paused, only when SRAM is enabled. |
| `m2epi.sram_crypted` | Boolean selecting the compressed SRAM adapter (zlib, despite the name). |
| `m2epi.sram_need_save_frame` | Integer passed to the save-frame setter; the PC Engine adapters ignore its command. |
| `m2epi.sram_common` | Boolean selecting a shared native working SRAM object; forced true in Chronos V2, whose hook supplies per-game ownership. |
| `m2epi.attr_each` | Boolean controlling whether emulator attribute settings are shared between variants. |
| `m2epi.sram_noupdate_when_state_load`, `sram_nocheck_when_state_load` | Boolean controls for preserving SRAM on state restore / bypassing the memory-card comparison. They do not select save-file ownership. |
| `m2epi.debug_logging` | Boolean requesting emulator logging in non-master/debug paths. |
| `m2epi.screen_setting_state` | Tag-to-state-path table used for the display settings preview. |
| `m2epi.subst_screen_zoom_x` | Numeric horizontal multiplier for the substitute preview image. |
| `controller` | Shared button labels/maps, default rapid-fire list, mappings, option availability and reset behavior. Per-version player/input/rapid fields are the supported per-game overrides. |
| `screen` and display-setting tables | Shared screen presets, zoom bounds, rotation/orientation, menu availability and default settings; not ScreenMode's raster crop. |
| `default_patch` | Shared RAM patch mode/list. The shipped PC Engine command adapter does not implement the patch command; forwarding it does not modify emulated RAM. |
| Input/menu/help/resource/session tables | Package-wide player-specific UI layout, input assignment, settings flags and resource references. Their paths and table shapes are not per-game engine options. |

The wider reverse-engineering audit catalogues 150 access paths (including
commented readers), not 150 independently effective per-game switches. No UI
field is advertised as functional merely because a string exists in the binary.

## Validation and evidence

Original binary: SHA-256
`b02848f66b82f8ac3090db523c4db9633508f9e3f53c7dc0ee3d01ce8aee8792`.
Native findings were obtained from its Thumb code and targeted r2 ESIL probes;
they are not inferred from the Mac replacement emulator. The original
`tg16_option` reasons chosen by M2 for individual games have not been proven by
visual comparison on hardware.

Validation uses one full Expert fixture across editor saving, PSB round trips,
C hook parsing/typed VM emission, Squirrel 2.2 view construction and the actual
stock option-packing functions. This proves transmission and masks. It does not
prove that changing every setting improves every game, or replace testing on a
physical PC Engine Mini.

```sh
cd editor
node --test tests/*.test.cjs
cargo test -p m2-publish --offline --lib
cargo test -p pce-game-editor --offline --lib
cd ..
python3 -m unittest discover -s console-mod/hook-src/tests -p 'test_catalog_runtime.py'
python3 mod-assets/tests/check-expert.py --checker /path/to/squirrel-2.2-checker
```
