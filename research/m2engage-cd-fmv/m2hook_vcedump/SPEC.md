# m2hook_vcedump — Implementation Spec

`LD_PRELOAD` hook that periodically dumps m2engage's emulated **VCE palette
(CRAM)** on the PC Engine Mini — the final confirming step for the FMV
black-screen bug.

This is a **small extension of `m2hook_vramdump`** — read
`m2hook_vramdump/m2hook_vramdump.c` and `m2hook_vramdump/REPORT.md` first; this
spec only describes the delta.

---

## 1. Why this hook

Investigation status (memory `cd_fmv_compat.md`):

- `m2hook_vramdump` proved VRAM is **70-76 % populated** with real tile/BAT/SAT
  data during the black cutscene — the FMV image *is* in VRAM.
- Decoding vramdump's VDC-context snapshot showed the VDC register file at
  `VDC_ctx + 0x28` (R0) … `+0x4E` (R19), with **R5 = 0x00CC** (display ON,
  BG+SP enabled) — byte-identical to Geargrafx's working VDC registers.
- ⟹ Bug is in the **VCE (HuC6260 color encoder)**: VRAM has the image, the VDC
  display is enabled, yet the screen is black ⟹ **the VCE palette is black** (or
  the VCE is blanking output).

This hook dumps m2engage's VCE palette to confirm it, and captures the VCE
control register.

---

## 2. Target

Same binary (`m2engage`, MD5 `060f4815731c0d0717ee018665ab4a2c`, ET_EXEC,
PC Engine Mini).

m2engage merges the VDC + VCE into one module — the **"vdp/tg16" object** — it
owns `vram`, `cram`, `regs`, `satb` (adjacent state-save key strings
`w*:vram`/`w*:cram`/`w*:regs`/`w*:satb` at VMA `0x1fc2d0`/`d8`/`e0`/`e8`).
So the **CRAM (palette) buffer is reachable from the same context object**
`m2hook_vramdump` already captures at the VDC constructor `0x7aa08`
(`r0` = the module context).

Known layout of that context (from vramdump RE):
- `+0x14` : VRAM pointer (64 KB)
- `+0x28 … +0x4E` : VDC register file R0…R19
- Pointer-valued fields also seen at `+0x0c`, `+0x10`, `+0x18`, `+0x1c`,
  `+0x20`, `+0x24` — one of these is the **CRAM / palette buffer**.

---

## 3. Find the CRAM buffer

The implementing session must identify which context field holds the palette.
Two routes (use whichever lands first):

**Route A — state-save registration.** The VDC/VCE constructor (`0x7aa08` …
~`0x7ac80`) registers each buffer for save-states via `bl 0x713c0` calls
(`register(slot, buffer_ptr, name, size)` style — the VRAM one is at `0x7aa48`).
Find the `0x713c0` call whose name-string argument is **`w*:cram`
(VMA `0x1fc2d8`)** — its buffer-pointer argument is the CRAM, and the size
argument gives its length. Trace which `VDC_ctx + N` offset that buffer pointer
is stored to.

**Route B — empirical.** PCE VCE color RAM is **512 entries × 9-bit**, stored
16-bit = **1024 bytes**. In the dumper, dump a 2 KB window at the target of each
pointer field (`+0x0c/+0x10/+0x18/+0x1c/+0x20/+0x24`); the CRAM is the one that
holds 16-bit values all `≤ 0x01FF` (9-bit colors) and matches the shape of
Geargrafx's palette (MCP `read_memory area 8`).

---

## 4. The hook

Take `m2hook_vramdump.c` verbatim and change only the dumper tick:

- Keep the one-shot VDC-ctor hook at `0x7aa08` capturing the context.
- In the background-thread dumper tick, additionally:
  - read `cram_ptr` = the CRAM field identified in §3,
  - dump the palette (1024 bytes) to `/tmp/m2_cram_<NNNN>.bin`,
  - log: non-zero byte count of the palette (so "palette all-black" is visible
    at a glance), plus the VCE control register if its offset is known.

