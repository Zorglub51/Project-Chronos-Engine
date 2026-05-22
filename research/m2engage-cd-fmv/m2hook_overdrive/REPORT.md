# m2engage `overdrive` — Investigation Report

Status: **observation hook built, deployed, opcode-44 path resolved, runtime
behaviour characterized**. Open: leaf-of-leaf semantics (what command
`0x1202` actually does inside `0x1ef160`).

Date: 2026-05-21.

---

## TL;DR

`overdrive` (the per-game `m2epi.version.GAME###.overdrive` field in
`title_prof.psb.m`) is **not** a "boost / performance" knob as initially
suspected. On the A33 / PC Engine Mini it acts as a **multiplier that
costs host CPU**, almost certainly an emulated-CPU cycle/clock multiplier:
host load scales roughly linearly with the value, and the Mini's
1 GHz Cortex-A7 falls behind quickly.

Measured behaviour on PCE Mini (JP retail, 1006JP stock m2engage):

| value | game            | arch       | result |
|------:|-----------------|------------|--------|
| 0     | Aldynes         | SGX/HuCard | normal (60 fps) |
| 2     | Aldynes         | SGX/HuCard | imperceptible |
| 2     | (CD games)      | tg16cd     | mild slowdown + audio stutter |
| 100   | PopfulMail      | tg16cd     | ~1–2 fps |
| 400   | Vasteel 2       | tg16cd     | super slow (effectively frozen) |

Stock pack ships **every game at overdrive=0** (`m2epi.overdrive=0` global,
no per-game override). The dev team explicitly forces `overdrive=0` during
replay playback (`utils.nut:1257` — comment: "リプレイ再生中はoverdriveを適用しない").

The knob is harmful on this hardware. Do not raise it.

---

## Background — why we investigated this

- We needed a way to make weak CD-arch games (Vasteel 2, PopfulMail) run
  better on the Mini, hypothesizing `overdrive` was a CPU-boost / accuracy
  knob from M2's emulator (analogous to Mednafen's `ocmultiplier`).
- Title-prof analysis identified `m2epi.overdrive` at the top level and
  `m2epi.version.GAME###.overdrive` per-game. `_init_emulator_get_option_by_regionTag`
  in `emulator.nut:668` reads per-game first, falls back to global, defaults to 0.
- C++ side exposes `setOverdrive` / `getOverdrive` / `setUseOverdrive` /
  `pauseOverdrive` in m2engage (Sqrat-bound). Static RE traced
  `setOverdrive` into a forwarder that calls into the M2Epi backend via a
  function-pointer dispatcher → leaf unknown without dynamic capture.
- Built `m2hook_overdrive.so` to observe that dispatcher in flight.

---

## What we built

**`/Users/vincentaycirieix/dev/pce/m2hook_overdrive/`** — pure-observer
`LD_PRELOAD` hook.

| File | Purpose |
|------|---------|
| `SPEC.md`          | Original spec (drove implementation) |
| `m2hook_overdrive.c` | Constructor + Thumb naked trampoline + C handler |
| `build.sh`         | Cross-compile via `m2engage-cross` Docker image |
| `Makefile`         | Thin wrapper around `build.sh` |
| `README.md`        | Build / deploy / log interpretation |
| `build/m2hook_overdrive.so` | Compiled ARM EABI5 .so (27,940 bytes) |
| `REPORT.md`        | This file |

### Hook mechanism

- Constructor scans m2engage's r-xp mapping for the 16-byte signature
  `0c b4 00 b5 83 b0 43 6a 23 b1 05 aa 08 46 04 99` (verified unique in
  the stock JP m2engage MD5 `060f4815731c0d0717ee018665ab4a2c`).
- Found at VMA `0x00034470` (the M2Epi opcode dispatcher).
- Patches the 8-byte prologue with `LDR.W PC, [PC, #0]` + trampoline
  address (the inline-veneer technique from `m2hook_print.c`).
- Trampoline saves r0–r3 + r12 + lr (24 bytes — 8-byte aligned), calls C
  handler with dispatcher args intact, restores, replays the 4 displaced
  Thumb instructions (`push {r2,r3}` / `push {lr}` / `sub sp,#12` /
  `ldr r3,[r0,#36]`), then `bx r12` to `0x34478 | 1` (Thumb resume).
- Pure observer: r0–r3 unchanged across the trampoline. Safe to leave
  armed during normal play.
