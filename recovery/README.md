# PCE Mini Recovery for Linux

Linux desktop port of the existing macOS recovery tool. It includes only:

- Boot ROM → FEL → RAM recovery startup over USB.
- Native Linux RNDIS network setup.
- Individual partition backups/restores and a sequential backup of all partitions.
- Raw images and Stored/Deflate ZIP/ZIP64 input, including explicit selection when several images match.
- Transfer progress and SHA-1 verification against the console.

There is no UART terminal, serial-port dependency, macOS bridge or background
bridge service. `nusb` accesses FEL through Linux USBFS; after recovery boots,
the kernel's `rndis_host` driver owns the USB network interface. The application
must run as a normal desktop user.

## Run

Use an x86_64 Linux desktop (Ubuntu 22.04 or newer, or current Arch Linux).
The AppImage includes recovery payloads, the GUI and its small network helper.
GTK and WebKit are bundled. The system supplies its normal graphics drivers,
Fontconfig, `iproute2`, and Polkit with a desktop authentication agent.

```sh
chmod +x PCE-Mini-Recovery-linux-x86_64.AppImage
./PCE-Mini-Recovery-linux-x86_64.AppImage
```

When FUSE is unavailable, prefix the launch command with
`APPIMAGE_EXTRACT_AND_RUN=1`.

Install the USB access rule once (provided beside the AppImage and inside it):

```sh
sudo install -m 0644 70-pce-recovery.rules /etc/udev/rules.d/70-pce-recovery.rules
sudo udevadm control --reload-rules
```

Then unplug and reconnect the console USB cable. The rule grants the active
local desktop user access only to `1f3a:efe8` (Boot ROM/FEL). A remote SSH-only
Linux session does not receive the desktop `uaccess` ACL automatically.

1. Connect the console's micro-USB port using a data cable and leave the power
   switch **OFF**. Connect only one console.
2. Click **Start recovery** while the switch is still **OFF**. Wait for the
   **Ready — switch the console ON now** prompt, then switch it **ON**. The
   application detects the brief startup USB probe and sends the trigger
   immediately; there is no further confirmation during this window. It waits
   for FEL re-enumeration, identifies the SoC, then loads the Chronos recovery
   files into RAM. This action does not flash an eMMC partition. If detection
   times out, switch OFF and repeat this sequence.
3. Wait for the USB network interface to appear, then click **Connect USB
   network**. Approve the Linux authentication dialog. The helper validates
   the selected interface's USB identity (`04e8:6863`) and RNDIS driver before
   adding host address `169.254.13.36/32` and a route to `169.254.13.37/32`.
   It does not add a gateway or change the default route or DNS.
4. When the console is connected, use **Dump** or **Restore** on the desired row.
   A restore always shows the file, ZIP member and destination before confirmation.

Linux normally loads `rndis_host` automatically. If it is unavailable, install
or enable the corresponding kernel module; `sudo modprobe rndis_host` can load
an installed module. The network configuration is temporary and may need to be
reapplied after unplugging/reconnecting the console. No network bridge is created.

Without Polkit, configure the detected USB interface manually:

```sh
sudo ip link set dev INTERFACE up
sudo ip address replace 169.254.13.36/32 dev INTERFACE
sudo ip route replace 169.254.13.37/32 dev INTERFACE scope link src 169.254.13.36
```

Replace `INTERFACE` with the recovery USB interface displayed by the application.

## Backup and restore behavior

Only one recovery/network/partition operation runs at a time. The remote
partition size is checked against the expected layout before a transfer. A
restore also requires a RAM-root recovery and an unmounted target. Full-device
restores reject any mounted eMMC partition.

Existing backup files are not overwritten. A dump is written to a temporary file
in the destination directory, checked against the console, and only then renamed
to the chosen filename. Failed dumps do not leave a success-looking final image.
Batch backups report individual failures rather than claiming all succeeded.

The entire restore image is validated locally, including ZIP CRC and size,
before the console writer starts. As in the Mac tool, restore currently holds
the uncompressed image in RAM (100 MiB for P7, approximately 2.2 GiB for P9).
The device is synced and its SHA-1 checksum checked after writing. SHA-1 is used
as a transfer-integrity checksum, not as firmware authenticity verification.

The port's automated tests cover image validation, restore confirmation,
operation locking, USB interface identification, partition preflight and Linux
window rendering. Physical FEL startup and partition transfers still require
validation with a console connected to a Linux host; a successful GUI test is
not a hardware test.

## Build

Source crates are ported from `pce_recovery_mac`. The proven recovery files are
fetched from `Zorglub51/Project-Chronos` at commit
`f8654289657f78910ea4bc7f9862bfc62972147c` and checked against
`packaging/payloads.sha256`. No console dumps or save files are included.

On Ubuntu 22.04:

```sh
sudo apt-get install build-essential pkg-config cmake libwebkit2gtk-4.1-dev \
  libgtk-3-dev libssl-dev librsvg2-dev libxdo-dev patchelf libfuse2
# Install Rust and Node.js, then put the verified recovery files in payloads/.
cargo build --release -p linux-network --locked
cargo test --workspace --locked
node --test apps/pce-gui/ui/*.test.cjs
cd apps/pce-gui
npm ci
npm run build -- --ci --bundles appimage -- --locked
```

The GitHub workflow also runs `editor/packaging/appimage/finalize.sh` to remove
outdated graphics-driver libraries from Tauri's bundle, preserving compatibility
with newer Mesa on Arch. Do not distribute the intermediate AppImage without
that finalization step.
