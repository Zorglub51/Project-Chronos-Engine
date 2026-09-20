# SSH/SFTP on the A33

The P7 image starts SSH and SFTP at **169.254.13.37:22**, user **root**, with an
empty password in `/etc/shadow`. The server binds only to that USB address.

BusyBox from stock P7 continues to provide the shell and basic Linux commands.
SSH is upstream Dropbear 2026.94; the SFTP helper is from OpenSSH 10.5p1. Both
are cross-compiled for ARMv7 hard-float with static musl 1.2.6, so no stock
libraries are replaced. The three stripped binaries total 336,168 bytes:

| Program | Installed path | Bytes |
|---|---|---:|
| Dropbear server | `/usr/sbin/dropbear` | 194,352 |
| Key generation | `/usr/bin/dropbearkey` | 54,560 |
| SFTP helper | `/usr/libexec/sftp-server` | 87,256 |

Only Dropbear stays resident. SFTP runs on demand during a transfer. SSH host
keys are Ed25519; X11, agent and TCP forwarding are omitted. No FTP, Telnet or
web terminal daemon is installed.

`S30chronos-remote` starts after the stock randomness initialization and runs
USB setup/key generation in the background. The RNDIS USB identity matches the
existing Mac bridge (VID 04e8, PID 6863). Per-console keys are generated once in
`/rootfs_data/chronos/ssh` on P8; the image contains no private host key. If this
storage is unavailable, the service logs that it is using a volatile key in
`/run/chronos-ssh`. Runtime state and logs use `/run` and `/tmp`, allowing P7 to
keep the stock read-only boot. Startup logs are at `/tmp/chronos-remote.log`.

The A33 3.4.113 kernel does not expose the gadget's `iManufacturer`, `iProduct`
or `iSerial` attributes. These optional strings are only written when present;
missing RNDIS controls, VID/PID or enable attributes still abort setup and name
the missing attribute in the log. The first image incorrectly required all
three strings, leaving USB disabled before SSH could start. The v2 image fixes
this; the SSH/SFTP binaries are unchanged.

On macOS, use **Start bridge** in PCE Mini Recovery to connect the normal-running
console. **Start recovery** boots a separate RAM image and does not start the
services in this P7. Then use `ssh root@169.254.13.37` or
`sftp root@169.254.13.37`; graphical SFTP clients use port 22 and a blank password.

Writable game files and saves are under `/mnt/usb/library/published`. To make an
administrative edit to the P7 filesystem itself, run `mount -o remount,rw /` from
SSH and finish with `sync` followed by `mount -o remount,ro /`.

## Rebuilding

`tools/build-remote-access.py` pins upstream release URLs and SHA-256 hashes.
Download those three archives from the recorded URLs to a source directory.
On Linux with `gcc-arm-linux-gnueabihf`, `make`, Python 3.12+ and binutils:

```sh
python3 tools/build-remote-access.py --sources /path/to/archives \
  --work /tmp/chronos-remote-build --output console-mod/remote-access/bin
```

Use a new work directory. Builds are local to that directory; nothing is
installed into the build host. The output manifest records source identities,
binary hashes and compile-time options. Upstream licences are retained under
`bin/licenses/` and installed into `/usr/share/chronos-remote/` in P7.

Build just the updated P7 without rebuilding the prepared USB library:

```sh
python3 tools/build-test-kit.py --p7-only \
  --p7 /path/to/verified-stock-p7.bin --output /path/to/new-kit \
  --debugfs /path/to/debugfs --fsck /path/to/e2fsck
```

The builder changes only the root password field, preserves other shadow fields
and accounts, and keeps `/etc/passwd`, `rcS`, `inittab` and `fstab` unchanged.

## Validation

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s console-mod/tests -v
sudo python3 test-environment/validate-remote-access.py /path/to/new-kit
```

The Linux test uses private user/mount/network/PID namespaces and its own ARM
interpreter registration. It runs the real ARM binaries against the P7's shell
and libraries, using dummy RNDIS/sysfs objects. It checks passwordless root SSH,
USB-address binding, SFTP upload/download/rename/delete, root read-only state,
startup idempotence and host-key persistence. It does not validate the physical
USB gadget or the actual A33 kernel.

Hardware validation on 2026-09-20 confirmed the RNDIS link and passwordless
root SSH on the console's 3.4.113 kernel, plus a 1 MiB SFTP upload, rename,
download and deletion with identical downloaded bytes. P7 was returned to
read-only after installing the fixed helper; the original helper was backed up.
After a full console power cycle, automatic startup, interactive SSH, the same
host key and another 1 MiB SFTP round trip were confirmed. On macOS the recovery
bridge must be started again if it exited while the console was powered off.
