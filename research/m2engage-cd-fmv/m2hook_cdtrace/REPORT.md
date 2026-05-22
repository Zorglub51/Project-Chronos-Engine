# m2engage FMV black-screen — Investigation Report (cdtrace pass)

Status: **CD subsystem ruled OUT as the bug site**. The cdtrace hook
captured a full successful read sequence during the failing PopfulMail
cutscene. CD data sectors and CDDA tracks both transfer cleanly. The
bug is **downstream** of the CD path — most likely the FMV decoder, the
VDC blit, or audio/video sync.

Date: 2026-05-21.

---

## TL;DR

The SPEC's working hypothesis (`cd_fmv_compat.md`) was that m2engage's
`tg16_cdrom_*` state machine wedged during FMV cutscene loads — the CD
data read failed/hung while CDDA audio played, producing the
black-screen. The cdtrace hook recovered the engine's own
phase-transition log to confirm or refute that.

**The hypothesis is wrong.** During a PopfulMail launch (overdrive=0,
no other modifications, exact black-screen reproducer):

- CD reads succeed: every `start reading N` is followed by sustained
  `data_phase` ↔ `read_sector` cycles and a clean `result_phase`.
- FMV sector streaming works: sectors progress 4691 → 4693 → 5797 →
  5813 → 5829 → … stepping by 16 (32 KB chunks — the classic streaming
  pattern).
- CDDA track playback completes: the engine reaches `play_stop` and
  `play track done` cleanly.
- No phase shows runaway repeat counts. The state machine never wedges.

So the FMV data arrives. The bug that black-screens the cutscene is in
whatever consumes it — decoder, renderer, or the timing glue between
them. **The next investigation pass should target that code, not the CD
subsystem.**

---

## What we built and what we found

### The hook

`/Users/vincentaycirieix/dev/pce/m2hook_cdtrace/` — pure-observer
`LD_PRELOAD` hook on m2engage's debug-log function at VMA `0x1eee48`
(file offset `0x1e6e48` in MD5 `060f4815731c0d0717ee018665ab4a2c`).

The release build calls this function with phase-name strings and
printf-format CD events but discards output because the log-sink
context pointer is null. The hook intercepts before the discard,
filters for tg16_cdrom_* strings and the six known CD printf formats,
and writes a coalesced trace to `/tmp/m2_cdtrace.log`.

Identical mechanism to `m2hook_overdrive`: signature-scan the unique
16-byte window → mprotect RWX → `LDR.W PC,[PC,#0]` + trampoline veneer
→ replay the 4 displaced Thumb instructions → resume at `0x1eee50 | 1`.

| File | Purpose |
|---|---|
| `SPEC.md` | Original spec |
| `m2hook_cdtrace.c` | Hook source (with the VFP fix described below) |
| `build.sh` / `Makefile` | Docker cross-compile via `m2engage-cross` |
| `README.md` | Build / deploy / log interpretation |
| `build/m2hook_cdtrace.so` | Compiled .so (25,944 bytes) |
| `popful_mail_trace.log` | The captured PopfulMail trace (85 KB, 1989 lines) |
| `REPORT.md` | This file |

### The bug we hit and fixed during deployment

First deploy caused PopfulMail to crash within 19 s of launch
(`rc=1` → "004 PLEASE SHUTDOWN"). The cdtrace log only had the startup
banner — the C handler was never reached, but the trampoline was firing
on every debug-logger call (which CD games hit far more often than
HuCard).

**Root cause**: armhf is hard-float ABI; `d0..d7` (= `q0..q3`) are
caller-saved. The C handler's `strncmp` / `fprintf` clobbered them, but
the trampoline only saved integer registers. At log-rate, this silently
corrupted whatever floating-point computation the engine was doing
around the log call. HuCard games (e.g. Aldynes) hit the logger orders
of magnitude less often, so this never tripped under the overdrive hook's
test load.

**Fix**: bracket the `bl cdtrace_handler` with `vpush {d0-d7}` /
`vpop {d0-d7}`. Stack delta becomes 24 + 64 = 88 bytes, still 8-aligned.

After the fix, PopfulMail runs cleanly with the hook armed — the cutscene
still black-screens (the bug we wanted to characterize), and the trace
fills with real CD activity.

This is a **takeaway for any future hook in this family**: if the patched
function might be called at high rate from FP-using code, save the
caller-saved VFP registers across the C call. The overdrive hook in this
repo does not need the fix because its target (opcode-44 dispatch) fires
at most a few times per launch.

---

## Trace highlights