- Logs to `/tmp/m2_overdrive_hook.log` on the Mini. Per-hit block on
  opcode-44 includes: backend ptr, handler ptr, value (signed + hex),
  `*(backend+0x24)` leaf ptr, backend header bytes `[0..0x30]`, and a
  4 KB code dump of the leaf for offline disassembly. Also logs an
  opcode-frequency histogram at process exit (atexit), useful as a sanity
  check if no opcode-44 hits are seen.

### Deployment

- Stick is at `/Volumes/CHRONOS` on Mac, plugged into console at runtime.
- `.so` lives at `/usr/game/lib/m2hook_overdrive.so` on the console
  (which is bind-mounted from `/mnt/usb/game/lib/m2hook_overdrive.so` on
  the stick). Source: `/Users/vincentaycirieix/dev/pce/m2hook_overdrive/build/m2hook_overdrive.so`.
- `/etc/init.d/gameapp` (custom USB-aware wrapper) was patched to chain
  it into `LD_PRELOAD` alongside `m2hook_print.so` + `probe_gl.so`. The
  added line (after the probe_gl line):
  ```sh
  [ -f ${GAME_HOME}/lib/m2hook_overdrive.so ] && preload=${preload:+${preload}:}${GAME_HOME}/lib/m2hook_overdrive.so
  ```
  Backup of original gameapp at `/etc/init.d/gameapp.bak`.

### Restart procedure

The wrapper has no internal loop — `StartGame &` runs once. To pick up
edits to the wrapper or restart cleanly:

```sh
ssh root@169.254.13.37 '
  : > /tmp/m2_overdrive_hook.log     # reset hook log
  touch /tmp/.game.exit              # tell wrapper this kill is intentional
  killall -9 m2engage 2>/dev/null
  sleep 1
  rm -f /tmp/.game.exit
  setsid /etc/init.d/gameapp start </dev/null >/dev/null 2>&1 &
'
```

The SSH session terminates while m2engage dies (signal propagation
through PTY — expected; the relaunch still happens). Verify
afterwards with `ps | grep -E "gameapp|m2engage"`.

---

## Static call chain (re-confirmed dynamically)

```
EmuTask::setOverdrive(N)                 VMA 0x23100
  ├─ str N -> [EmuTask + 0xEC]
  ├─ logs "console.overdrive=N"
  └─ bl 0x3549C            (opcode-44 forwarder)
        ├─ r1 = [EmuTask->[0xF4]]->[8]   ; GATE 1 (handler null check)
        ├─ cbz r1 -> return
        ├─ r0 = [EmuTask->[0xF4]]->[4]   ; backend
        ├─ r2 = 44                        ; opcode
        └─ b 0x34470          (DISPATCHER  ← we hook this)
              ├─ r3 = [r0 + 0x24]         ; leaf fn ptr
              ├─ cbz r3 -> return         ; GATE 2
              └─ blx r3                   ; → leaf @ 0x001ed5f0  (resolved dynamically)
```

At dispatcher entry: `r0=backend, r1=handler, r2=opcode, r3=value`.

The hook captured opcode-44 hits with `*(backend+0x24) = 0x001ed5f1`
across every observed call (HuCard, CD, value=0/2/100/400) — i.e. the
**leaf is constant for this build** at VMA `0x001ed5f0`.

---

## Leaf analysis — `0x001ed5f0`

A 45-entry tbb-based opcode-dispatch function. Disassembled prologue:

```
1ed5f0:  subs r1, #1
1ed5f2:  push {r4, lr}
1ed5f4:  mov  r4, r2          ; r4 = value pointer (saved)
1ed5f6:  sub  sp, #264        ; 264-byte local frame (= 0x108)
1ed5f8:  cmp  r1, #44         ; bound check
1ed5fa:  bhi  0x1ed63a        ; out of range → return
1ed5fc:  tbb  [pc, r1]        ; jump table indexed by (opcode - 1)
```

Jump-table bytes (offsets in halfwords from 0x1ed600):

```
1ed600: 87 81 81 1d 7e 7b 75 6e 68 1d 65 17 5e 1d 58 1d
1ed610: 1d 1d 52 1d 1d 1d 1d 1d 4e 4a 46 41 3b 30 29 1d
1ed620: 1d 1d 1d 1d 1d 1d 1d 1d 1d 1d 1d 1f
                                          ^^
                                          index 43 → opcode 44
```

Byte `0x1f` at index 43 → target = `0x1ed600 + 2*0x1f` = **`0x1ed63e`**.

Many indices share offset `0x1d` (= target `0x1ed63a`, the bare
"return" tail) — those opcodes are no-ops in this leaf. The non-`1d`
entries fall into clusters that suggest the leaf is a generic
"emulator-property" command handler, with each opcode reading/writing
one property of the backend.

