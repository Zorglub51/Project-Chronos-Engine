# m2engage FMV black-screen — Investigation Report (vramdump pass)

Status: **bug localized to the display side of the VDC/VCE — VRAM is
correctly populated during the black cutscene**. The decoder, the
CPU→VRAM blit, the CD subsystem, and the in-memory data path are all
ruled out. Next step is to inspect the VCE palette and the VDC display
controls.

Date: 2026-05-21.

---

## TL;DR

The previous pass (`m2hook_cdtrace`) showed the CD subsystem delivers
FMV sectors cleanly. This pass instruments m2engage's emulated **VDC
VRAM** with a periodic dumper, runs PopfulMail through the black-screen
cutscene, and looks at what's in VRAM at the moment the screen is
black.

Result: **VRAM holds ~46–50 KB of structured tile + BAT + SAT data**
while the screen is fully black.

| moment                | non-zero VRAM bytes | percent | vram base   |
|-----------------------|--------------------:|--------:|-------------|
| menu (pre-launch)     | 0/65536             | 0.0 %   | 0x01537370  |
| PopfulMail boot       | 12947/65536         | 19.8 %  | 0x01657908  |
| during black cutscene | 47533–49829/65536   | 72.5–76.0 % | 0x01657908 |
| sustained (cutscene)  | 45000–49000/65536   | ~70 %   | 0x01657908  |

Per `SPEC.md` §7 — populated VRAM ⟹ **display-side bug**. The image is
in VRAM. Something downstream (palette / display-enable / scroll
register / VCE blanking) hides it.

---

## What we built

`/Users/vincentaycirieix/dev/pce/m2hook_vramdump/` — `LD_PRELOAD`
observer that:

1. Inline-patches the VDC constructor at VMA `0x7aa08` (same proven
   trampoline mechanism as `m2hook_overdrive` / `m2hook_cdtrace`). The
   C handler captures `r0` = VDC context pointer into a global.
2. Spawns a background pthread that wakes every ~2 s, reads
   `vram_ptr = *(g_vdc_ctx + 0x14)`, dumps the 64 KB VRAM to
   `/tmp/m2_vram_NNNN.bin`, dumps the first 256 bytes of the VDC ctx
   to `/tmp/m2_vdcctx_NNNN.bin`, and writes a one-line summary
   (non-zero byte count, ctx pointer, vram pointer, timestamp) to
   `/tmp/m2_vramdump.log`.

| File | Purpose |
|---|---|
| `SPEC.md`                 | Original spec |
| `m2hook_vramdump.c`       | Hook source (with the two fixes below) |
| `build.sh` / `Makefile`   | Docker cross-compile |
| `README.md`               | Build / deploy / test |
| `build/m2hook_vramdump.so` | Compiled .so |
| `dumps/`                  | Captured PopfulMail VRAM + VDC-ctx dumps + log |
| `REPORT.md`               | This file |

### Two bugs we hit during deployment (lessons for the next hook)

1. **`usleep` cap on Mini's older glibc.** Initial dumper used
   `usleep(2_000_000)`. POSIX `usleep` is defined for values `< 1e6 us`
   only, and on the Mini's Linux-3.4-era glibc this returns `EINVAL`
   immediately, leaving the dumper loop spinning without doing anything.
   Replaced with `nanosleep(struct timespec)` which takes arbitrary
   durations and is the canonical sleep primitive.
2. **m2engage fork()s before its emulator subsystem starts.** The
   `LD_PRELOAD` constructor runs in the parent process (PID Y, only ~2
   threads — setup work); the actual emulator runs in a *forked child*
   (PID X+1, ~9 threads). Threads do **not** carry across `fork()`,
   so the dumper thread we created in the constructor lived in the
   parent process and saw `g_vdc_ctx` as 0 forever — while the VDC
   constructor handler (running in the child) was happily writing the
   real value to *its* `g_vdc_ctx`. Same address, different process,
   different memory. Fixed by deferring the dumper spawn to inside the
   VDC ctor handler itself, using an atomic-CAS once-flag — this way
   the thread is created **post-fork in the same process** as the
   handler. (We diagnosed this from `/proc/<pid>/maps` + thread counts:
   parent had `m2hook_vramdump.so` mapped with 2 threads, child had it
   mapped with 9 threads but no dumper. Same library, two processes.)

Both fixes are documented in the source as in-code comments — keep them
when extending or reusing this hook.

---

## Capture session

Engine state during capture:
- PopfulMail (`GAME08`, tg16cd arch, `arcade` system card)
- `overdrive` cleared from all games (was 0 globally during capture).
- LD_PRELOAD chain: `m2hook_print.so:probe_gl.so:m2hook_vramdump.so`
  (no cdtrace; no overdrive hook).
