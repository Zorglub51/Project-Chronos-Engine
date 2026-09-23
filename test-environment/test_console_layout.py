"""Native package selection: synthetic trees, no original M2 assets."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class ConsoleLayoutTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.build = tempfile.TemporaryDirectory()
        source = Path(cls.build.name) / "probe.c"
        source.write_text('''#include "console_layout.h"
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    const struct console_layout *layout = console_layout_detect(argv[1]);
    if (!layout) { puts("invalid"); return 0; }
    char motion[80];
    snprintf(motion, sizeof(motion), layout->motion_format, "jp");
    printf("%s %s\\n", layout->directory, motion);
    return 0;
}
''')
        cls.exe = Path(cls.build.name) / "probe"
        header = Path(__file__).resolve().parents[1] / "console-mod/hook-src"
        subprocess.run([os.environ.get("CC", "cc"), "-std=c99", "-O2", "-Wall", "-Wextra", "-Werror",
                        "-I" + str(header), str(source), "-o", str(cls.exe)], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.build.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def tree(self, model):
        prefix = "jp" if model == "040" else "us"
        for name in ("config/title_prof.psb.m", "config/title_mode_top.psb.m",
                     f"motion/title_{prefix}_titleselect_jp.psb.m",
                     f"motion/title_{prefix}_titleselect_us.psb.m"):
            path = self.root / model / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"fixture")

    def detected(self):
        return subprocess.check_output([str(self.exe), str(self.root)], text=True).strip()

    def test_both_versions_select_correct_jp_lineup_filename(self):
        self.tree("040")
        self.tree("041")
        for version, result in (("1006JP", "040 title_jp_titleselect_jp.psb.m"),
                                ("1006WW", "041 title_us_titleselect_jp.psb.m")):
            (self.root / "version").write_text(version)
            self.assertEqual(result, self.detected())

    def test_legacy_detection_rejects_ambiguity(self):
        self.tree("041")
        self.assertEqual("041 title_us_titleselect_jp.psb.m", self.detected())
        self.tree("040")
        self.assertEqual("invalid", self.detected())

    def test_incomplete_and_mixed_resources_are_rejected(self):
        self.tree("040")
        (self.root / "version").write_text("1006WW")
        self.assertEqual("invalid", self.detected())
        (self.root / "version").write_text("unknown")
        self.assertEqual("invalid", self.detected())
        (self.root / "version").write_text("1006JP")
        (self.root / "040/motion/title_jp_titleselect_us.psb.m").unlink()
        self.assertEqual("invalid", self.detected())