Carry forward the two vramdump lessons (already in that source):
`nanosleep` not `usleep`; spawn the thread post-`fork()` inside the ctor
handler via an atomic-CAS once-flag. No high-rate patch site → no VFP issue.

Deploy as `/usr/game/lib/m2hook_vcedump.so`, chained into `LD_PRELOAD` in
`/etc/init.d/gameapp`.

---

## 4b. IMPORTANT — new evidence: the "rare image flash"

The user reports that **very rarely a correct image flashes** on-screen during
the otherwise-black cutscene. This is decisive: a dead palette would give
*unbroken* black forever. A rare flash of a *correct* image proves the whole
pipeline (decoder → VRAM → VDC → VCE palette → display) **works end-to-end,
intermittently.** The palette is **not dead — it oscillates**, and the bug is a
**timing race**, not a broken path. The rareness (not 50/50 flicker) indicates
a **beat-frequency / per-frame-deadline-miss** pattern.

Consequence for this hook: a slow periodic buffer dump (every 2 s) would only
show "mostly black palette, occasionally populated" — confirming the oscillation
but not *who causes it*. The higher-value capture is to **trace the game's VCE
palette writes**, frame-correlated.

### Revised hook goal — trace VCE palette WRITES

Hook m2engage's **VCE I/O write handler** for PCE ports `$0402-$0405` (CTA /
CTW — color-table address and color-table write). Find it the same way the
cdrom code was found: m2engage's machine memory map registers an I/O handler
for the VCE register block; the `b:vcec` state-save string (`0x1fc300`) and the
VCE port range are the leads. The handler receives (port, value).

For each frame (coalesce by frame, using the VDC vblank or a frame counter):
log — number of CTW writes this frame, whether the written colors are all-zero
(black) or contain real 9-bit values, and the frame index.

## 5. Test & interpret — the decisive split

1. Boot the Mini, launch Popful Mail, sit on the black cutscene ~20 s, pull the
   log.
2. Reference: Geargrafx (MCP port 7777) `read_memory area 8` (PALETTES) during
   the working cutscene — full of non-zero 9-bit RGB values.

**The split that names whose clock is wrong:**

- **Game writes a correct, full palette nearly every frame, screen still black**
  → m2engage *applies/latches* the palette at the wrong time relative to display
  scanout → **m2engage VCE-timing bug** (m2engage's fault — fixable in
  m2engage's VCE emulation).
- **Game writes a black palette most frames, real palette only rarely**
  → the game's FMV decoder is mostly missing its per-frame deadline →
  **upstream deadline-miss** (Beetle-8×-CD-speed territory — the FMV's per-frame
  work isn't fitting in m2engage's emulated frame budget).

Either outcome names the subsystem *and* tells us if the bug is m2engage's
emulation or a timing-budget issue. A periodic CRAM buffer dump (the original
§4 plan) can still run alongside as corroboration — expect a mix of black and
populated snapshots, matching the flash.

---

## 6. After confirmation — the fix-hunt (next session, not this one)

Once the palette is confirmed black, the question becomes **why m2engage's VCE
palette-write path fails for FMV specifically** when it works for every other
game. Hypothesis to **test, not assume**:

> PCE FMV decoders blast a full palette every frame via a HuC6280 **block-
> transfer instruction** (TIA / TAI) targeting the VCE color-write port
> (`$0404/$0405`, CTW). If m2engage emulates that block-transfer variant's
> interaction with the VCE port incorrectly, the per-frame palette upload
> fails — while the VRAM tile blit (which vramdump proved works) uses a
> different transfer path. This would explain "VRAM populated, palette black."

Test it by tracing the game's writes to `$0402-$0405` during the cutscene
(another small hook on m2engage's VCE-port I/O handler), or by checking
m2engage's block-transfer-instruction emulation against the HuC6280 spec.
Do not patch on the hypothesis alone — confirm the mechanism first.

---

## 7. Deliverables

- `m2hook_vcedump.c`, `Makefile`/`build.sh`, `README.md`, `REPORT.md`.
- Update memory `cd_fmv_compat.md`.
