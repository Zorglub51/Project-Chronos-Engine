"""Safety/regression checks with synthetic data; no original assets needed."""
import contextlib
import hashlib
import importlib.util
import io
import os
from pathlib import Path
import socket
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import native

spec = importlib.util.spec_from_file_location("install_local", Path(__file__).with_name("install-local.py"))
install_local = importlib.util.module_from_spec(spec)
spec.loader.exec_module(install_local)


class PrepareTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.data = self.base / "stock"
        self.published = self.base / "published"
        self.support = self.base / "support"
        self.repo = self.base / "repo"
        self.destination = self.base / "test copy with spaces"
        self.put(self.data / "system/script/init.nut.m", b"stock init")
        self.put(self.data / "040/config/title_prof.psb.m", b"stock config")
        self.put(self.data / "system/roms/stock.pce.m", b"stock ROM")
        self.put(self.support / "version", b"1006JP")
        self.put(self.support / "shutdown.png", b"image")
        self.binary = self.support / "m2engage"
        self.put(self.binary, b"synthetic engine")
        self.pack = self.published / "folders/jp/_root"
        for name in ("title_prof.psb.m", "title_mode_top.psb.m", "title_jp_titleselect_jp.psb.m"):
            self.put(self.pack / name, b"published pack")
        self.put(self.pack / "saves/real-save.bin", b"important user save")
        self.put(self.published / "roms/custom.pce", b"custom ROM")
        self.put(self.published / "folders/.current", b"jp/FOLDER_OTHER\n")
        for name in native.PATCHES:
            self.put(self.repo / "mod-assets/scripts-built" / name, b"Chronos patch")
        self.args = SimpleNamespace(runtime=self.destination, data=self.data, published=self.published,
                                    binary=self.binary, support=self.support)
        digest = hashlib.sha256(self.binary.read_bytes()).hexdigest()
        for mock in (patch.object(native, "REPO", self.repo),
                     patch.dict(native.SUPPORTED_BINARIES, {digest: "test fixture"}, clear=True)):
            mock.start()
            self.addCleanup(mock.stop)

    @staticmethod
    def put(path, data):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)

    def prepare(self):
        with contextlib.redirect_stdout(io.StringIO()):
            native.prepare(self.args)

    def snapshot_sources(self):
        return {p: p.read_bytes() for tree in (self.data, self.published, self.support, self.repo)
                for p in tree.rglob("*") if p.is_file()}

    def test_preparation_preserves_inputs_and_excludes_user_saves(self):
        before = self.snapshot_sources()
        self.prepare()
        self.assertEqual(before, self.snapshot_sources())
        self.assertEqual([], list((self.destination / "game/save").iterdir()))
        self.assertEqual([], list((self.destination / "published/folders/jp/_root/saves").iterdir()))
        self.assertEqual("jp/_root\n", (self.destination / "published/folders/.current").read_text())
        self.assertTrue((self.destination / "game/system/roms/custom.pce").is_symlink())
        self.assertTrue((self.destination / "game/system/roms/stock.pce.m").is_symlink())
        private_config = self.destination / "game/040/config/title_prof.psb.m"
        private_config.write_bytes(b"modified privately")
        self.assertEqual(before, self.snapshot_sources())
        self.assertEqual(self.destination.resolve(), native.runtime(self.destination))

    def test_existing_directory_is_not_overwritten(self):
        self.put(self.destination / "valuable", b"keep")
        with self.assertRaisesRegex(RuntimeError, "already exists"):
            self.prepare()
        self.assertEqual(b"keep", (self.destination / "valuable").read_bytes())

    def test_dangling_destination_symlink_is_not_followed(self):
        target = self.base / "not-created"
        self.destination.symlink_to(target)
        with self.assertRaisesRegex(RuntimeError, "already exists"):
            self.prepare()
        self.assertFalse(target.exists())

    def test_output_under_input_is_rejected_before_writing(self):
        self.args.runtime = self.data / "output"
        with self.assertRaisesRegex(RuntimeError, "outside input"):
            self.prepare()
        self.assertFalse(self.args.runtime.exists())

    def test_wrong_binary_fails_before_copy(self):
        self.binary.write_bytes(b"unknown firmware")
        with self.assertRaisesRegex(RuntimeError, "Unsupported binary"):
            self.prepare()
        self.assertFalse(self.destination.exists())

    def test_incomplete_pack_fails_before_copy(self):
        (self.pack / "title_mode_top.psb.m").unlink()
        with self.assertRaisesRegex(RuntimeError, "Incomplete pack"):
            self.prepare()
        self.assertFalse(self.destination.exists())

    def test_missing_patch_fails_before_copy(self):
        (self.repo / "mod-assets/scripts-built/utils.nut.m").unlink()
        with self.assertRaisesRegex(RuntimeError, "Missing Chronos script"):
            self.prepare()
        self.assertFalse(self.destination.exists())

    def test_pid_reuse_does_not_target_an_unrelated_process(self):
        self.destination.mkdir()
        (self.destination / "session.json").write_text('{"pid": 123, "start_time": "old"}')
        with patch.object(native, "identity", return_value="new"):
            self.assertIsNone(native.active(self.destination))

    def test_local_install_survives_removal_of_original_resources(self):
        self.destination = self.base / "standalone/runtime"
        self.args.runtime = self.destination
        self.prepare()
        base = self.destination.parent
        for name in ("gl_proxy", "libMali.so", "m2hook_print.so"):
            self.put(base / "build" / name, b"local binary")
        self.put(self.destination / "game/save/private-save.bin", b"keep this save")
        before = self.snapshot_sources()
        with contextlib.redirect_stdout(io.StringIO()):
            install_local.install(base)
        self.assertEqual(before, self.snapshot_sources())
        self.assertFalse(any(p.is_symlink() for p in self.destination.rglob("*")))
        for source in (self.data, self.published):
            for p in source.rglob("*"):
                if p.is_file():
                    p.unlink()
        self.assertEqual(b"custom ROM", (self.destination / "game/system/roms/custom.pce").read_bytes())
        self.assertEqual(b"keep this save", (self.destination / "game/save/private-save.bin").read_bytes())
        self.assertTrue((base / "native.py").is_file())
        self.assertTrue((base / "chronos").stat().st_mode & 0o111)

    def test_local_install_refuses_an_active_session(self):
        self.destination = self.base / "standalone/runtime"
        self.args.runtime = self.destination
        self.prepare()
        with patch.object(native, "active", return_value={"pid": 123}):
            with self.assertRaisesRegex(RuntimeError, "Stop this test session"):
                install_local.install(self.destination.parent)
        self.assertFalse((self.destination.parent / "chronos").exists())


class AudioEnvironmentTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def server(self, uid):
        path = self.root / str(uid) / "pulse/native"
        path.parent.mkdir(parents=True)
        connection = socket.socket(socket.AF_UNIX)
        self.addCleanup(connection.close)
        connection.bind(str(path))
        return "unix:" + str(path)

    def test_root_launch_discovers_single_desktop_server(self):
        server = self.server(1000)
        env = {}
        result = native.audio_environment(env, False, self.root)
        self.assertEqual(server, result["PULSE_SERVER"])
        self.assertEqual("pulse", result["ALSOFT_DRIVERS"])
        self.assertEqual({}, env)

    def test_sudo_user_selects_their_own_server(self):
        server = self.server(1000)
        self.server(1001)
        result = native.audio_environment({"SUDO_UID": "1000"}, False, self.root)
        self.assertEqual(server, result["PULSE_SERVER"])

    def test_graphical_session_owner_selects_their_server(self):
        server = self.server(os.getuid())
        self.server(os.getuid() + 1)
        auth = self.root / "xauth"
        auth.touch()
        result = native.audio_environment({"XAUTHORITY": str(auth)}, False, self.root)
        self.assertEqual(server, result["PULSE_SERVER"])

    def test_multiple_unidentified_sessions_require_explicit_selection(self):
        self.server(1000)
        self.server(1001)
        with self.assertRaisesRegex(RuntimeError, "set PULSE_SERVER"):
            native.audio_environment({}, False, self.root)

    def test_explicit_settings_are_preserved(self):
        env = {"PULSE_SERVER": "unix:/custom/audio", "ALSOFT_DRIVERS": "alsa"}
        self.assertEqual(env, native.audio_environment(env, False, self.root))

    def test_silent_mode_overrides_driver_without_needing_a_server(self):
        self.server(1000)
        self.server(1001)
        result = native.audio_environment({"ALSOFT_DRIVERS": "pulse"}, True, self.root)
        self.assertEqual("null", result["ALSOFT_DRIVERS"])

    def test_no_server_leaves_automatic_audio_selection(self):
        self.assertEqual({}, native.audio_environment({}, False, self.root))


if __name__ == "__main__":
    unittest.main()