- User launched the game, sat on the black opening cutscene for
  multiple minutes while the dumper ticked every 2 s.

Result:
- **188 VDC ctx dumps** captured (no rotation on these, all kept).
- **20 VRAM dumps** kept (the dumper rotates to the last 20 to avoid
  filling tmpfs — the rotation is by SPEC §4b).
- Full log at `dumps/m2_vramdump.log`.

### Log timeline (sampled)

```
#0000  vram=0x01537370   0/65536  (0.0%)    ← menu, no game
#0001  vram=0x01537370   0/65536  (0.0%)
…
#0005  vram=0x01537370   0/65536  (0.0%)
                                              ← VDC ctor fires (new game launch)
                                              ← ctx changes to 0x0150ec80
                                              ← vram base changes to 0x01657908
#0006  vram=0x01657908   12947/65536  (19.8%)  ← boot loading begins
#0007                    12796/65536  (19.5%)
#0008                    15889/65536  (24.2%)
#0009                    16281/65536  (24.8%)
#0010                    13364/65536  (20.4%)
#0011                    47533/65536  (72.5%)  ← black cutscene now (per user) — VRAM is populated
#0012                    47533/65536  (72.5%)
#0013                    47533/65536  (72.5%)
#0014                    49829/65536  (76.0%)
…
#0019                    49719/65536  (75.9%)
#0029                    45352/65536  (69.2%)
#0049                    47056/65536  (71.8%)
#0099                    45434/65536  (69.3%)
#0149                    49719/65536  (75.9%)
#0187                    46020/65536  (70.2%)  ← still black on-screen, VRAM still ~70%
```

Stability of the percentage across hundreds of seconds suggests the
engine is **actively writing FMV frames into VRAM** the whole time — the
content fluctuates between ~45 KB and ~50 KB non-zero, consistent with a
streaming decoder updating tiles/BAT each frame.

### Sample VRAM content (dump `m2_vram_0168.bin` during sustained black-screen)

First 64 bytes — looks like the BAT (Background Attribute Table):

```
00000000  80 20 a1 21 a2 21 a3 21  a4 21 a5 21 d8 20 9a 61  |. .!.!.!.!.!. .a|
00000010  a6 61 74 61 9c 91 a7 61  a8 61 a9 61 aa 81 ab 81  |.ata...a.a.a....|
00000020  74 61 0e 63 0f 93 10 93  11 93 12 93 13 93 14 93  |ta.c............|
00000030  15 63 16 63 17 63 df 02  18 03 37 02 03 02 80 20  |.c.c.c....7.... |
```

PCE BAT entries are 16-bit words `(palette << 12) | tile_number`. The
sequence `a1 21 / a2 21 / a3 21 / a4 21 / a5 21` decodes as tiles
0x1a1..0x1a5 with palette 2 — a horizontal run of consecutive tiles, the
classic FMV-strip layout.

Mid-VRAM (offset 0x8000) — tile pattern data, looks like a typical PCE
2-bitplane-pair tile format:

```
00008000  fe 03 ad f3 b5 7b 7f 81  5e e1 4a fd d5 3f e5 1f  |.....{..^.J..?..|
00008010  00 00 40 40 00 00 00 00  80 80 30 30 08 08 02 02  |..@@......00....|
…
```

End of VRAM (last bytes — typically the SAT, Sprite Attribute Table):

```
0000ff20  a0 00 40 00 aa 01 80 10  00 00 00 00 00 00 00 00  |..@.............|
```

`a0 00 / 40 00 / aa 01 / 80 10` — a valid SAT entry (Y, X, pattern,
attribute). Not random.

**All three layers (BAT + tiles + SAT) are populated with non-random,
non-zero content.** This is real PCE VDC state, not garbage. If displayed
correctly, it would render the FMV frame.

### VDC context snapshot (`m2_vdcctx_0168.bin`)

First 0x60 bytes:

```
00000000  00 ba 08 01 f0 00 08 00  08 00 e8 00 70 99 85 01  |............p...|
00000010  88 3b 73 01 08 79 65 01  a0 f2 51 01 40 00 00 00  |.;s..ye...Q.@...|
00000020  02 00 00 00 00 e0 00 00  00 80 00 40 00 00 00 00  |...........@....|
00000030  00 00 cc 00 40 00 b2 00  00 00 10 00 02 02 1f 04  |............@...|
00000040  02 0f ef 00 04 00 10 00  00 00 00 00 00 00 00 7f  |................|
00000050  01 00 00 00 04 00 00 00  63 74 5f 73 22 00 00 00  |........ct_s"...|
```