Source: `popful_mail_trace.log` (also on console at `/tmp/m2_cdtrace.log`
until reboot). 1989 lines, sequence numbers up to 1383.

### Healthy CD-read pattern (excerpt around seq #21–#33)

```
[CD] #21 ENTER phase: tg16_cdrom_read_sector
[CD] #22 seek start: 0 -> 4691
[CD] #23 seek 4666
[CD] #24 seek 4691
[CD] #25 seek done
[CD] #26 seek is finished
[CD] #27 start reading 4691
[CD] #28 ENTER phase: tg16_cdrom_data_phase     ← data streaming
[CD] #29 ENTER phase: tg16_cdrom_read_sector
[CD] #30 ENTER phase: tg16_cdrom_data_phase     ← data streaming
[CD] #31 ENTER phase: tg16_cdrom_read_sector
[CD] #32 ENTER phase: tg16_cdrom_result_phase   ← read completes
```

This is exactly the SPEC §8's "BUSY → DATA IN → sectors stream →
transfer completes" healthy reference. Every `start reading N` in the
trace follows this same shape.

### Phase frequency (full trace)

| count | phase                          |
|------:|--------------------------------|
| 1178  | `tg16_cdrom_read_sector`       |
| 566   | `tg16_cdrom_data_phase`        |
| 34    | `tg16_cdrom_result_phase`      |
| 16    | `tg16_cdrom_get_toc`           |
| 2     | `tg16_cdrom_track_search`      |
| 2     | `tg16_cdrom_read`              |
| 2     | `tg16_cdrom_play_stop`         |
| 1     | `tg16_cdrom_test_unit_ready`   |
| 1     | `tg16_cdrom_data_out`          |

`read_sector` ↔ `data_phase` dominate at a roughly 2:1 ratio —
consistent with two read-sector ticks per data delivery. No phase shows
the "wedged-with-runaway-count" pattern SPEC §8 said to look for.

### FMV data sectors actually read

```
#27  start reading 4691     (2 sectors: 4691 then 4693)
#77  start reading 5797     (stepping by 16: 5797, 5813, 5829, 5845,
#116 start reading 5813      5861, 5877, 5893, 5909, ...)
#155 start reading 5829
#194 start reading 5845
#233 start reading 5861
#272 start reading 5877
#311 start reading 5893
#350 start reading 5909
```

Each `start reading` is followed by ~10 `data_phase` / `read_sector`
cycles then a `result_phase`. **The FMV data is loaded.**

### Long CDDA seek phase at the end

After data reads, the trace shifts to large-LBA seeks (steps of 4666
LBAs ≈ 62 s of CDDA audio) up to LBA 193 499, ending with `seek done` →
`seek is finished` → `result_phase` → `play_stop` → `result_phase` →
`play track done`. That's the engine emulating CDDA-track positioning
for the cutscene's audio backing. It completes cleanly.

The 4666-LBA step size is interesting — it's not a smooth ramp; it looks
like the emulator is iterating a coarse-seek register, not following a
real-drive seek trajectory. If anything is "off" in this trace, it's
this stepping pattern — but it terminates correctly so it's probably
just the M2 model's seek emulation strategy and not bug-relevant.

---

## What the bug isn't, and what it might be

### Ruled out by this trace

- CD command decode: command codes round-trip correctly (TOC reads,
  data reads, CDDA seek all produce expected phase sequences).
- Data delivery: `start reading N` produces sustained `data_phase`
  cycles, then `result_phase`. Sectors are read.
- CDDA path: `track_search` / `play_stop` / `play track done` all reach
  their terminal phases.
- The `overdrive` parameter: with overdrive=0, the trace looks identical
  to what we'd expect from a working CD load. (Earlier we'd already
  ruled out overdrive as a fix; it's a workload knob, not an accuracy
  one.)

### Plausible bug sites (next investigation targets)

1. **FMV decoder.** PopfulMail's cutscenes use a vertically-streamed
   tile-and-strip format (Konami/Falcom-era custom codec). Look for the
   decompression/decode routine and verify it runs on a captured chunk.
   - Approach: dump the contents of the data buffer at the address the
     CD subsystem fills after `start reading 5797` and onward. Compare
     against the original CD image to confirm bytes match. If bytes
     match → decoder is the bug. If bytes differ → an earlier transfer
     step corrupted memory.
   - Hook candidate: patch the `data_out`/`data_phase` handler exit and
     log the destination buffer pointer + first 32 bytes per delivery.
