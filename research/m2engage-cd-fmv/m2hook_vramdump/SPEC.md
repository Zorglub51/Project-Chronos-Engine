# m2hook_vramdump — Implementation Spec

`LD_PRELOAD` hook that periodically dumps m2engage's emulated **VDC VRAM** on
the PC Engine Mini, to localize the FMV black-screen bug to **display-side vs
upstream**.

Self-contained. Reuses the proven mechanism from `m2hook_overdrive/` and
`m2hook_cdtrace/` — read those (especially `m2hook_cdtrace/m2hook_cdtrace.c`)
as reference implementations.

---

## 1. Why this hook

Investigation status (memory `cd_fmv_compat.md`, `m2hook_cdtrace/REPORT.md`):

- The FMV cutscene is **black on m2engage**, plays fine on Geargrafx (same game
  dump, byte-identical BIOS).
- `m2hook_cdtrace` proved m2engage's **CD subsystem is innocent** — phases
  healthy, FMV sectors stream, CDDA completes. The game streams 30+ FMV chunks
  (it is running its playback loop, not skipping).
- Geargrafx shows the cutscene renders via **completely standard VDC** (no
  exotic mode). m2engage renders standard VDC for every other game.

Deduction: **if the decoded FMV tiles reached m2engage's VRAM, m2engage would
display them.** Black screen ⟹ the tiles are not correctly in VRAM. This hook
settles the split:

- **VRAM contains the FMV image** → display-side bug (palette / display-enable).
- **VRAM empty/garbage** → upstream bug (decoder, or the CPU→VRAM blit, or CD
  data *content* — `cdtrace` verified phases, never the bytes).

---

## 2. Target binary

| Property | Value |
|---|---|
| Path on device | `/usr/game/m2engage` |
| Local copy | `/Users/vincentaycirieix/dev/pce/rootfs/usr/game/m2engage` |
| MD5 | `060f4815731c0d0717ee018665ab4a2c` |
| ELF type | `EXEC` (non-PIE — VMA == runtime address) |
| Device | PC Engine Mini, Allwinner A33, Linux 3.4.113 armv7l |

---

## 3. What to dump — m2engage's VDC VRAM

Established by disassembly of m2engage's **VDC (HuC6270) module constructor**:

- VDC constructor entry: **VMA `0x7aa08`** (file `0x72a08`). On entry
  `r0 = VDC context object` (immediately saved: `mov r7, r0`).
- It `operator new`s a 64 KB temp buffer (`0x71d2c`, size `0x10000`), fills it
  with the initial VRAM pattern, then **memcpy's it into the live VRAM** at
  `[VDC_context + 0x14]` (`0x7ab4c`), then frees the temp.
- Therefore the **live VDC VRAM buffer** is:

  ```
  vram_ptr = *(uint32_t *)(VDC_context + 0x14);   // 64 KB = 0x10000 bytes
  ```

PCE VDC VRAM is 32 K words = 64 KB — matches the `0x10000` allocation.

---

## 4. Hook strategy

Two parts — neither is a high-rate call site, so **no VFP-save needed** (unlike
`cdtrace`; see its REPORT for that lesson).

### 4a. Capture the VDC context (one-shot hook on the constructor)

Patch the VDC constructor entry `0x7aa08`. Locate by this **unique 16-byte
signature** (verified: 1 occurrence):

```
2d e9 f0 4f 07 46 87 b0 08 46 4c f2 c4 21 03 aa
```

Displaced bytes = first **8 bytes / 3 instructions** (`2d e9 f0 4f 07 46 87 b0`):
```
7aa08:  e92d 4ff0   stmdb sp!, {r4,r5,r6,r7,r8,r9,sl,fp,lr}
7aa0c:  4607        mov   r7, r0
7aa0e:  b087        sub   sp, #28
```
Resume address = `0x7aa10 | 1`.

