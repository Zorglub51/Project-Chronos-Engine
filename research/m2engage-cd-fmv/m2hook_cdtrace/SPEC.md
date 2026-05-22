# m2hook_cdtrace — Implementation Spec

`LD_PRELOAD` observer hook that traces m2engage's **CD-ROM subsystem activity**
on the PC Engine Mini, to diagnose the FMV-cutscene black-screen bug in
Popful Mail and Vasteel 2.

Self-contained. Reuses the proven mechanism from `m2hook_overdrive/` and
`m2hook_pi5/m2hook_print.c` — read those as reference implementations.

---

## 1. Why this hook

Established by investigation (see memory `cd_fmv_compat.md`):

- Both games' cutscenes use the **same pattern**: a black "loading" screen
  during which the game issues a **CD data-sector read**, then playback from
  loaded data. Audio is CD-DA; rendering is standard VDC.
- On the **working reference (Geargrafx)** the CD load completes:
  `scsi_phase` BUSY → DATA IN (sectors stream, `sectors_left` counts down) →
  transfer completes → cutscene plays.
- On **m2engage** the cutscene stays black while CD-DA audio plays — i.e. the
  CD **data** load fails/hangs. The bug is in m2engage's `tg16_cdrom_*` data
  path. Every other factor (dumps, BIOS, VDC, CPU/overdrive) is ruled out.

This hook recovers m2engage's own CD phase log so we can see **where its CD
state machine diverges** from the known-good sequence.

### The free instrumentation

m2engage's CD state machine already logs every phase transition by calling the
debug logger at **VMA `0x1eee48`** with a `tg16_cdrom_*` phase-name string.
But `0x1eee48` **discards output when the log sink is null** (release build on
the Mini — confirmed by disassembly: it calls `(*(*ctx))[4]` only if non-null,
else returns). So the phase logs are computed and thrown away. Hooking
`0x1eee48` recovers them — no CD state-machine RE required.

---

## 2. Target binary

Same binary as the other hooks. Verify before patching.

| Property | Value |
|---|---|
| Path on device | `/usr/game/m2engage` |
| Local copy | `/Users/vincentaycirieix/dev/pce/rootfs/usr/game/m2engage` |
| MD5 | `060f4815731c0d0717ee018665ab4a2c` |
| ELF type | `EXEC` (non-PIE, fixed load — VMA == runtime address) |
| Device | PC Engine Mini, Allwinner A33, Linux 3.4.113 armv7l |

---

## 3. Hook point — the debug logger `0x1eee48`

Locate by this **unique 16-byte signature** (verified: 1 occurrence):

```
0e b4 00 b5 82 b0 03 aa 52 f8 04 1b 01 92 18 b1
```

Logger signature (from disassembly):
`log(r0 = ctx, r1 = format_string, r2 = arg0, r3 = arg1, ...)` — varargs.

Displaced bytes = first **8 bytes / 4 instructions** (`0e b4 00 b5 82 b0 03 aa`):
```
1eee48:  b40e   push {r1, r2, r3}
1eee4a:  b500   push {lr}
1eee4c:  b082   sub  sp, #8
1eee4e:  aa03   add  r2, sp, #12
```
Resume address after the trampoline = `0x1eee50 | 1`.

---

## 4. Mechanism — inline patch + trampoline

Identical technique to `m2hook_overdrive` (see `m2hook_overdrive/SPEC.md` §4 and
the implemented `m2hook_overdrive.c`). Patch `0x1eee48` with
`LDR.W PC,[PC,#0]` + `.word trampoline`; trampoline:

```
push {r0-r3, r12, lr}        ; preserve logger args (24 B, 8-aligned)
bl   cdtrace_handler          ; C handler, receives (r0,r1,r2,r3) intact
pop  {r0-r3, r12, lr}
; replay the 4 displaced instructions:
push {r1, r2, r3}
push {lr}
sub  sp, #8
add  r2, sp, #12
; resume in the logger at 0x1eee50
ldr  r12, =resume_addr        ; (found + 8) | 1
bx   r12
```

Pure observer. The C handler must not alter r0-r3 as seen by the logger
(the trampoline save/restore guarantees this).

---

## 5. C handler

