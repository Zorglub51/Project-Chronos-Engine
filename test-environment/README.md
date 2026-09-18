# Native M2 test environment

Run the original ARM32 `m2engage` engine and Chronos folder packs on a Linux
desktop VM, including an ARM64 Ubuntu VM on an Apple Silicon Mac. This exercises
the **console's engine**, the unchanged Chronos hook, and the four shipped
Squirrel patches. It does not use the separate C++/macOS rewrite.

The sources in `graphics/` were recovered from the existing Linux VM prototype
(`m2engage-pce-pi5`). This directory makes that work buildable from this repo,
adds an isolated launcher, and removes the old duplicate Squirrel print hook.
No original engine, ROM, BIOS, extracted asset, VM image or user save is included.

## How it works

```text
ARM32 m2engage under qemu-arm-static
  + console-mod/hook-src/m2hook_print.c (built unchanged)
  + ARM32 libMali.so → shared command ring → native gl_proxy → Mesa/virgl → X11
```

The hook sees its console paths inside a private mount namespace:

| Console path | Private test data |
|---|---|
| `/usr/game` | `runtime/game` |
| `/rootfs_data` | `runtime/game/save` |
| `/mnt/usb/library/published` | `runtime/published` |

Scripts, configurations, graphics and packs are copied. ROMs are symlinked to
their sources, which are read-only inside the namespace. All initial test saves
are empty; existing saves in the published library are **not imported**. The
rest of the VM filesystem is read-only to the engine. `/tmp`, input FIFOs and
`/dev/shm` are private. The VM's GPU and desktop sockets are shared for display.
This is filesystem/process isolation for testing, not a security boundary for
untrusted binaries.

## Prerequisites inside Linux

Tested with Ubuntu 24.04 ARM64, QEMU 8.2.2, Mesa 25.2.8 and a Parallels desktop
session. Log into the graphical desktop first. The GPU must support GBM/EGL ES2;
the display must offer X11 or Xwayland. The proxy currently uses
`/dev/dri/renderD128` and a fixed 1280×720 window.

On Ubuntu ARM64, these packages supply the build and runtime dependencies:

```sh
sudo dpkg --add-architecture armhf
sudo apt update
sudo apt install build-essential gcc-arm-linux-gnueabihf python3 \
  qemu-user-static bubblewrap libegl1-mesa-dev libgles2-mesa-dev libgbm-dev libx11-dev \
  libc6:armhf libstdc++6:armhf libgcc-s1:armhf libopenal1:armhf \
  libopus0:armhf libbsd0:armhf libpulse0:armhf zlib1g:armhf
```

Use a VM for this workflow. `start` needs root for the hook's bind mounts, which
are confined to its namespace; it does not install console init/udev scripts or
alter the VM's `/usr/game`. Python 3.9+ is required. No Docker or system-wide
ARM32 executable registration is necessary.

You also need your own:

- extracted stock `alldata.bin` tree (`system/`, `040/`, etc.);
- JP `1006JP` ARM32 `m2engage` and its `version` / `shutdown.png` support files;
- published Chronos library with `folders/jp/_root` and each folder's three
  `.psb.m` files, plus its optional `roms/` directory.

The hook has fixed addresses. Preparation accepts only these SHA-256 identities:

| Binary | SHA-256 | VM validation |
|---|---|---|
| Stock JP 1006JP | `b02848f66b82f8ac3090db523c4db9633508f9e3f53c7dc0ee3d01ce8aee8792` | Accepted identity; stock path not yet smoke-tested |
| Existing VM platform-patched JP binary | `200044b9b0491302a0cac7830e6dd6ec2289b8315a5866eee952222f1de37cfb` | Used for the interactive tests below |

An arbitrary firmware version must not be added to this list without checking
the hook addresses and the hardware adaptation. The launcher does not patch or
download an engine.

## From this Mac, using the existing Parallels VM

Run these commands from the repository root. Defaults are the VM
`Ubuntu 24.04 ARM64`, its `/home/parallels/chronos-native` working directory, and
the repository under the shared Mac home at `/media/psf/Home`.

