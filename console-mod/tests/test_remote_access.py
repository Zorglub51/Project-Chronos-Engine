"""Check USB scoping, startup failures and per-console host-key persistence."""
import importlib.util
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
LIB = ROOT / 'console-mod/pce-remote-lib.sh'
spec = importlib.util.spec_from_file_location('kit', ROOT / 'tools/build-test-kit.py')
kit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(kit)


class RemoteAccess(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.gadget = self.root / 'gadget'
        self.gadget.mkdir()
        for name in ('enable', 'iManufacturer', 'iProduct', 'iSerial', 'f_rndis/manufacturer',
                     'f_rndis/vendorID', 'f_rndis/wceis', 'idVendor', 'idProduct', 'functions', 'bDeviceClass'):
            p = self.gadget / name
            p.parent.mkdir(exist_ok=True)
            p.write_text('0\n')
        self.events = self.root / 'events'
        self.events.touch()

    def run_shell(self, extra='', fail=''):
        script = '. ' + shlex.quote(str(LIB)) + '\n' + r'''
GADGET=$TEST_ROOT/gadget
KEY_DIR=$TEST_ROOT/keys
REMOTE_PID=$TEST_ROOT/dropbear.pid
DROPBEAR=test_dropbear
DROPBEARKEY=test_key
ip() {
    echo "ip $*" >> "$TEST_ROOT/events"
    if [ "$FAIL" = interface ] && [ "$1 $2" = 'link show' ]; then return 1; fi
    if [ "$FAIL" = address ] && [ "$1 $2" = 'addr add' ]; then return 1; fi
    return 0
}
sleep() { :; }
sync() { :; }
test_key() {
    echo "keygen $*" >> "$TEST_ROOT/events"
    [ "$FAIL" != key ] || return 1
    printf 'private-test-key' > "$4"
}
test_dropbear() { echo "dropbear $*" >> "$TEST_ROOT/events"; }
''' + extra + '\nremote_start\n'
        return subprocess.run(['/bin/sh', '-c', script], text=True, capture_output=True,
                              env=dict(os.environ, TEST_ROOT=str(self.root), FAIL=fail))

    def test_binds_only_usb_and_reuses_persistent_key(self):
        result = self.run_shell()
        self.assertEqual(result.returncode, 0, result.stderr)
        log = self.events.read_text()
        self.assertIn('ip addr add 169.254.13.37/16 dev rndis0', log)
        self.assertIn('dropbear -E -B -p 169.254.13.37:22 -r ', log)
        self.assertEqual((self.gadget / 'idVendor').read_text(), '04e8\n')
        self.assertEqual((self.gadget / 'idProduct').read_text(), '6863\n')
        self.assertEqual((self.gadget / 'enable').read_text(), '1\n')
        key = self.root / 'keys/dropbear_ed25519_host_key'
        self.assertEqual(key.stat().st_mode & 0o777, 0o600)
        result = self.run_shell()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.events.read_text().count('keygen '), 1)

    def test_missing_gadget_never_starts_ssh(self):
        result = self.run_shell('GADGET=$TEST_ROOT/missing')
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn('dropbear ', self.events.read_text())

    def test_a33_kernel_without_usb_string_attributes(self):
        for name in ('iManufacturer', 'iProduct', 'iSerial'):
            (self.gadget / name).unlink()
        result = self.run_shell()
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        self.assertIn('dropbear -E -B -p 169.254.13.37:22', self.events.read_text())
        for name in ('iManufacturer', 'iProduct', 'iSerial'):
            self.assertFalse((self.gadget / name).exists())

    def test_bad_gadget_node_stops_configuration(self):
        (self.gadget / 'functions').unlink()
        result = self.run_shell()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Missing USB gadget attribute: functions', result.stdout)
        self.assertNotIn('dropbear ', self.events.read_text())

    def test_interface_address_and_key_failures_do_not_start_server(self):
        for failure in ('interface', 'address', 'key'):
            self.events.write_text('')
            result = self.run_shell(fail=failure)
            self.assertNotEqual(result.returncode, 0, failure)
            self.assertNotIn('dropbear ', self.events.read_text())

    def test_shadow_only_changes_root_password_field(self):
        original = 'root:$6$test:18165:0:99999:7:::\nnobody:*:18165:0:99999:7:::\n'
        updated = kit.blank_root_password(original)
        self.assertEqual(updated, 'root::18165:0:99999:7:::\nnobody:*:18165:0:99999:7:::\n')
        self.assertEqual(kit.blank_root_password(updated), updated)
        for bad in ('nobody:*:18165:0:99999:7:::\n', original + original, 'root:x:1\n'):
            with self.assertRaises(RuntimeError): kit.blank_root_password(bad)


if __name__ == '__main__':
    unittest.main()