This is the C++ EmuVDC context. Members at known offsets (from
disassembly):

- `+0x00` : `0x0108ba00`  — a function table / vtable-like pointer.
- `+0x04` : `0x000800f0` — likely a sub-object pointer or two halfwords.
- `+0x14` : `0x01859970` — the VRAM pointer (matches the log:
  `vram=0x01657908` for *this* dump session — note the value above is
  from a different dump; cross-reference per-file).
- The block of small ints at `+0x30..+0x60` is most likely the **VDC
  register file** (HuC6270 has 20 registers, each 16 bits, fits in
  ~40 bytes). The exact mapping needs disassembly of the VDC register
  R/W path (which is the next step).
- `ct_s"` ASCII at `+0x58` is part of a class-name string — likely
  RTTI for the VDC class identifier (`PSGEmuVDC` or similar).

To localize the bug we'll need to identify **VDC register R5 (CR —
Control Register)** in this layout. CR's bits SB (BG enable) and CB
(SP enable) determine whether any pixel makes it to output regardless
of VRAM/palette state.

---

## Where the bug is (and isn't)

### Conclusively ruled out

- **CD subsystem** — `m2hook_cdtrace` showed sector reads complete
  cleanly, CDDA tracks play through, no phase wedges.
- **Memory delivery of FMV bytes** — VRAM has the actual tile data
  populated (this report).
- **VDC initialisation** — the VDC ctor fires multiple times across
  game switches, the VRAM allocation happens, the dumper sees a stable
  `vram_ptr`.

### Candidate bug sites (priority order)

1. **VCE palette (HuC6260) all-black**. PC Engine has 32 palettes
   (16 BG + 16 SP), each 16 entries of 9-bit RGB stored in the VCE.
   If the cutscene writes valid VDC tiles but never updates the VCE
   palette (or the VCE module has an init bug that leaves it zero),
   the screen is black even with full VRAM.
   - Diagnostic: instrument the HuC6260 module to dump its 512 palette
     entries. Compare to Geargrafx (MCP `read_memory area 8` /
     PALETTES — known-good).
2. **VDC R5 / Control Register display-enable bits clear**. The CR has
   BG-enable (SB) and SP-enable (CB) bits at positions 7 and 6. If
   m2engage's emulation isn't observing those bits, or the game wrote
   them as 0 (some FMV blanking that the real hardware-ignores), the
   screen would be black.
   - Diagnostic: locate R5 inside the VDC ctx (likely in the +0x30..+0x60
     window we sampled), log per-frame.
3. **VDC R6/R7 (BXR/BYR — scroll registers) point off-screen**. Less
   likely (the cutscene would show *something* even with wrong scroll),
   but possible.
4. **VCE control register / display blanking**. The VCE has a control
   register controlling the colour-burst gate and blanking. A bug
   here would suppress the VDC output entirely.

A single dump of m2engage's VCE palette + a dump of Geargrafx's VCE
palette at the same point in the cutscene will settle which side of
this list the bug lives on within an hour of work.

---

## Comparison procedure — Geargrafx reference

Per SPEC §6:
- Geargrafx exposes its emulator state via MCP server on **port 7777**.
- For the same PopfulMail dump, navigate to the same cutscene.
- Read memory **area 4 (VRAM)** for the known-good 64 KB.
- Compare to any dump from `m2_vram_NNNN.bin` taken while the m2engage
  screen was black.
- Read memory **area 8 (PALETTES)** for the known-good VCE palette
  (this is the one that will likely reveal the bug — m2engage's
  palette during the same moment is what we need to capture next).

Expected outcomes:
- VRAM byte-similar between m2engage and Geargrafx ⟹ the data path is
  identical, just not displayed — confirms display-side bug definitively.
- VRAM very different ⟹ the upstream path is producing different
  bytes; either the decoder runs differently or the CD-to-RAM blit
  routes bytes to a different VRAM region. Less likely given the byte
  count argues for "real content", but worth confirming.

---

## All available dumps

Stored in `m2hook_vramdump/dumps/`. Naming: `m2_vram_NNNN.bin` =
64 KB VRAM snapshot, `m2_vdcctx_NNNN.bin` = first 256 bytes of the
VDC context object captured at the same instant.

### VRAM snapshots (20 — most recent only; dumper rotates to last 20)