```sh
python3 test-environment/parallels.py resume
python3 test-environment/parallels.py build

# Once only: use a NEW destination. These are this workspace's existing inputs.
python3 test-environment/parallels.py prepare \
  --data /media/psf/Home/dev/pce/alldata_original \
  --binary /home/parallels/pce/m2engage-patched \
  --support /media/psf/Home/dev/pce/rootfs/usr/game \
  --published /media/psf/Home/dev/pce/publish_out/m2engage

python3 test-environment/parallels.py start
python3 test-environment/parallels.py status
python3 test-environment/parallels.py screenshot
python3 test-environment/parallels.py stop
```

Open the VM's desktop to use the window named **m2engage (GL proxy)**. Screenshots
contain only that window's rendered frame and are copied to the ignored local
`test-environment/.work/screenshot.png` file. `stop` stops only this session and
keeps its test saves; it leaves the VM running.

Global options go before the command, for example:

```sh
python3 test-environment/parallels.py --vm 'My Linux VM' \
  --guest-root /home/me/chronos-test \
  --guest-repo /media/psf/Home/dev/Project-Chronos-Engine build
```

To refresh published content, prepare a new `--guest-root` rather than
overwriting an existing runtime. Build into that new root too. The old
`/home/parallels/pce/m2engage-pce-pi5` prototype and its saves are not modified.

## Directly inside Linux

For an existing prepared runtime, install a local launcher and copy linked ROMs
onto the VM disk **once**, while the test session is stopped:

```sh
sudo python3 test-environment/install-local.py --root /home/parallels/chronos-native
```

After installation, run these commands in Ubuntu, from any working directory:

```sh
~/chronos-native/chronos
~/chronos-native/chronos stop
~/chronos-native/chronos status
```

The launcher requests `sudo` when needed and uses the guest's audio configuration
by default. Use `~/chronos-native/chronos start --silent` to mute a test. It uses
only the VM's local `native.py`, binaries, packs and copied ROMs; the shared Mac
folder is no longer needed. The original test saves are kept. Local sessions
also hide `/media` inside their mount namespace to detect accidental reliance
on the Mac share. They do not unmount the share for the rest of Ubuntu.

To update the installed launcher, stop it and rerun `install-local.py` from an
updated checkout. To update games/packs, prepare a new runtime as described above.

The same harness works without Parallels Tools:

```sh
make -C test-environment BUILD=/home/me/chronos-test/build
python3 test-environment/native.py prepare \
  --runtime /home/me/chronos-test/runtime \
  --data /path/to/extracted-stock \
  --binary /path/to/m2engage \
  --support /path/to/original/usr/game \
  --published /path/to/published-library
sudo python3 test-environment/native.py start \
  --runtime /home/me/chronos-test/runtime \
  --build /home/me/chronos-test/build
sudo python3 test-environment/native.py stop --runtime /home/me/chronos-test/runtime
```

With GNOME/Xwayland, the single logged-in desktop's Xauthority file is discovered
automatically. Other desktops or multiple logged-in users need explicit
`DISPLAY` and `XAUTHORITY` environment variables passed to `start`.

Audio uses the desktop's PulseAudio-compatible socket (including PipeWire-Pulse).
The launcher discovers it from the graphical session or `sudo` user, then sets
`PULSE_SERVER` and selects OpenAL's `pulse` backend. This is necessary because
the engine runs as root while the sound server belongs to the desktop user.
Explicit `PULSE_SERVER` and `ALSOFT_DRIVERS` settings are preserved; `--silent`
always selects the null driver. The selected backend/server appear in the log.

## Controls and diagnostics

| Key in the VM window | Console control |
|---|---|
| Arrow keys | Direction pad |
| Z | I / confirm |
| X | II / back |
| Enter | RUN |
| Right Shift | SELECT |
| Right Shift + Enter | In-game menu |

The inherited keyboard adapter currently mirrors the keyboard to both virtual
pads. The command-line injector below sends only to pad 1.

```sh
python3 test-environment/parallels.py key --key left
python3 test-environment/parallels.py key --key z
python3 test-environment/parallels.py key --key select --key run --hold 1
python3 test-environment/parallels.py log
```

`start --debug --silent` enables graphics/hook traces; `--duration 60` stops
automatically after 60 seconds. Each start replaces `runtime/session.log`.
`status` reports process liveness and `.current`, not a guarantee of menu
readiness; use a screenshot and the log to inspect actual progress.