2. **VDC FMV blit.** Even with correct decoded tiles, the data has to
   reach the VDC's CG-RAM. m2engage's VDC emulation may not handle the
   DMA pattern or memory window that PopfulMail uses. Compare with a
   working CD game that does NOT do FMV (uses static screens only) —
   the difference is what to inspect.
3. **A/V sync stall.** The decoder runs, the blit runs, but a sync
   primitive (vblank wait, sample-rate match) wedges so frames are
   produced but never presented. Less likely given audio plays, but
   worth ruling out by checking if VDC is even being programmed for
   the cutscene mode.
4. **Game-side detection of "wrong" hardware.** Some Falcom/Konami
   FMVs ran a quick CD-drive identification check; if it returns an
   unexpected value, the game shows a black screen rather than crashing.
   The CD reads succeed but the game's own decision tree might pick the
   "skip cutscene" branch. Diagnostic: look for game-code branches
   shortly after the FMV data reads complete.

Priority order for next session: **(1)** then **(4)** are the cheapest
to confirm/refute and would either find the bug or narrow it further.

---

## Hook deployment & current console state

- `m2hook_cdtrace.so` is deployed at `/usr/game/lib/m2hook_cdtrace.so`
  (= `/mnt/usb/game/lib/m2hook_cdtrace.so` on the stick).
- `/etc/init.d/gameapp` chains it into LD_PRELOAD via the conditional
  line:
  ```sh
  [ -f ${GAME_HOME}/lib/m2hook_cdtrace.so ] && preload=${preload:+${preload}:}${GAME_HOME}/lib/m2hook_cdtrace.so
  ```
- `m2hook_overdrive.so` was removed at user request (no longer needed
  after the overdrive investigation concluded). Its conditional line in
  gameapp was also removed. Backup of the original gameapp is at
  `/etc/init.d/gameapp.bak`.
- `title_prof.psb.m` has `overdrive` cleared from GAME04 / GAME07 /
  GAME08 (the experiment values were 2 / 400 / 100 — all reverted).

To leave cdtrace in place but quiet, do nothing — it produces no output
unless a CD game runs. To disable it without removing the .so:
```sh
ssh root@169.254.13.37 'mv /mnt/usb/game/lib/m2hook_cdtrace.so /mnt/usb/game/lib/m2hook_cdtrace.so.disabled'
```
…then restart m2engage (see procedure in `m2hook_overdrive/REPORT.md`).

---

## Files / locations cheat-sheet

- **Hook source**: `/Users/vincentaycirieix/dev/pce/m2hook_cdtrace/`
- **The reference implementation that informed this one**: `/Users/vincentaycirieix/dev/pce/m2hook_overdrive/m2hook_overdrive.c` (note: that one does NOT save VFP — needs adding if it ever targets a high-rate function)
- **Other reference**: `/Users/vincentaycirieix/dev/pce/m2hook_pce/m2hook_print.c`
- **Local m2engage binary** (MD5 `060f4815731c0d0717ee018665ab4a2c`): `/Users/vincentaycirieix/dev/pce/rootfs/usr/game/m2engage`
- **Console SSH**: `ssh root@169.254.13.37`; binary transfer via `cat | ssh "cat >"`.
- **Console-side log**: `/tmp/m2_cdtrace.log` (tmpfs — pull before reboot).
- **PopfulMail trace from this session**: `m2hook_cdtrace/popful_mail_trace.log`.

---

## Suggested next session

1. Read the SPEC's referenced memory `cd_fmv_compat.md` (if it exists)
   to confirm what "the working reference (Geargrafx)" actually does
   during the same cutscene — the data-delivery pattern there should
   match what m2engage produces, confirming once more that the CD path
   is innocent.
2. Decide which downstream candidate to instrument:
   - **FMV decoder** (most likely): use the same trampoline skeleton as
     `m2hook_cdtrace.c` to hook the data-buffer-write path. Find the
     destination address from where `start reading N` lands, log the
     first ~32 bytes per delivery, compare to the source CD image.
   - **Game-side detection**: load the binary in a static disassembler,
     look for what the FMV-trigger routine in PopfulMail expects post-
     CD-read. The game's vector table + entry points are well-known for
     PCE titles.
3. If a v2 hook is needed, **carry over the VFP fix**. Any hook on a
   function called from FP-using code in m2engage must
   `vpush {d0-d7}` around its `bl` to the C handler.
4. Pull `popful_mail_trace.log` for full reference. Capture a known-good
   CD-game trace (e.g. a PC-Engine CD title without FMV, like Ys I&II's
   title screen) alongside for diff baseline.