Inline-trampoline (same as `m2hook_cdtrace.c`): the C handler receives `r0` =
the VDC context. Store it in a global:
```c
static volatile uint32_t g_vdc_ctx;
void vdcctor_handler(uint32_t r0){ g_vdc_ctx = r0; }
```
The constructor may run more than once (game switch) — just overwrite; the
latest is the live one.

### 4b. Periodic dumper (background thread)

In the `LD_PRELOAD` constructor, after installing the hook, start a `pthread`
that every ~2 s:
1. If `g_vdc_ctx` is set, read `vram_ptr = *(uint32_t*)(g_vdc_ctx + 0x14)`.
2. Sanity-check `vram_ptr` is a plausible heap pointer (non-null, not tiny).
3. Dump the 64 KB at `vram_ptr` to `/tmp/m2_vram_<NNNN>.bin` (incrementing
   counter), and also dump the first 256 bytes of `*g_vdc_ctx` to capture VDC
   register/state context.
4. Append a line to `/tmp/m2_vramdump.log`: counter, timestamp, `vram_ptr`,
   and a quick summary — count of non-zero bytes in the 64 KB (so a glance at
   the log shows "VRAM empty" vs "VRAM populated" without opening the .bin).

Keep only the last ~20 dumps (rotate) so tmpfs doesn't fill.

A background thread is used instead of hooking a per-frame function so there is
**no high-rate patched site** — avoids the `cdtrace` VFP-clobber class of bug
entirely.

---

## 5. Constructor / build / deploy

Same as `m2hook_cdtrace` (`SPEC.md` §6, build via `m2engage-cross` Docker):
- Locate `0x7aa08` by the §4a signature in the `m2engage` `r-xp` mapping;
  verify the 8 original bytes before patching; `mprotect` RWX; write the 8-byte
  `LDR.W PC,[PC,#0]` + trampoline redirect; `__builtin___clear_cache`.
- Link `-lpthread`.
- Deploy to the stick at `/usr/game/lib/m2hook_vramdump.so`; chain into
  `LD_PRELOAD` in `/etc/init.d/gameapp` (append — keep `m2hook_cdtrace.so` /
  `m2hook_print.so` / `probe_gl.so`).

---

## 6. Test procedure

1. With the hook preloaded, boot the Mini, launch **Popful Mail**, let it sit on
   the **black cutscene** for ~20 s, then skip/continue.
2. Pull `/tmp/m2_vramdump.log` and the `/tmp/m2_vram_*.bin` files.
3. From the log's non-zero-byte counts, pick the dump(s) taken while the screen
   was black.
4. Repeat for **Vasteel 2**.
5. **Reference:** in Geargrafx (MCP, port 7777) during the *working* cutscene,
   `read_memory` area 4 (VRAM) — that is the known-good VRAM content to compare
   against. Also area 8 (PALETTES) if a palette check is needed later.

---

## 7. Interpreting the result — the decisive split

- **m2engage VRAM during the black cutscene contains tile/BAT data** (non-zero,
  structured, resembling Geargrafx's VRAM):
  → the FMV image *is* in VRAM but not shown → **display-side bug**.
  Next: dump the VCE palette (HuC6260 module) and the VDC control register R5
  (display-enable) — the screen is black because the palette is black or the
  display is disabled.

- **m2engage VRAM is empty / all-zero / garbage** while Geargrafx's is
  populated:
  → the FMV image never reached VRAM → **upstream bug**.
  Next: split decoder vs blit vs CD-data-content —
  - extend `m2hook_cdtrace` to dump the **bytes** delivered per CD data
    transfer (not just phases) and compare to the CD image / Geargrafx CDROM
    RAM (area 2) + CARD RAM (area 9);
  - and/or trace the CPU→VDC VRAM-write path (the VWR port handler).

Either branch narrows the bug to a single subsystem with a clear next hook.

---

## 8. Deliverables

- `m2hook_vramdump.c`, `Makefile`/`build.sh`, `README.md`.
- `REPORT.md` with the non-zero-byte verdict and the chosen branch from §7.
- Update memory `cd_fmv_compat.md`.