If Parallels reports `PrlJob_GetResult: Invalid argument`, inspect `status` and a
new screenshot before repeating input: the guest command may already have run.
This error was observed intermittently in Parallels Tools during testing.

## Validation and current limits

Interactive checks on the existing Apple Silicon / Parallels VM:

- GPU rendering via `virgl (Apple M4 (Compat))`, language selection and French menu;
- entering the Namcot folder and returning via its back entry, with `.current`
  changing to `jp/FOLDER_NAMCOT` and back to `jp/_root`;
- native HuCard emulation (The Genji and the Heike Clans and The Kung Fu);
- SELECT + RUN opens the original in-game menu, and its return action restores
  the game catalogue;
- restart with existing private test data, without debug mode.
- local launcher startup with `/media` hidden and all ROMs copied into the VM.
- audible menu music through the desktop's Pulse-compatible server, with both
  M2 stereo channels active on the VM's playback device after a normal launch.
- JP → US → JP menu switching with 29 JP entries and 7 US entries, restoring
  the JP cursor; selecting an empty destination leaves the current menu intact.

### Black screen when changing lineup

The installed US test pack initially contained 200 `DUMMY` items and both
`titleNum` and `titleNumTG` set to zero. The native carousel assumes at least
one entry and accesses an empty array during its rebuild. Changing `.current`
alone only recovers startup; it does not repair that catalogue.

The shipped title-select script now reads the destination root catalogue before
changing the active pack, cursor, saves or transition animation. An empty or
unreadable catalogue cancels the switch with the existing rejection sound. The
check loads only configuration, releases its resource afterwards, and does not
run each frame or load the destination covers. Resource paths use
`../../mnt/usb/library/published/folders/<lineup>/_root/title_mode_top.psb`
because the original engine prefixes resource paths with `/usr/game/`.

For a stale catalogue, republish the intended library and refresh its complete
set of three pack PSBs while the engine is stopped. Back up existing packs and
saves first; do not replace `saves/` just to update a catalogue. The affected VM
was repaired with the seven US entries already present in its source library;
all seven ROMs were already installed and matched the published files.

Regression checked with the original ARM32 engine in the VM:

1. Keep the empty US pack, start JP, then choose US. The menu stays responsive
   and `.current` remains `jp/_root`; no folder swap or SRAM splice occurs.
2. Install the populated US pack and restart. Choose US: the log reports
   `titleNumTG=7` and all seven covers appear in the TurboGrafx menu.
3. Choose JP again: the JP catalogue and previous cursor are restored.

This validates menu switching, not game/save-state compatibility after a switch.

### Remaining limits

The previous adapter retained fake-device descriptor numbers after `close`,
allowing later file reads to be mistaken for I2C reads. It now clears them.
Early runs also exhibited a black startup; subsequent successful starts do not
constitute a long-session reliability test.

**Known native-menu issue:** restarting with `.current` pointing inside a folder
can attempt to initialize its `BACK` pseudo-title as a game (`arch=folder`) and
crash. By default the harness starts from `jp/_root`. `start --resume-pack` keeps
the previous pack for reproducing this issue; it does not fix the console code.

CD-ROM games, USB insertion/removal, real controllers, save-state round trips
and long sessions are not validated by these checks. OpenAL output to the
desktop's stereo device has been checked through the Pulse backend; `--silent`
uses OpenAL's null driver for tests that should not produce sound.

This tests folder/content behavior, not A33 speed or memory use. QEMU, the VM,
readback display buffers and the 32 MiB command ring + 4 MiB response area all add
overhead that the A33 does not have. Chronos's existing folder limits still
apply (one folder level and up to 49 games plus the back entry per folder).

Run the asset-free preparation safety tests on macOS or Linux:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s test-environment -v
```

They check input preservation, private saves, rejection of existing destinations
and symlinks, incomplete packs, missing scripts, unsupported binaries and PID
reuse, desktop audio-server selection and explicit/silent audio overrides.
These are complementary to the interactive VM checks.

The ARM32 descriptor-reuse regression can also be run inside the VM, without
starting the engine or graphics proxy:

```sh
python3 test-environment/parallels.py build check
```

This runs the Python tests on Linux, then opens/closes the emulated I2C device
and verifies that a normal file reusing the same descriptor retains its contents.
