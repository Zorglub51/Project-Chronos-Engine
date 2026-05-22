# m2hook_overdrive

`LD_PRELOAD` observation hook for m2engage on the PC Engine Mini. Resolves
what the `overdrive` emulator parameter actually does by patching the
dispatcher that routes `setOverdrive` calls to the M2Epi backend, logging
the resolved leaf function pointer + 4 KB of its code for offline
disassembly.

Pure observer — never modifies registers seen by the dispatcher, so it is
safe to leave armed during normal play.

See `SPEC.md` for full background, addresses, and rationale.

## Build

Requires Docker (uses the `m2engage-cross` image — built on first run if
absent, same toolchain as `m2engage-mac/build-a33.sh`).

```sh
bash build.sh
# -> build/m2hook_overdrive.so   (ARM, EABI5, dynamically linked)
```

## Deploy

1. Copy the `.so` to the Mini:

   ```sh
   cat build/m2hook_overdrive.so | ssh root@169.254.13.37 "cat > /usr/game/lib/m2hook_overdrive.so"
   ssh root@169.254.13.37 "chmod 755 /usr/game/lib/m2hook_overdrive.so"
   ```

2. Chain into `LD_PRELOAD` for the m2engage launch. The Mini already loads
   `m2hook_print.so` — **append**, do not replace. In `/etc/init.d/gameapp`
   (or wherever the launch line lives), prepend:

   ```sh
   export LD_PRELOAD="/usr/game/lib/m2hook_overdrive.so:${LD_PRELOAD}"
   ```

   On the USB-mod setup, the hook ships under `mod-assets/lib/` and the
   `gameapp` wrapper auto-loads everything in `/usr/game/lib/`.

3. Edit `title_prof.psb.m` to set `"overdrive": 8` (or any non-zero int) on
   one game's `m2epi.version.GAME###` block. Use `mzstool.py` to extract
   /repack.

4. Boot the Mini, launch the modified game. `setOverdrive` fires during
   emulator init — within seconds of the game starting. FMV playback is
   **not** required to trigger an opcode-44 event.

5. Retrieve the log:

   ```sh
   scp root@169.254.13.37:/tmp/m2_overdrive_hook.log .
   ```

## Reading the log

Startup banner — confirms the hook armed at the expected dispatcher VMA:

```
[OVRD] === hook startup (pid 1234, exe /usr/game/m2engage) ===
[OVRD] code section: 0x00010000 .. 0x003e7000 (4022272 bytes)
[OVRD] dispatcher found at 0x00034470
[OVRD] hook armed at 0x00034470 -> trampoline 0x76aaXXXX (resume 0x00034479)
```

Per-hit block on each opcode-44 dispatch:

```
[OVRD] hit #1  opcode=44 (setOverdrive)
       backend = 0x00abc000
       handler = 0x00abc100
       value   = 8 (0x00000008)
       *(backend+0x24) = 0x00345789  -> handler code @ 0x00345788
       backend[0x00..0x30]: ...
       --- code dump @ 0x00345788 (4096 bytes) ---
       00345788: 80 b5 00 af ...
       ...
       --- end dump ---
```

Process-exit histogram (via `atexit`):

```
[OVRD] opcode census (dispatcher calls by opcode):
       opcode   3: 1840
       opcode  12: 60
       opcode  44: 1
       ...
[OVRD] total opcode-44 hits: 1
```

### Interpreting

- **opcode-44 hit captured** → disassemble the dumped bytes (`Hcode` from
  the per-hit line) with:

  ```sh
  # Extract just the hex columns from the dump, then:
  arm-linux-gnueabihf-objdump -D -b binary -m armv7 -M force-thumb dump.bin
  ```

  Look for a jump table (`tbb` / `tbh`) or `cmp rX, #44` to find the
  opcode-44 case; follow it to the leaf — that's the definitive answer to
  what `overdrive` does.

- **histogram non-empty, no opcode 44** → the dispatcher fires for other
  opcodes but `setOverdrive`'s forwarder skipped it (GATE 1 at
  `[EmuTask + 0xF4][8]` was null). The value never reaches the backend.
  Investigate why the handler pointer is missing on the Mini build.

- **histogram empty** → the hook didn't take, or the dispatcher genuinely
  is never called. Re-check the startup banner; verify the `m2engage` MD5
  matches `060f4815731c0d0717ee018665ab4a2c`.

## Files

- `m2hook_overdrive.c` — hook source (constructor + Thumb trampoline + C handler).
- `build.sh` — Docker cross-compile via `m2engage-cross` image.
- `Makefile` — thin wrapper around `build.sh`.
- `SPEC.md` — full spec (the document that drove this implementation).
- `build/m2hook_overdrive.so` — output binary (after `bash build.sh`).
