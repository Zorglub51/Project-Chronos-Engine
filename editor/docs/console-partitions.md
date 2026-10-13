# Console backups and partitions

Open **Console backups and partitions…** on the editor's home screen. The
library options remain available, and **Home** returns to this screen from an
open library after saving pending edits.

This first console-management page handles MMC backups and image restoration.
Hack installation and integrated recovery startup are separate follow-up work.

## Connect

Connect the console's micro-USB port using a data cable. Start the existing
recovery utility and establish its USB network connection first, then use
**Check connection** in the editor. The console address is `169.254.13.37`;
the transport uses the recovery image's SSH service and a temporary TCP data
channel. Recovery may regenerate its SSH host key on each boot.

The Rust recovery crate is linked into the editor executable; SSH and TCP do
not invoke an external recovery or command-line SSH process. The transfer uses
SSH on port 22 and BusyBox `nc` on TCP port 5001, followed by SHA-1 verification.
RAM recovery provides the required BusyBox applets. Chronos' normal P7 BusyBox
does not include `nc`, and normal firmware remains inspection-only here.
Listener detection supports IPv4-only kernels, where `/proc/net/tcp6` is absent.

Transfers require a verified RAM-root recovery environment and an unmounted
target. The editor rejects normal firmware and incomplete device information.
A full MMC transfer requires all of its partitions to be unmounted. Mounted
aliases and bind mounts are checked by their underlying device numbers.

Opening the page only displays the known partition layout. A connection check
reads diagnostic information; it does not flash a partition or start recovery.

## Back up

Select numbered partitions for separate image files, or the full MMC for a
single complete image, then choose the destination folder. P3/P4 are extended
partition-table slots and have no separate image in the factory layout.

Backups are streamed to temporary files. The transferred length and console
checksum are checked before a final image is saved. Existing images are never
overwritten. Successful batches include a `chronos-backup.json` manifest with
the verified checksums. A batch stops at the first failure and keeps the images
already verified and saved; the error identifies the failed partition.

Use RAM recovery for a stable backup: a checksum alone cannot guarantee a
consistent snapshot of a filesystem that is changing during the transfer.

## Restore

1. Select the destination partition or full MMC.
2. Choose a raw image or ZIP archive. Stored/Deflate ZIP and ZIP64 are supported.
3. If several ZIP images match the required size, choose the intended member.
4. Wait for the entire image to be staged and checked locally. Review the
   source file, ZIP member, target, byte count and SHA-256.
5. Confirm replacement of the displayed target and click its Restore button.

The selected image is staged into a private, anonymous temporary file before confirmation.
The restore uses that checked snapshot, so changing the source file afterwards
cannot substitute another image. CRC and exact-size errors stop preparation
before any console writer is opened. Staging needs free temporary disk space
for the uncompressed image, but does not allocate image-sized RAM buffers.

Partition size is a validation check, not identification: P2 and P6 are both
8 MiB. Always review the destination and ZIP member explicitly. A full MMC
restore replaces the entire internal device, including its partition table.

Only one console operation runs at a time. Navigation and normal application
closure are blocked during transfers. Keep power and the data cable connected
until verification finishes. The console is synced after a restore and its
checksum is compared with the sent image. Verification failure is reported as
failure, even if bytes have already been written.

## Validation

Static protocol checks on 2026-10-12 inspected the exact recovery payload used
by the existing Mac utility: `boot.img.tftp.cpio.xz`, SHA-256
`f1156e0d12af154aed7efdca4fc5852f312b8b83c8e92f255a5cdbfa50151bcd`.
Its BusyBox 1.29.0.git provides the required `nc -l -p`, `stat -c %t:%T`,
`awk`, `readlink`, `sha1sum` and shell commands. Dropbear 2018.76 supports the
SSH algorithms allowed by the Rust client, with an empty root password and
port 22 configured via inetd. The payload's `hakchi-shell` command line keeps
the maintenance shell in RAM. These facts describe this payload, not the
separate newer clean-recovery backups or Chronos' normal P7 system.

Automated tests exercise image CRC/size validation, streaming, mounted-device
rejection, confirmation, operation locking and interface navigation. Physical
USB-network transfers need a console in RAM recovery; UI or unit tests do not
establish hardware validation.
