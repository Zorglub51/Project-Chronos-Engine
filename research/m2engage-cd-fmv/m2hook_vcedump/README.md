# m2hook_vcedump

`LD_PRELOAD` observer for m2engage on the PC Engine Mini. Periodically
dumps the emulated VCE CRAM (palette, 1024 bytes / 512 × 16-bit
entries) every 500 ms. Confirms whether the palette is black at the
moment the FMV cutscene screen is black.

Derives directly from `m2hook_vramdump` — same VDC/VCE constructor
patch at VMA `0x7aa08`, same trampoline mechanism, same lazy post-fork
spawn pattern. CRAM pointer comes from `*(VDC_ctx + 0x18)` (identified
via the second `bl 0x713c0` state-save registration in the constructor
disassembly, where `r3 = 512` → 512 16-bit color entries).

See `SPEC.md` for the full investigation context and `SPEC.md §4b` for
the follow-up plan (per-frame VCE-port-write trace for the "rare image
flash" timing-race hypothesis).

## Build

```sh
bash build.sh
# -> build/m2hook_vcedump.so   (ARM, EABI5, ~30 KB)
```

## Deploy

```sh
cat build/m2hook_vcedump.so | ssh root@169.254.13.37 \
    "cat > /usr/game/lib/m2hook_vcedump.so && chmod 755 /usr/game/lib/m2hook_vcedump.so"
```

Chain into `LD_PRELOAD` by adding to `/etc/init.d/gameapp`:

```sh
[ -f ${GAME_HOME}/lib/m2hook_vcedump.so ] && preload=${preload:+${preload}:}${GAME_HOME}/lib/m2hook_vcedump.so
```

Then restart m2engage (kill + setsid /etc/init.d/gameapp start). Can
run alongside vramdump or independently — they patch the same VDC ctor
site but neither modifies behaviour.

## Reading the log

`/tmp/m2_vcedump.log` writes one line per dump tick:

```
[VCE] #0042 02:35:18 ctx=0x0150ec80 cram=0x01a8b400 nonzero_bytes=0/1024 (0.0%) nonzero_entries=0/512 head=[0000 0000 0000 0000 0000 0000 0000 0000] -> /tmp/m2_cram_0042.bin
```

The decisive at-a-glance fields:

- **`nonzero_bytes`** — total non-zero bytes in the 1024-byte palette
  buffer. Stays near 0 during a black-palette cutscene; jumps to
  several hundred when a real palette is active.
- **`nonzero_entries`** — non-zero 16-bit entries out of 512. A real
  game uses ~16–256 of these; black-palette = 0–few.
- **`head=[...]`** — the first 8 palette entries (palette 0,
  entries 0–7) as raw 16-bit values, each is a 9-bit RGB color. All
  zeros = black BG palette 0.

## Test

1. Launch PopfulMail, sit on the black cutscene ~30 s.
2. Pull `/tmp/m2_vcedump.log` and the `/tmp/m2_cram_*.bin` files.
3. Cross-reference timestamps in `m2_vramdump.log` (if vramdump is
   also loaded): VRAM populated + CRAM all-zero is the canonical
   "palette black" signature.
4. **Reference**: in Geargrafx (MCP port 7777), `read_memory area 8`
   (PALETTES) during the working cutscene — known-good. Compare with
   any `m2_cram_NNNN.bin` taken during the same cutscene moment in
   m2engage.

## Files

- `m2hook_vcedump.c` — hook source.
- `build.sh` / `Makefile` — Docker cross-compile.
- `SPEC.md` — full investigation spec (incl. §4b port-write trace plan).
- `build/m2hook_vcedump.so` — output binary (after `bash build.sh`).
