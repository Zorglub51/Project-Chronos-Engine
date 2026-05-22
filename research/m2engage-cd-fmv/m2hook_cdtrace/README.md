# m2hook_cdtrace

`LD_PRELOAD` observer hook for m2engage on the PC Engine Mini. Recovers
the CD-ROM phase-transition log that m2engage's release build computes
and then discards (the log sink at VMA `0x1eee48` returns early when
its context pointer is null).

Built to diagnose the FMV-cutscene black-screen bug in Popful Mail and
Vasteel 2 — by capturing the CD state machine's own log lines, we can
see exactly which `tg16_cdrom_*` phase the engine wedges in.

Pure observer — never modifies the logger's r0..r3, safe to leave armed
during normal play. See `SPEC.md` for full background.

## Build

Requires Docker (uses the `m2engage-cross` image — built on first run if
absent, same toolchain as `m2engage-mac/build-a33.sh` and
`m2hook_overdrive/build.sh`).

```sh
bash build.sh
# -> build/m2hook_cdtrace.so   (ARM, EABI5, dynamically linked)
```

## Deploy

1. Copy the `.so` to the Mini via the console (it lives in the stick's
   `game/lib/`, bind-mounted to `/usr/game/lib/`):

   ```sh
   cat build/m2hook_cdtrace.so | ssh root@169.254.13.37 \
       "cat > /usr/game/lib/m2hook_cdtrace.so && chmod 755 /usr/game/lib/m2hook_cdtrace.so"
   ```

2. Chain into `LD_PRELOAD`. The Mini's `/etc/init.d/gameapp` wrapper
   conditionally adds each `.so` it finds — append a line for this one
   (alongside the existing `m2hook_print.so`, `probe_gl.so`,
   `m2hook_overdrive.so`):

   ```sh
   [ -f ${GAME_HOME}/lib/m2hook_cdtrace.so ] && preload=${preload:+${preload}:}${GAME_HOME}/lib/m2hook_cdtrace.so
   ```

3. Restart `m2engage` (see SPEC §7 / `m2hook_overdrive/REPORT.md`).

## Test

1. Boot the Mini, launch **Popful Mail**. Let it reach the opening
   cutscene (black screen). Wait 15–20 s past the point the cutscene
   should appear.
2. Pull the log:

   ```sh
   ssh root@169.254.13.37 'cat /tmp/m2_cdtrace.log' > popful_cdtrace.log
   ```

3. Repeat for **Vasteel 2** and a known-good CD title for baseline.

## Reading the log

Startup banner — confirms the hook armed:

```
[CD] === hook startup (pid 1234, exe /usr/game/m2engage) ===
[CD] code section: 0x00008000 .. 0x003dd000 (4018176 bytes)
[CD] logger found at 0x001eee48
[CD] hook armed at 0x001eee48 -> trampoline 0x76aaXXXX (resume 0x001eee49)
```

Trace body — coalesced phase transitions + decoded format-string lines:

```
[CD] #12 ENTER phase: tg16_cdrom_command_phase
[CD] phase: tg16_cdrom_command_phase   x3
[CD] #13 ENTER phase: tg16_cdrom_read
[CD] #14 start reading 4748
[CD] #15 ENTER phase: tg16_cdrom_data_phase
[CD] #16 ENTER phase: tg16_cdrom_data_out
...
```

### Diagnostic patterns

- **A phase with a runaway `xN` count that never advances** →
  m2engage is stuck. That phase's handler is the bug.
- **Sequence stops progressing** (last `ENTER` has no follow-up) →
  state machine wedged on an event/IRQ that never fires.
- **`start reading <N>` with no following `data_phase`/`data_out`** →
  read issued but no data delivery — the data-phase path is broken.
- **Wrong sector number in `start reading`** → command decode is off
  upstream.

The phase whose handler (in the binary's `~0x80ce0..0x82900` VMA range)
is wedging names the function to disassemble next. That's the fix
target.

## Files

- `m2hook_cdtrace.c` — hook source.
- `build.sh` / `Makefile` — Docker cross-compile.
- `SPEC.md` — full spec.
- `build/m2hook_cdtrace.so` — output binary (after `bash build.sh`).
