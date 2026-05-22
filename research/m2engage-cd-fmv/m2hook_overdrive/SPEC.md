# m2hook_overdrive — Implementation Spec

Spec for an `LD_PRELOAD` observation hook that resolves what the m2engage
`overdrive` emulator parameter (backend "opcode 44") actually does.

This document is self-contained. A session implementing it does not need prior
context — everything required is below.

---

## 1. Background / why this hook exists

m2engage (M2's emulator for the PC Engine Mini) exposes a per-game integer
parameter `overdrive`, settable in `title_prof.psb.m` at
`m2epi.version.GAME###.overdrive` and via Squirrel as `g_emu_task.overdrive`.
It is suspected — but **not proven** — to be a CPU-speed / timing knob analogous
to Mednafen's `ocmultiplier`. It is the leading candidate for fixing black-screen
FMV cutscenes in Popful Mail and Vasteel 2.

Static reverse engineering traced the setter as far as a backend command
dispatch but could **not** resolve the leaf behavior, because it goes through a
runtime-assigned function pointer. This hook resolves it dynamically.

### Verified call chain (from static RE — trust these, they are confirmed)

```
EmuTask::setOverdrive(N)                      VMA 0x23100
  ├─ str N -> [EmuTask + 0xEC]                ; value stored in object
  ├─ logs "console.overdrive=N"               ; unconditional
  └─ bl 0x3549C            (opcode-44 forwarder)
        ├─ r1 = [EmuTask->[0xF4]]->[8]        ; "handler"
        ├─ cbz r1 -> return                   ; GATE 1
        ├─ r0 = [EmuTask->[0xF4]]->[4]        ; "backend"
        ├─ r2 = 44                            ; opcode
        └─ b 0x34470          (DISPATCHER  <-- HOOK THIS)
              ├─ r3 = [r0 + 0x24]             ; backend handler fn ptr
              ├─ cbz r3 -> return             ; GATE 2
              └─ blx r3   (handler, opcode, &value)   <-- the unknown leaf
```

At the dispatcher entry `0x34470` the registers are:
`r0 = backend`, `r1 = handler`, `r2 = opcode`, `r3 = value`.

The unknown — "what does overdrive do" — is the function at `*(r0 + 0x24)`.
The hook captures that pointer and dumps its code so a static session can
disassemble it.

---

## 2. Target binary — verify before patching

The hook is hard-coded to one exact binary. The implementer MUST verify identity.

| Property | Value |
|---|---|
| Path on device | `/usr/game/m2engage` (PC Engine Mini) |
| Local reference copy | `/Users/vincentaycirieix/dev/pce/rootfs/usr/game/m2engage` |
| Size | 4041012 bytes |
| MD5 | `060f4815731c0d0717ee018665ab4a2c` |
| SHA1 | `312b223b5674a8143cdff7e4b59cca3c989086bb` |
| ELF type | `EXEC` (non-PIE, **fixed load** — VMAs are runtime addresses, no ASLR base math) |
| Arch | ARMv7, Thumb-2, stripped |

Device: PC Engine Mini — Allwinner A33 (dual Cortex-A7), Linux 3.4.113 armv7l.

If the binary differs, all offsets below are invalid. Abort and re-derive.

---

## 3. What to hook

**Hook point:** the dispatcher at **VMA `0x34470`** (file offset `0x2C470`).

Locate it robustly by this **unique 16-byte signature** (verified: exactly one
occurrence in the binary) rather than trusting the raw offset:

```
0c b4 00 b5 83 b0 43 6a 23 b1 05 aa 08 46 04 99
```

The first 8 bytes (`0c b4 00 b5 83 b0 43 6a`) are the 4 Thumb-2 instructions the
hook displaces:

```
0x34470:  0c b4   push {r2, r3}
0x34472:  00 b5   push {lr}
0x34474:  83 b0   sub  sp, #12
0x34476:  43 6a   ldr  r3, [r0, #36]
```

All four are simple, non-PC-relative — safe to relocate into a trampoline.

---

## 4. Hooking mechanism — inline patch + trampoline

Use the **same proven technique** as the existing `m2hook_pi5/m2hook_print.c`
in this repo (read it as a reference implementation — it patches m2engage on
real hardware the same way). Do **not** use BKPT/SIGTRAP — the inline trampoline
lets the C handler be ordinary code (it can call `fprintf`), which is simpler
and safer.

### Patch
Overwrite the 8 bytes at `0x34470` with a redirect to the trampoline:

```
F8DF F000      LDR.W PC, [PC, #0]
<4-byte addr>  .word  <trampoline address>   (ARM veneer addr; see note in m2hook_print.c)
```

Page handling: `mprotect` the containing page(s) to `RWX`, write the 8 bytes,
`__builtin___clear_cache()` over the patched range. (Reuse m2hook_print.c logic
verbatim.)

### Trampoline (naked Thumb function, in the .so)

```
push {r0-r3, r12, lr}        ; preserve dispatcher args + caller-saved (24 bytes, keeps 8-byte align)
bl   overdrive_handler        ; C handler, receives (r0,r1,r2,r3) intact
pop  {r0-r3, r12, lr}         ; restore exactly

; replay the 4 displaced instructions:
push {r2, r3}
push {lr}
sub  sp, #12
ldr  r3, [r0, #36]

; resume in the original dispatcher at 0x34470 + 8
ldr  r12, =resume_addr        ; resume_addr = 0x34478 | 1   (Thumb bit set)
bx   r12
```

Notes:
- `overdrive_handler` is a normal AAPCS function; it must NOT modify r0-r3 as
  seen by the dispatcher (the trampoline save/restore guarantees this).
- r12 is free to clobber for the final jump — the dispatcher does not rely on
  r12 at `0x34478`.
- `resume_addr` (`0x34478 | 1`) and the patched binary base: binary is ET_EXEC
  so the runtime address equals the VMA. Still, locate `0x34470` by signature
  scan within the `m2engage` `r-xp` mapping (see m2hook_print.c
  `find_code_section` + `find_pattern`), and compute `resume_addr` as
  `found_addr + 8`.

---

## 5. C handler logic

```c
// opcode of interest
#define OP_OVERDRIVE 44

static volatile unsigned g_op_counts[256];   // histogram of all opcodes seen
static volatile int      g_od_hits = 0;
static FILE             *g_log;              // opened in constructor

void overdrive_handler(uint32_t r0, uint32_t r1, uint32_t r2, uint32_t r3)
{
    if (r2 < 256) g_op_counts[r2]++;         // census of every dispatch
    if (r2 != OP_OVERDRIVE) return;          // filter

    g_od_hits++;
    uint32_t backend = r0;
    uint32_t handler = r1;
    uint32_t value   = r3;

    // backend->[0x24] : the function blx will call — THE KEY DATUM
    uint32_t H     = *(uint32_t *)(backend + 0x24);
    uint32_t Hcode = H & ~1u;                // strip Thumb bit

    fprintf(g_log,
        "[OVRD] hit #%d  opcode=44(setOverdrive)\n"
        "       backend = 0x%08x\n"
        "       handler = 0x%08x\n"
        "       value   = %d (0x%08x)\n"
        "       *(backend+0x24) = 0x%08x  -> handler code @ 0x%08x\n",
        g_od_hits, backend, handler, (int)value, value, H, Hcode);

    // context: dump the backend object header
    fprintf(g_log, "       backend[0x00..0x30]:");
    for (int i = 0; i <= 0x30; i += 4)
        fprintf(g_log, " %08x", *(uint32_t *)(backend + i));
    fprintf(g_log, "\n");

    // dump the handler function's code so a static session can disassemble it
    const int DUMP = 4096;                   // generous: covers switch + nearby cases
    fprintf(g_log, "       --- code dump @ 0x%08x (%d bytes) ---\n", Hcode, DUMP);
    for (int i = 0; i < DUMP; i += 16) {
        fprintf(g_log, "       %08x:", Hcode + i);
        for (int j = 0; j < 16; j++)
            fprintf(g_log, " %02x", *(uint8_t *)(Hcode + i + j));
        fprintf(g_log, "\n");
    }
    fprintf(g_log, "       --- end dump ---\n");
    fflush(g_log);
}
```

Reading `*(backend+0x24)` and the code at `Hcode` is safe: the dispatcher itself
performs the same `[r0+0x24]` read, and `Hcode` lies in the binary's `r-x`
segment.

Keep the hook **always armed** — the dispatcher is not hot enough for the
overhead to matter on a 1 GHz A7. No disarm logic needed.

---

## 6. Constructor

`__attribute__((constructor))`, mirroring `m2hook_print.c`:

1. Confirm `/proc/self/exe` contains `m2engage`; else return (do nothing).
2. Open log file `/tmp/m2_overdrive_hook.log` (`O_CREAT|O_WRONLY|O_APPEND`),
   keep a `FILE*` global. Also write a startup banner.
3. Parse `/proc/self/maps`, find the `r-xp` mapping for `m2engage` → code base+size.
4. Scan that range for the 16-byte signature in §3. If not found → log error, return.
5. **Verify** the 8 bytes at the found address equal `0c b4 00 b5 83 b0 43 6a`.
   If not → log error, return (wrong binary / bad match).
6. Save the original 8 bytes; compute `resume_addr = found + 8` (Thumb bit set).
7. `mprotect` page(s) → `RWX`; write the `LDR.W PC,[PC]` + `.word trampoline`
   redirect; `__builtin___clear_cache()` the 8-byte range.
8. Register an `atexit()` handler that dumps `g_op_counts` (see §7).
9. Log "hook armed at 0x%08x".

---

## 7. Logging / output

File: `/tmp/m2_overdrive_hook.log`

- **Startup banner:** hook version, resolved dispatcher address, timestamp.
- **Per opcode-44 hit:** the full block from the §5 handler — args, the
  `backend+0x24` handler pointer, backend header, and a 4 KB code dump of the
  handler function.
- **At process exit (`atexit`):** dump the opcode histogram:
  ```
  [OVRD] opcode census (dispatcher calls by opcode):
         opcode  3: 1840
         opcode 12: 60
         opcode 44: 1
         ...
  ```

The histogram is the critical diagnostic if **no opcode-44 line appears**:
- Histogram non-empty, no `44` → dispatcher works, but `setOverdrive`'s dispatch
  was skipped (GATE 1 null) — the value never reaches the backend.
- Histogram empty → hook didn't take, or the dispatcher genuinely is never
  called (investigate the patch).
- `44` present → we have the handler address + code dump; done.

---

## 8. Build

Cross-compile for the A33 / PC Engine Mini. Use the toolchain from
`m2engage-mac/build-a33.sh` (Docker, Ubuntu 20.04 + ARM GCC 9) — it is already
validated to produce binaries that run on the Mini.

```
arm-linux-gnueabihf-gcc -shared -fPIC -O2 -march=armv7-a \
    -o m2hook_overdrive.so m2hook_overdrive.c -ldl -Wl,--no-as-needed
```

glibc caveat: the Mini runs an old (Linux 3.4-era) glibc. Keep libc usage
minimal (`open`, `write`/`fprintf`, `mprotect`, `memcpy`, `getenv`, `atexit`).
If the `.so` fails to load with symbol-version errors, rebuild against the same
sysroot `build-a33.sh` uses, or drop `stdio` for raw `write(2)` formatting.

---

## 9. Deploy & run

1. Copy `m2hook_overdrive.so` to the Mini (see device access in project memory:
   `ssh root@169.254.13.37`; for binary transfer use
   `cat file | ssh root@169.254.13.37 "cat > /path"` — `scp -O` is unreliable).
2. Inject `LD_PRELOAD` into the m2engage launch. See the USB-stick library
   workflow notes for how m2engage is started on the Mini; prepend
   `LD_PRELOAD=/path/to/m2hook_overdrive.so` (append to any existing
   `LD_PRELOAD`, colon-separated — do not clobber it).
3. Set `"overdrive": 8` (any non-zero int) in the Popful Mail per-game
   `m2epi.version.GAME###` block of its `title_prof.psb.m`, repacked with
   `mzstool.py` if editing the JSON form.
4. Boot the Mini, launch Popful Mail. `setOverdrive` fires during emulator init
   — within seconds of the game loading. FMV playback is **not** required to
   capture the opcode-44 event.
5. Retrieve `/tmp/m2_overdrive_hook.log`.

---

## 10. Interpreting results / next step

The log's 4 KB code dump at `Hcode` is the M2Epi backend command handler. It is
a shared multi-opcode function — it switches on the opcode argument (opcode
arrives in `r1` inside that function; see §1 dispatcher: `blx r3(handler,
opcode, &value)`).

A static session then:
1. Disassembles the dumped bytes (`arm-linux-gnueabihf-objdump -D -b binary
   -m armv7 -M force-thumb`).
2. Finds the opcode-44 case (look for a jump table `tbb/tbh` after a bound
   check, or a `cmp rX,#44`).
3. Follows case 44 to the leaf — that is the definitive answer to "what does
   overdrive do."

**Phase 2 (only if needed):** if the case-44 leaf jumps outside the 4 KB dump,
note the leaf address from the disassembly and make a v2 hook that patches
*that* address with the same trampoline technique, logging its registers/effects.

---

## 11. Risks & gotchas

- **Wrong binary** → offsets invalid. The §3 signature scan + §6 step-5 byte
  verification guard against this; both must pass or the hook self-aborts.
- **Thumb veneer:** GCC emits an ARM veneer for a Thumb naked function taken by
  address. Use the veneer address (no Thumb bit) in the `.word`, exactly as
  `m2hook_print.c` documents at its `hook_addr` line.
- **Stack alignment:** AAPCS requires 8-byte sp alignment at `bl`.
  `push {r0-r3, r12, lr}` = 24 bytes = multiple of 8 — preserved. Do not change
  the register list without rechecking.
- **Do not modify r0-r3** as seen by the dispatcher — this is an observer.
  The trampoline's save/restore is mandatory.
- **`overdrive` is a 32-bit signed integer** (verified: stored via `str.w`,
  read via `ldr.w`, no VFP anywhere). The logged `value` should be printed both
  signed-decimal and hex.
- **LD_PRELOAD chaining:** the Mini may already set `LD_PRELOAD` (e.g. a libMali
  shim). Append, never overwrite.
- The hook is a pure observer — it changes no emulation behavior, so it is safe
  to leave installed while testing different `overdrive` values.

---

## 12. Deliverables

- `m2hook_overdrive.c` — the hook (constructor + trampoline + handler).
- `Makefile` — cross-compile rule (model on `m2hook_pi5/Makefile`).
- `README.md` — build + deploy + how to read the log.