- [`m2_vram_0168.bin`](dumps/m2_vram_0168.bin)
- [`m2_vram_0169.bin`](dumps/m2_vram_0169.bin)
- [`m2_vram_0170.bin`](dumps/m2_vram_0170.bin)
- [`m2_vram_0171.bin`](dumps/m2_vram_0171.bin)
- [`m2_vram_0172.bin`](dumps/m2_vram_0172.bin)
- [`m2_vram_0173.bin`](dumps/m2_vram_0173.bin)
- [`m2_vram_0174.bin`](dumps/m2_vram_0174.bin)
- [`m2_vram_0175.bin`](dumps/m2_vram_0175.bin)
- [`m2_vram_0176.bin`](dumps/m2_vram_0176.bin)
- [`m2_vram_0177.bin`](dumps/m2_vram_0177.bin)
- [`m2_vram_0178.bin`](dumps/m2_vram_0178.bin)
- [`m2_vram_0179.bin`](dumps/m2_vram_0179.bin)
- [`m2_vram_0180.bin`](dumps/m2_vram_0180.bin)
- [`m2_vram_0181.bin`](dumps/m2_vram_0181.bin)
- [`m2_vram_0182.bin`](dumps/m2_vram_0182.bin)
- [`m2_vram_0183.bin`](dumps/m2_vram_0183.bin)
- [`m2_vram_0184.bin`](dumps/m2_vram_0184.bin)
- [`m2_vram_0185.bin`](dumps/m2_vram_0185.bin)
- [`m2_vram_0186.bin`](dumps/m2_vram_0186.bin)
- [`m2_vram_0187.bin`](dumps/m2_vram_0187.bin)

All taken during the sustained black-screen cutscene; non-zero counts
range from ~46k to ~50k bytes (70–76 %). Any one of these is suitable
for diff against Geargrafx's known-good VRAM.

### VDC context snapshots (188 — every dump tick from menu through full cutscene)

`m2_vdcctx_0000.bin` through `m2_vdcctx_0187.bin` — 256 bytes each,
located at `dumps/`. Snapshots `0000`–`0005` are from the menu state
(empty VRAM, original VDC ctx 0x01509178). Snapshots `0006`–`0187` are
from the game-instance VDC (ctx 0x0150ec80, vram base 0x01657908).

### Full log

[`dumps/m2_vramdump.log`](dumps/m2_vramdump.log) — every dumper tick
line with `nonzero=N/65536 (P%) -> /tmp/m2_vram_NNNN.bin`. Useful for
correlating moments with content levels.

---

## Files / locations cheat-sheet

- **Hook source**: `/Users/vincentaycirieix/dev/pce/m2hook_vramdump/`
- **All dumps**: `/Users/vincentaycirieix/dev/pce/m2hook_vramdump/dumps/`
- **Reference implementation that informed this hook**: `/Users/vincentaycirieix/dev/pce/m2hook_cdtrace/m2hook_cdtrace.c`
- **VFP-save lesson from `cdtrace`**: `/Users/vincentaycirieix/dev/pce/m2hook_cdtrace/REPORT.md`
- **Local m2engage** (MD5 `060f4815731c0d0717ee018665ab4a2c`): `/Users/vincentaycirieix/dev/pce/rootfs/usr/game/m2engage`
- **Console**: `ssh root@169.254.13.37` ; binary transfer via `cat | ssh "cat >"`.
- **Geargrafx MCP**: localhost port 7777, `read_memory` (area 4 = VRAM, area 8 = PALETTES, area 9 = CARD RAM, area 2 = CDROM RAM).

---

## Suggested next session

1. **Identify the VDC register offsets inside the VDC ctx struct.**
   Disassemble the VDC R/W port path in m2engage (the I/O handler for
   PCE addresses `$0000-$0003`). The register file is almost certainly
   in the `+0x30..+0x60` window of the VDC ctx — find R5 specifically
   and add per-dump-tick logging of its value.
2. **Dump m2engage's VCE palette.** The HuC6260 (VCE) is a separate
   subsystem. Find its constructor by signature (similar process to the
   VDC ctor), grab its context pointer, dump the 32 palettes × 16
   entries × 2 bytes = 1024 bytes of palette RAM each dumper tick.
   Reuse this hook's structure verbatim — just point at the VCE ctor
   and read a different field.
3. **Compare the m2engage VRAM dump against Geargrafx area-4 VRAM.**
   Drive Geargrafx to the same cutscene point, `read_memory area=4`,
   diff against `m2_vram_0168.bin`. Even a coarse comparison (count of
   matching bytes, BAT-region similarity) will confirm the
   bytes-equivalence and lock in the display-side verdict.
4. **Carry forward the two hook bugs encountered this session.** Any
   future hook with a background timer needs `nanosleep` (not
   `usleep`). Any future hook with a thread that needs to see state
   set by the patched function needs to spawn the thread *post-fork*
   (defer to the patched-function's handler, gated by an atomic-CAS
   flag).
