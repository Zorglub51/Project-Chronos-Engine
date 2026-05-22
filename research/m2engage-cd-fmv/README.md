# m2engage CD FMV Black-Screen Investigation

This directory contains the research and tooling produced during an
investigation into PopfulMail (and Vasteel 2)'s cutscene black-screen
bug on the PC Engine Mini.

## TL;DR

The bug is in m2engage's CD-ROM emulator state machine. After ~5
sectors of FMV data stream successfully, the state machine gets stuck
at `phase=6 (RESULT)` with `status=0x78`, and subsequent command bytes
from the BIOS are rejected by a buggy phase check at VMA `0x827d2`.
The PC Engine BIOS at `$EABE` then polls `$1800` forever waiting for
a transition that never comes.

See `m2hook_vcedump/REPORT.md` for the full investigation log (1475
lines across 9 sessions of analysis).

## The hooks (chronological investigation order)

| Hook | Purpose | Status |
|------|---------|--------|
| `m2hook_overdrive` | Trace m2engage's `overdrive` parameter dispatch | Done — found it's a CPU clock knob |
| `m2hook_cdtrace` | Recover m2engage's discarded CD phase logger | Done — confirmed CD reads succeed |
| `m2hook_vramdump` | Periodic VRAM dumps | Done — confirmed VRAM is populated |
| `m2hook_vcedump` | Periodic CRAM/palette dumps | Done — confirmed palette is alive (contains main REPORT.md) |
| `m2hook_cdstatus` | Hook CD-ROM ctor (wrong target — was registration helper) | Failed but informative |
| `m2hook_cpuread` | Hook CPU read dispatcher (turned out to be write path) | Failed but informative |
| `m2hook_cdpoke` | Live SCSI state observer + optional unstick poker | Done — found stuck state, force-poke unsafe |
| `m2hook_cdfix` | Binary patch attempt (NOP the buggy bne) | Failed — broke BIOS init |

## Build & deploy

Each hook is built with `bash build.sh` (uses the
`m2engage-cross` Docker image — same toolchain as the rest of the
project). Output is `build/m2hook_<name>.so`.

Deploy to the PC Engine Mini via:
```sh
cat build/m2hook_<name>.so | ssh root@169.254.13.37 \
    "cat > /usr/game/lib/m2hook_<name>.so && chmod 755 /usr/game/lib/m2hook_<name>.so"
```

Then add to the `LD_PRELOAD` chain in `/etc/init.d/gameapp`.

## Final state

The bug is precisely localized. The remaining work needs an
interactive Ghidra session (the decompiler doesn't work on Apple
Silicon in our setup) to find where m2engage's CD-ROM emulator
should write `phase = 1` after MSG_IN completes. Once that path is
identified and fixed, the cutscene should play.