### Case 44 — the actual code

```
1ed63e:  ldr  r2, [r2, #0]        ; r2 = *value_ptr  → load overdrive int
1ed640:  movs r3, #0
1ed642:  str  r3, [sp, #8]        ; local_zero = 0
1ed644:  movw r1, #0x1202         ; sub-command code (4610)
1ed648:  ldr  r0, [r0, #4]        ; r0 = *(arg0 + 4)
1ed64a:  add  r3, sp, #8          ; r3 = &local_zero
1ed64c:  bl   0x1ef160            ; ← forwards to the real handler
1ed650:  b    0x1ed63a            ; return tail
```

So opcode-44's job is a **thin forwarder** that calls function
`0x1ef160` with:

```
r0 = *(arg0 + 4)        ; arg0 was r0 at leaf entry — origin still TBD
r1 = 0x1202   (= 4610)  ; the real command code
r2 = the overdrive int  ; passed by value (post-deref)
r3 = &zero              ; out-param / sentinel
```

That puts the actual semantics in **`0x1ef160`'s case for command 0x1202**.
We did not disassemble that function. It's the natural next step.

---

## Runtime captures

Excerpt from `/tmp/m2_overdrive_hook.log` (annotated):

```
hit #1  value=2     backend=0x0152ade4 backend[+04]=0x12 (tg16cd arch) — init / state restore
hit #2  value=0     backend=0x0152ad9c backend[+04]=0x0a (HuCard arch) — HuCard launch
hit #3  value=2     backend=0x0152ade4 backend[+04]=0x12 — PopfulMail (overdrive=2 then)
hit #4  value=0     backend=0x01510a9c backend[+04]=0x0a — Aldynes (overdrive=0)
hit #5  value=1     backend=0x0152ade4 backend[+04]=0x12 — Vasteel 2 (overdrive=1 then)

— wrapper restart with new title_prof (overdrive 2/100/400) —

hit #1  value=0     backend=0x0151f6b4 backend[+04]=0x0a — engine init
hit #2  value=2     backend=0x0151f7ac backend[+04]=0x0a — Aldynes launch (no perceptible change)
hit #3  value=400   backend=?          backend[+04]=0x12 — Vasteel 2 (super slow / frozen)
hit #4  value=100   backend=?          backend[+04]=0x12 — PopfulMail (~1–2 fps)
```

Key observations:

- `backend[+0x04]` differs by arch: **`0x0a`** for HuCard/SGX, **`0x12`**
  for tg16cd. Likely an arch ID or initial cycle count.
- The leaf fn ptr `0x001ed5f1` is identical across all backends — the
  per-instance differentiator is the backend struct itself (different
  `m_handler` member at +0x18 across hits), not the dispatcher leaf.
- The value passes through unmodified — no clamping at the leaf. The
  4610-command receiver must do whatever scaling/bounds happen.
- Effect scales roughly linearly with value: ~normal at 0–2, ~1 fps at
  100, ~unusable at 400. Confirms it's a **workload knob**, not a
  budget/divisor.

---

## Open questions

