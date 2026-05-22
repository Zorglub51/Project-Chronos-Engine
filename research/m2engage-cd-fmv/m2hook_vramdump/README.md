# m2hook_vramdump

`LD_PRELOAD` observer for m2engage on the PC Engine Mini. Periodically
dumps the emulated VDC VRAM (64 KB) to disk so we can compare against
Geargrafx's known-good VRAM during the same FMV cutscene — the
**decisive split** between an "FMV image is in VRAM but not displayed"
bug (display side) and an "FMV image never reached VRAM" bug (upstream:
decoder, blit, or CD data content).

See `SPEC.md` for the full rationale. Implementation is modelled on
`m2hook_cdtrace/m2hook_cdtrace.c`; see `m2hook_cdtrace/REPORT.md` for
why this hook intentionally avoids any high-rate patched site (no VFP
clobber risk).

## Build

```sh
bash build.sh
# -> build/m2hook_vramdump.so   (ARM, EABI5)
```

## Deploy

```sh
cat build/m2hook_vramdump.so | ssh root@169.254.13.37 \
    "cat > /usr/game/lib/m2hook_vramdump.so && chmod 755 /usr/game/lib/m2hook_vramdump.so"
```

Chain into `LD_PRELOAD` by adding to `/etc/init.d/gameapp` (after the
existing `m2hook_cdtrace.so` line, or anywhere else in the chain):

```sh
[ -f ${GAME_HOME}/lib/m2hook_vramdump.so ] && preload=${preload:+${preload}:}${GAME_HOME}/lib/m2hook_vramdump.so
```

Then restart m2engage (kill + setsid /etc/init.d/gameapp start).

## Test

1. Launch Popful Mail, let it reach the black cutscene, sit on it ~20 s.
2. Pull files:

   ```sh
   ssh root@169.254.13.37 'cat /tmp/m2_vramdump.log'                 # summary
   ssh root@169.254.13.37 'ls -la /tmp/m2_vram_*.bin'                 # available dumps
   ssh root@169.254.13.37 'cat /tmp/m2_vram_NNNN.bin' > vram_NNNN.bin # specific dump
   ```

3. Each dump is exactly 64 KB. The accompanying `m2_vdcctx_NNNN.bin` is
   the first 256 bytes of the VDC context object — register-state
   context for the dump's moment.

## Reading the log

Banner (constructor + hook arm):

```
[VRAM] === hook startup (pid 1234, exe /usr/game/m2engage) ===
[VRAM] code section: 0x00008000 .. 0x003dd000 (...)
[VRAM] VDC ctor found at 0x0007aa08
[VRAM] hook armed at 0x0007aa08 -> trampoline 0x76aaXXXX (resume 0x0007aa11)
[VRAM] dumper thread launched (period 2000000 us, keep 20)
```

First constructor hit (one line per game launch):

```
[VRAM] VDC ctor: ctx=0x01abcd00
```

Periodic dumps (one line every ~2 s once the ctx is captured):

```
[VRAM] #0000 12:34:56 ctx=0x01abcd00 vram=0x01abce00 nonzero=12345/65536 (18.8%) -> /tmp/m2_vram_0000.bin
```

The non-zero byte percentage is the at-a-glance verdict — flat ~0%
during the black cutscene means VRAM is empty (upstream bug); a
plausible structured count (tens of %) means it's populated and the
bug is display-side.

## Files

- `m2hook_vramdump.c` — hook source.
- `build.sh` / `Makefile` — Docker cross-compile.
- `SPEC.md` — full spec.
- `build/m2hook_vramdump.so` — output binary (after `bash build.sh`).
