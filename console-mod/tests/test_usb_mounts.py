"""Exercise the real shell helpers with simulated mounts, without root/USB."""
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

LIB = Path(__file__).resolve().parents[1] / "pce-usb-lib.sh"
MOCKS = r'''
mount() {
    if [ "$#" = 0 ]; then cat "$MOUNTS"; return; fi
    printf 'mount %s\n' "$*" >> "$EVENTS"
    if [ "$1" = --bind ]; then
        [ "$3" != "$FAIL_TARGET" ] || return 1
        printf '/dev/sda1 on %s type vfat (rw)\n' "$3" >> "$MOUNTS"
    fi
}
umount() {
    [ "$1" != -l ] || shift
    printf 'umount %s\n' "$1" >> "$EVENTS"
    [ "$1" != "$FAIL_UNMOUNT" ] || { echo 'simulated busy mount' >&2; return 1; }
    awk -v t="$1" '{lines[NR]=$0; if ($3 == t) last=NR} END {for(i=1;i<=NR;i++) if(i!=last) print lines[i]}' "$MOUNTS" > "$MOUNTS.new"
    mv "$MOUNTS.new" "$MOUNTS"
}
'''


class UsbMounts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="chronos mounts ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "game").mkdir()
        engine = self.root / "game/m2engage"
        engine.write_text("stub")
        engine.chmod(0o755)
        (self.root / "library/published/roms").mkdir(parents=True)
        saves = self.root / "library/published/save"
        saves.mkdir()
        (saves / "data_008_0000.bin").write_bytes(b"test settings")
        self.mounts = self.root / "mounts"
        self.events = self.root / "events"
        self.mounts.write_text("/dev/sda1 on /mnt/usb type vfat (rw)\n")
        self.events.write_text("")

    def run_shell(self, action, fail=""):
        env = dict(os.environ, USB_TEST_ROOT=str(self.root), MOUNTS=str(self.mounts),
                   EVENTS=str(self.events), FAIL_TARGET=fail)
        script = f'. {shlex.quote(str(LIB))}\n' + MOCKS + r'''
USB_MNT=$USB_TEST_ROOT
USB_GAME=$USB_MNT/game
USB_ROMS=$USB_MNT/library/published/roms
USB_SAVE=$USB_MNT/library/published/save
LOG=$USB_MNT/log
''' + action
        return subprocess.run(["/bin/sh", "-c", script], env=env, capture_output=True, text=True)

    def test_roms_mount_after_game_without_a_copy(self):
        rom = self.root / "library/published/roms/test.pce.m"
        rom.write_bytes(b"ROM")
        result = self.run_shell("pce_apply_bind")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.events.read_text().splitlines(), [
            f"mount --bind {self.root}/game /usr/game",
            f"mount --bind {self.root}/library/published/roms /usr/game/system/roms",
            f"mount --bind {self.root}/library/published/save /usr/game/save",
        ])
        self.assertEqual(list((self.root / "game/system/roms").iterdir()), [])
        self.assertEqual(rom.read_bytes(), b"ROM")

    def test_missing_published_roms_does_not_replace_running_mounts(self):
        (self.root / "library/published/roms").rmdir()
        result = self.run_shell("pce_apply_bind")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.events.read_text(), "")

    def test_rom_mount_failure_rolls_back_game_mount(self):
        result = self.run_shell("pce_apply_bind", fail="/usr/game/system/roms")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn(" on /usr/game ", self.mounts.read_text())
        self.assertTrue(self.events.read_text().endswith("umount /usr/game\n"))

    def test_save_mount_failure_rolls_back_rom_and_game_mounts(self):
        result = self.run_shell("pce_apply_bind", fail="/usr/game/save")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn(" on /usr/game", self.mounts.read_text())

    def test_legacy_live_saves_are_not_hidden_by_the_new_mount(self):
        saves = self.root / "game/save"
        saves.mkdir()
        (saves / "data_008_0000.bin").write_bytes(b"legacy save")
        result = self.run_shell("pce_apply_bind")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.events.read_text(), "")
        self.assertEqual((saves / "data_008_0000.bin").read_bytes(), b"legacy save")

    def test_unmount_children_first_and_leave_other_disks_alone(self):
        with self.mounts.open("a") as f:
            f.write("/dev/mmcblk0p9 on /usr/game type ext4 (rw)\n")
            for path in ("/usr/game", "/usr/game/system/roms", "/usr/game/040/config/title_prof.psb.m", "/other"):
                f.write(f"/dev/sda1 on {path} type vfat (rw)\n")
        result = self.run_shell("pce_drop_game_binds")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.events.read_text().splitlines(), [
            "umount /usr/game/system/roms",
            "umount /usr/game/040/config/title_prof.psb.m",
            "umount /usr/game",
        ])
        self.assertIn(" on /other ", self.mounts.read_text())
        self.assertIn(" on /mnt/usb ", self.mounts.read_text())
        self.assertIn("/dev/mmcblk0p9 on /usr/game ", self.mounts.read_text())

    def test_failed_unmount_is_reported_without_claiming_success(self):
        with self.mounts.open("a") as f:
            f.write("/dev/sda1 on /usr/game type vfat (rw)\n")
        self.run_shell('FAIL_UNMOUNT=/usr/game\nPCE_USB_DEBUG=1\npce_drop_game_binds')
        log = (self.root / "log").read_text()
        self.assertIn("UNMOUNT FAILED target=/usr/game rc=1", log)
        self.assertIn("simulated busy mount", log)
        self.assertNotIn("popped /usr/game", log)
        self.assertIn(" on /usr/game ", self.mounts.read_text())


if __name__ == "__main__":
    unittest.main()