1. **What does command `0x1202` (4610) actually do inside `0x1ef160`?**
   That is the literal answer to "what does overdrive do."
   - First step: disassemble m2engage at file offset `0x1e7160` (= VMA
     `0x1ef160 - 0x8000` load base). Use `arm-linux-gnueabihf-objdump
     -D -b binary -m armv7 -M force-thumb --start-address=0x1e7160 ...`
     against
     `/Users/vincentaycirieix/dev/pce/rootfs/usr/game/m2engage`.
   - `0x1ef160` is itself almost certainly another opcode dispatcher
     (the 12-bit `movw` constant `0x1202` looks like a sub-namespace
     marker — 0x12 = backend "tg16cd" perhaps, with 0x02 as the
     sub-opcode inside it; or it's a flat command ID).
   - If static analysis is hard, build a v2 hook against `0x1ef160`
     and log per-call (r0, r1, r2, r3) so we can confirm the calling
     convention and capture the sub-leaf address. The same trampoline
     skeleton from `m2hook_overdrive.c` should apply with minor changes
     (different displaced instructions; signature-scan a unique 16-byte
     window at `0x1ef160` to be safe).

2. **Why is the effect arch-specific?** HuCard at value=2 was
   imperceptible; CD at value=2 stuttered noticeably. Two hypotheses:
   - Overdrive scales the **emulated CPU** cycles-per-frame. HuCard CPU
     emulation is light; CD systems also emulate ADPCM, CD-DA mixing,
     and SCSI timing — the *additional* cycles forced into those
     subsystems hit hard.
   - Overdrive is **arch-specific code** entirely, applying only on
     CD-arch backends (the leaf's `ldr r0, [r0, #4]` could pull an
     arch-specific sub-object).

3. **What's the right `overdrive` value for an A33-class host, if any?**
   Empirically: anything > 0 on CD-arch games hurts. The default `0` is
   correct for this hardware. Useful range is probably only on
   significantly faster hosts (Pi5, desktop).

4. **Does `setUseOverdrive` / `pauseOverdrive` matter?** We only hit
   `setOverdrive` (opcode 44). The other natives may route through
   different opcodes (24/25 in the table point to non-`1d` targets:
   `0x4e`, `0x4a`). Worth a v2-hook capture if pursuing this further.

---

## Reverting the experimental edits

The runtime `title_prof.psb.m` currently has:

- GAME04 (Aldynes)    : overdrive=2
- GAME07 (Vasteel 2)  : overdrive=400
- GAME08 (PopfulMail) : overdrive=100

These are in the **on-stick published copy** at:
`/Volumes/CHRONOS/library/published/folders/jp/_root/title_prof.psb.m`
(which is what `enterGameFolder` bind-swaps onto the active
`/usr/game/040/config/title_prof.psb.m` at lineup entry).

To revert:

```sh
# Pull from console (or read directly from stick when plugged into Mac)
ssh root@169.254.13.37 'cat /mnt/usb/library/published/folders/jp/_root/title_prof.psb.m' > /tmp/tp.psb.m
python3 /Users/vincentaycirieix/dev/pce/tools/psbtool/mzstool.py extract /tmp/tp.psb.m
# Edit /tmp/title_prof/title_prof.json — delete "overdrive" key from GAME04, GAME07, GAME08
python3 /Users/vincentaycirieix/dev/pce/tools/psbtool/mzstool.py build /tmp/title_prof/title_prof.json
cat /tmp/title_prof/title_prof.psb.m | ssh root@169.254.13.37 \
    'cat > /mnt/usb/library/published/folders/jp/_root/title_prof.psb.m && sync'
# Then restart m2engage per the procedure above.
```

---

## Files / locations cheat-sheet

- **Hook source / build**: `/Users/vincentaycirieix/dev/pce/m2hook_overdrive/`
- **Reference impl (existing hook with same patch technique)**: `/Users/vincentaycirieix/dev/pce/m2hook_pce/m2hook_print.c`
- **Cross-compile toolchain**: Docker image `m2engage-cross` (built by `m2engage-mac/build-a33.sh`)
- **Local m2engage binary** (matches console — MD5 `060f4815731c0d0717ee018665ab4a2c`): `/Users/vincentaycirieix/dev/pce/rootfs/usr/game/m2engage`
- **Static RE notes**: `/Users/vincentaycirieix/dev/pce/re/`
- **Mini access**: `ssh root@169.254.13.37` (no password). Binary transfer: `cat file | ssh root@169.254.13.37 "cat > /path"` (scp -O unreliable).
- **Console-side log path**: `/tmp/m2_overdrive_hook.log` (tmpfs — pull before reboot)
- **Stock script `emulator.nut`** (overdrive caller — unmodified by mods): `/Users/vincentaycirieix/dev/pce/game/system/script/emulator.nut` lines 631, 668, 725-726, 907-916
- **`utils.nut` (mod-patched, forces overdrive=0 during replay)**: `/Users/vincentaycirieix/dev/pce/Project-Chronos-Engine/mod-assets/scripts-src/utils.nut:1256-1257`

---

## Suggested next session

1. **Disassemble `0x1ef160`** (file offset `0x1e7160` in m2engage). Find
   the case for command `0x1202`. Either it's another tbb-style
   dispatcher (look for `cmp r1, #...` + `tbb`/`tbh` after the prologue),
   or a tree of `cmp`/`beq`. Follow case 0x1202 to its leaf — that is
   the final answer.
2. If static analysis stalls, build `m2hook_overdrive_v2.so` that hooks
   `0x1ef160` (signature-scan a 16-byte window from that address in the
   local binary first to confirm uniqueness), logging (r0, r1, r2, r3)
   on entry and on each blx within. Reuse the trampoline skeleton from
   `m2hook_overdrive.c`.
3. Once the leaf semantics are known, decide whether overdrive has any
   useful application on the A33 or if it's purely a "stronger host"
   feature. If the latter, document in the wiki that the field exists
   but should always be 0 on this hardware, and consider removing the
   field from the publisher / editor surface to prevent accidents.
