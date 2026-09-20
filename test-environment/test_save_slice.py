"""SRAM persistence and asynchronous IO regressions; synthetic data, no ROMs."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

HOOK = Path(__file__).resolve().parents[1] / 'console-mod/hook-src'
OFFSET, LENGTH = 0x5e80, 8448 * 150


class SaveSliceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.root = Path(cls.temp.name)
        source = cls.root / 'probe.c'
        source.write_text('''#include <stdlib.h>
#include <unistd.h>
#include <errno.h>
static int test_fsync(int fd) {
    if (getenv("FAIL_SYNC")) { errno = EIO; return -1; }
    return fsync(fd);
}
#define fsync test_fsync
#include "save_slice.h"
int main(int argc, char **argv) {
    if (argc != 4) return 2;
    int rc = argv[1][0] == 'o'
        ? save_slice_export(argv[2], argv[3], 0x5e80, 8448*150)
        : save_slice_import(argv[2], argv[3], 0x5e80, 8448*150);
    printf("%d\\n", rc);
    return 0;
}
''')
        cls.exe = cls.root / 'probe'
        subprocess.run([os.environ.get('CC', 'cc'), '-std=c99',
                        '-D_POSIX_C_SOURCE=200809L', '-Wall', '-Wextra', '-Werror',
                        '-O2', '-I'+str(HOOK), str(source), '-o', str(cls.exe)], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def setUp(self):
        self.live = self.root / 'live'
        self.pack = self.root / 'pack'
        self.body = (bytes(range(256)) * ((LENGTH + 255)//256))[:LENGTH]
        self.live.write_bytes(b'P'*OFFSET + self.body + b'tail settings')
        if self.pack.exists(): self.pack.unlink()

    def run_slice(self, direction, expected, fail_sync=False):
        env = os.environ.copy()
        if fail_sync: env['FAIL_SYNC'] = '1'
        result = subprocess.check_output([str(self.exe), direction, str(self.live),
                                          str(self.pack)], env=env, text=True)
        self.assertEqual(int(result), expected)

    def test_export_roundtrip_and_no_rewrite_when_equal(self):
        self.run_slice('out', 1)
        self.assertEqual(self.pack.read_bytes(), self.body)
        os.utime(self.pack, (100, 100))
        old = self.pack.stat()
        self.run_slice('out', 0)
        self.assertEqual(self.pack.stat().st_ino, old.st_ino)
        self.assertEqual(self.pack.stat().st_mtime_ns, old.st_mtime_ns)
        self.assertFalse(self.pack.with_suffix('.tmp').exists())

    def test_changed_export_is_atomic_and_exact_size(self):
        for previous in [b'', b'x'*LENGTH, self.body+b'extra']:
            self.pack.write_bytes(previous)
            self.run_slice('out', 1)
            self.assertEqual(self.pack.read_bytes(), self.body)

    def test_failed_sync_preserves_previous_snapshot(self):
        self.pack.write_bytes(b'previous snapshot')
        self.run_slice('out', -1, fail_sync=True)
        self.assertEqual(self.pack.read_bytes(), b'previous snapshot')
        self.assertFalse(self.pack.with_suffix('.tmp').exists())

    def test_truncated_live_does_not_destroy_snapshot(self):
        self.live.write_bytes(b'truncated')
        self.pack.write_bytes(b'previous snapshot')
        self.run_slice('out', -1)
        self.assertEqual(self.pack.read_bytes(), b'previous snapshot')

    def test_import_preserves_other_settings_and_inode(self):
        self.pack.write_bytes(b'new save' + b'\0'*(LENGTH-8))
        inode = self.live.stat().st_ino
        self.run_slice('in', 1)
        self.assertEqual(self.live.stat().st_ino, inode)
        self.assertEqual(self.live.read_bytes(), b'P'*OFFSET+self.pack.read_bytes()+b'tail settings')
        os.utime(self.live, (100, 100))
        self.run_slice('in', 0)
        self.assertEqual(self.live.stat().st_mtime, 100)

    def test_missing_and_short_snapshot_are_zero_padded(self):
        self.run_slice('in', 1)
        self.assertEqual(self.live.read_bytes()[OFFSET:OFFSET+LENGTH], b'\0'*LENGTH)
        self.pack.write_bytes(b'abc')
        self.run_slice('in', 1)
        self.assertEqual(self.live.read_bytes()[OFFSET:OFFSET+LENGTH], b'abc'+b'\0'*(LENGTH-3))

    def test_read_errors_are_not_treated_as_empty_sram(self):
        self.pack.mkdir()
        previous = self.live.read_bytes()
        try:
            self.run_slice('in', -1)
            self.assertEqual(self.live.read_bytes(), previous)
        finally:
            self.pack.rmdir()


class FolderWorkerTests(unittest.TestCase):
    def test_io_does_not_block_polling_and_workers_are_reaped(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'worker.c'
            source.write_text('''#include <assert.h>
#include <time.h>
#include "folder_worker.h"
static int work(const char *lineup, const char *dir) {
    struct timespec delay = {0, 20000000};
    nanosleep(&delay, NULL);
    assert(!strcmp(lineup, "jp"));
    return !strcmp(dir, "FOLDER_FAIL") ? -1 : 0;
}
int main(void) {
    struct folder_worker w = FOLDER_WORKER_INIT;
    for (int i = 0; i < 50; ++i) {
        assert(folder_worker_begin(&w, "jp", i%2 ? "FOLDER_FAIL" : "_root", work) == 0);
        assert(folder_worker_begin(&w, "us", "_root", work) == EBUSY);
        int ticks = 0, rc;
        struct timespec frame = {0, 1000000};
        while ((rc = folder_worker_poll(&w)) == 1) { ++ticks; nanosleep(&frame, NULL); }
        assert(ticks > 0);
        assert(rc == (i%2 ? -1 : 0));
        assert(!w.active);
        assert(folder_worker_poll(&w) == -1);
    }
    pthread_mutex_destroy(&w.lock);
    return 0;
}
''')
            exe = root/'worker'
            subprocess.run([os.environ.get('CC', 'cc'), '-std=c99', '-D_POSIX_C_SOURCE=200809L',
                            '-Wall', '-Wextra', '-Werror', '-pthread', '-I'+str(HOOK),
                            str(source), '-o', str(exe)], check=True)
            subprocess.run([str(exe)], check=True, timeout=10)


if __name__ == '__main__':
    unittest.main()
