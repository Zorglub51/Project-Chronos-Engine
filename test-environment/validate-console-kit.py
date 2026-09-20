#!/usr/bin/env python3
"""Check a generated kit on Linux using disposable FAT/ext4 images and bwrap.

No physical console/disk is opened. Guest loop devices only refer to temporary
files; mounts are private. Uses the P7's actual ARM BusyBox for mount/unmount.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def run(args):
    result = subprocess.run([str(x) for x in args], capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(f"{args[0]} exited {result.returncode}\n{result.stdout}\n{result.stderr}")
    return result.stdout + result.stderr


CHECK = r'''
set -eux
stock_busybox() { qemu-arm-static -L /stock-root -E LD_LIBRARY_PATH=/stock-root/lib:/stock-root/usr/lib /stock-root/bin/busybox "$@"; }
mount() { stock_busybox mount "$@"; }
# The stand-in block devices are loops. Keep their associations for the
# reinsertion test; a physical USB/eMMC device has no loop to release.
umount() { stock_busybox umount -D "$@"; }
. /stock-root/usr/bin/pce-usb-lib.sh

# Reproduce the state AFTER stock rcS and BEFORE the USB cold-plug handler.
mount -t ext4 -o ro /dev/mmcblk0p9 /usr/game
test "$(cat /usr/game/stock-marker)" = stock
mount -t vfat -o rw,sync,uid=0,gid=0,fmask=0022,dmask=0022 /dev/sdz1 /mnt/usb

stock_busybox sort -ru << EOF
/usr/game
/usr/game/save
/usr/game/system/roms
EOF

pce_apply_bind
test "$(find /usr/game/system/roms -maxdepth 1 -type f | wc -l)" -eq 6
printf 'persisted test' > /usr/game/save/chronos-check.txt
test "$(cat /mnt/usb/library/published/save/chronos-check.txt)" = 'persisted test'
test -z "$(ls -A /mnt/usb/game/save)"
test -z "$(ls -A /mnt/usb/game/system/roms)"

# Exercise the same per-file mount layering as the hook.
mount --bind /mnt/usb/library/published/folders/jp/_root/title_prof.psb.m /usr/game/040/config/title_prof.psb.m
cmp /usr/game/040/config/title_prof.psb.m /mnt/usb/library/published/folders/jp/_root/title_prof.psb.m

# Resolve the stock engine + bundled hook using the stock ARM dynamic loader.
qemu-arm-static -L /stock-root \
    -E LD_LIBRARY_PATH=/stock-root/lib:/stock-root/usr/lib:/usr/game \
    -E LD_PRELOAD=/usr/game/lib/m2hook_print.so \
    -E LD_TRACE_LOADED_OBJECTS=1 /usr/game/m2engage

pce_drop_binds
test "$(cat /usr/game/stock-marker)" = stock
! mount | grep -q '/dev/sdz1 on /usr/game'
umount /mnt/usb

# A second insertion must restore the same files, with no duplicate layers.
mount -t vfat -o rw,sync,uid=0,gid=0,fmask=0022,dmask=0022 /dev/sdz1 /mnt/usb
pce_apply_bind
test "$(cat /usr/game/save/chronos-check.txt)" = 'persisted test'
test "$(mount | grep -c '/dev/sdz1 on /usr/game')" -eq 3
pce_drop_binds
test "$(cat /usr/game/stock-marker)" = stock
umount /mnt/usb
umount /usr/game
echo 'PASS: stock ARM mounts, ROM/save visibility, file overlay, detach, reinsertion, loader'
'''


def check(kit):
    with tempfile.TemporaryDirectory(prefix="chronos-kit-check-") as tmp:
        root = Path(tmp)
        stock = root / "stock-root"
        stock.mkdir()
        extraction = run(["debugfs", "-R", f"rdump / {stock}", kit / "p7/mmcblk0p7-chronos.bin"])
        # rdump does not recreate device nodes, which are not needed here.
        if not (stock / "lib/ld-linux-armhf.so.3").exists():
            raise RuntimeError("Stock loader missing after extraction\n" + extraction)
        (root / "check.sh").write_text(CHECK)
        usb = root / "usb.fat"
        app = root / "app.ext4"
        for image, size in ((usb, 512 * 1024 * 1024), (app, 16 * 1024 * 1024)):
            with image.open("wb") as f:
                f.truncate(size)
        run(["mkfs.vfat", "-F", "32", usb])
        run(["mkfs.ext4", "-q", "-F", app])
        loops = []
        mounts = []
        try:
            for image, name in ((usb, "usb-mount"), (app, "app-mount")):
                loop = run(["losetup", "--find", "--show", image]).strip()
                loops.append(loop)
                dest = root / name
                dest.mkdir()
                run(["mount", loop, dest])
                mounts.append(dest)
                if image == usb:
                    shutil.copytree(kit / "USB", dest, dirs_exist_ok=True)
                else:
                    (dest / "stock-marker").write_text("stock")
                run(["umount", dest])
                mounts.remove(dest)

            cmd = ["bwrap", "--die-with-parent", "--unshare-pid", "--unshare-ipc"]
            for path in sorted(Path("/").iterdir()):
                if path.name in {"usr", "tmp", "dev", "proc", "mnt", "rootfs_data"}:
                    continue
                if path.is_symlink():
                    cmd += ["--symlink", os.readlink(path), str(path)]
                else:
                    cmd += ["--ro-bind", str(path), str(path)]
            cmd += ["--dir", "/usr"]
            for path in sorted(Path("/usr").iterdir()):
                if path.name != "game":
                    cmd += ["--ro-bind", str(path), str(path)]
            cmd += ["--dir", "/usr/game", "--dir", "/mnt/usb", "--dir", "/rootfs_data",
                    "--ro-bind", str(stock), "/stock-root", "--ro-bind", str(root), "/work",
                    "--tmpfs", "/tmp", "--dev", "/dev", "--proc", "/proc",
                    "--dev-bind", loops[0], "/dev/sdz1", "--dev-bind", loops[1], "/dev/mmcblk0p9",
                    "--dir", "/dev/by-name", "--symlink", "/dev/mmcblk0p9", "/dev/by-name/app",
                    "--cap-add", "CAP_SYS_ADMIN", "/bin/sh", "/work/check.sh"]
            try:
                output = run(cmd)
            except RuntimeError as error:
                (kit / "vm-validation-failed.log").write_text(str(error))
                raise
            (kit / "vm-validation.log").write_text(output)
            run(["fsck.vfat", "-n", usb])
            return {"passed": True, "guest_kernel": os.uname().release,
                    "stock_arm_busybox_mounts": True, "fat32_rom_and_save_mounts": True,
                    "stock_engine_and_hook_dependencies_resolved": True,
                    "overlay_detach_and_reinsertion": True, "physical_console_tested": False}
        finally:
            for dest in reversed(mounts):
                subprocess.run(["umount", str(dest)], check=False)
            for loop in reversed(loops):
                subprocess.run(["losetup", "-d", loop], check=False)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("kit", type=Path)
    p.add_argument("--private", action="store_true", help=argparse.SUPPRESS)
    args = p.parse_args()
    if os.geteuid() != 0:
        raise SystemExit("Run inside the Linux VM with sudo")
    if not args.private:
        return subprocess.call(["unshare", "--mount", "--propagation", "private", sys.executable,
                                str(Path(__file__).resolve()), str(args.kit.resolve()), "--private"])
    result = check(args.kit.resolve())
    (args.kit / "vm-validation.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