```c
// rodata bounds (for safe pointer validation)
#define RODATA_LO 0x001f6eb8u
#define RODATA_HI 0x003dcb0cu

// cdrom phase-name string block
#define CDPHASE_LO 0x001fc4b8u
#define CDPHASE_HI 0x001fc618u

static FILE *g_log;
static unsigned g_seq;
static char     g_last_phase[40];
static unsigned g_phase_repeat;

// CD-related printf format strings worth capturing with their args
static const struct { uint32_t vma; } g_cd_fmts[] = {
    {0x003dc208},  // "start reading %d"
    {0x003dc21c},  // "seek start: %d -> %d"
    {0x003dc234},  // "play track done"
    {0x003dc244},  // "seek %d"
    {0x003dc24c},  // "seek done"
    {0x001fc894},  // "seek is finished"
};

static int in_rodata(uint32_t p){ return p>=RODATA_LO && p<RODATA_HI; }

static void flush_phase(void){
    if (g_phase_repeat){
        fprintf(g_log, "[CD] phase: %-26s x%u\n", g_last_phase, g_phase_repeat);
        g_phase_repeat = 0;
    }
}

void cdtrace_handler(uint32_t r0, uint32_t r1, uint32_t r2, uint32_t r3)
{
    // (a) phase-name log: r2 points at a tg16_cdrom_* string
    if (r2 >= CDPHASE_LO && r2 < CDPHASE_HI){
        const char *name = (const char *)r2;
        // coalesce consecutive identical phases
        if (strncmp(name, g_last_phase, sizeof g_last_phase) == 0){
            g_phase_repeat++;
        } else {
            flush_phase();
            strncpy(g_last_phase, name, sizeof g_last_phase - 1);
            g_last_phase[sizeof g_last_phase - 1] = 0;
            g_phase_repeat = 1;
            fprintf(g_log, "[CD] #%u ENTER phase: %s\n", ++g_seq, name);
            fflush(g_log);
        }
        return;
    }

    // (b) CD format-string log lines (carry sector numbers etc.)
    for (unsigned i = 0; i < sizeof g_cd_fmts/sizeof g_cd_fmts[0]; i++){
        if (r1 == g_cd_fmts[i].vma){
            flush_phase();
            // r1 is a printf format; render with up to two int args
            fprintf(g_log, "[CD] #%u ", ++g_seq);
            fprintf(g_log, (const char *)r1, (int)r2, (int)r3);
            fputc('\n', g_log);
            fflush(g_log);
            return;
        }
    }
    // everything else (non-CD log calls): ignore
}
```

Notes:
- **Coalescing is essential** — the CD state machine logs its current phase
  every tick. Without it the log floods. With it, a phase stuck forever shows
  as a huge `xN` count — that is the hang signature.
- Pointer safety: `r2` is only dereferenced after the `CDPHASE_LO..HI` range
  check, so it is always a valid `tg16_cdrom_*` string. `r1` is only used as a
  printf format after an exact-VMA match against the known CD format strings.
- The handler runs as ordinary code (inline trampoline, not a signal handler)
  so `fprintf` is fine.
- Register an `atexit()` that calls `flush_phase()` so the final phase's count
  is written.

---

## 6. Constructor / build / deploy

Same as `m2hook_overdrive` (`SPEC.md` §6, §8, §9):
- Locate `0x1eee48` by the §3 signature within the `m2engage` `r-xp` mapping;
  verify the 8 original bytes (`0e b4 00 b5 82 b0 03 aa`) before patching.
- `mprotect` RWX, write the 8-byte redirect, `__builtin___clear_cache`.
- Log file: `/tmp/m2_cdtrace.log`. Write a startup banner.
- Cross-compile with the `m2engage-cross` Docker toolchain.
- Deploy to the stick at `/usr/game/lib/m2hook_cdtrace.so`; chain into
  `LD_PRELOAD` in `/etc/init.d/gameapp` (append, colon-separated — do not
  clobber the existing `m2hook_print.so` / `probe_gl.so`).

It can run alongside the other hooks (different patch site, different log).

---

## 7. Test procedure

1. With the hook preloaded, boot the Mini and launch **Popful Mail** (via the
   USB-library workflow). Let it reach the opening cutscene (the black screen).
2. Wait ~15-20 s past where the cutscene should appear, then pull
   `/tmp/m2_cdtrace.log`.
3. Repeat for **Vasteel 2**.
4. For a baseline, also capture a known-good CD load — e.g. a lineup CD game
   that works on the Mini — so the healthy phase sequence is on record.

---

## 8. Interpreting the trace

The log is a coalesced CD phase-transition sequence, e.g.:
```
[CD] #12 ENTER phase: tg16_cdrom_command_phase
[CD] phase: tg16_cdrom_command_phase   x3
[CD] #13 ENTER phase: tg16_cdrom_read
[CD] #14 start reading 4748
[CD] #15 ENTER phase: tg16_cdrom_data_phase
[CD] #16 ENTER phase: tg16_cdrom_data_out
...
```

Compare against the Geargrafx known-good sequence (BUSY → DATA IN → sectors
stream → transfer completes → result/idle). Diagnostic patterns:

- **A phase with a runaway `xN` count** that never advances → m2engage is
  **stuck** in that phase. That phase's handler is the bug site.
- **Sequence stops progressing** (last `ENTER` never followed by the next
  expected phase) → the state machine wedged or is waiting on an event/IRQ
  that never fires.
- **`start reading <N>` present but no `data_phase`/`data_out` following** →
  the read was issued but data delivery never happened → the data-phase /
  sector-delivery path is broken (the Beetle "CD read speed" analog).
- Compare the sector numbers in `start reading %d` against what the game
  expects — a wrong/zero sector means the command decode is off.

Whichever `tg16_cdrom_*` phase the trace wedges in names the exact handler to
disassemble next — and that handler (or the constant inside it) is the fix
target. The phase region in the binary is ~VMA `0x80ce0`–`0x82900`.

---

## 9. Deliverables

- `m2hook_cdtrace.c`, `Makefile`/`build.sh`, `README.md`.
- Append findings to a `REPORT.md` here, and update memory `cd_fmv_compat.md`.
