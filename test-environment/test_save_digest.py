"""Portable regression tests for the native save metadata update (no ROMs)."""
import hashlib
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest

HOOK = Path(__file__).resolve().parents[1] / 'console-mod/hook-src'


def metadata(width=1):
    # A single PSB stream containing the 16-byte Digest, with padding before it.
    buf = bytearray(96)
    struct.pack_into('<4sHH', buf, 0, b'PSB\0', 3, 0)
    struct.pack_into('<III', buf, 24, 44, 56, 80)
    for at, value in [(44, 0), (56, 16)]:
        array = bytes([12+width]) + (1).to_bytes(width, 'little')
        array += bytes([12+width]) + value.to_bytes(width, 'little')
        buf[at:at+len(array)] = array
    buf[80:] = bytes(range(16))
    return bytes(buf)


class SaveDigestTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.root = Path(cls.temp.name)
        source = cls.root / 'probe.c'
        source.write_text('''#include "save_digest.h"
int main(int argc, char **argv) {
    if (argc != 3) return 2;
    return save_digest_refresh(argv[1], argv[2]) == 0 ? 0 : 1;
}
''')
        cls.exe = cls.root / 'probe'
        subprocess.run([os.environ.get('CC', 'cc'), '-std=c99',
                        '-D_POSIX_C_SOURCE=200809L', '-Wall', '-Wextra', '-Werror',
                        '-O2', '-I'+str(HOOK), str(source), '-o', str(cls.exe)], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def refresh(self, data, meta, success=True):
        data_path, meta_path = self.root/'data.bin', self.root/'meta.bin'
        data_path.write_bytes(data)
        meta_path.write_bytes(meta)
        inode = meta_path.stat().st_ino
        result = subprocess.run([str(self.exe), str(data_path), str(meta_path)])
        self.assertEqual(result.returncode, 0 if success else 1)
        self.assertEqual(meta_path.stat().st_ino, inode)
        self.assertEqual(data_path.read_bytes(), data)
        expected = meta[:80] + hashlib.md5(data).digest() if success else meta
        self.assertEqual(meta_path.read_bytes(), expected)

    def test_hash_padding_and_stream_boundaries(self):
        for size in [0, 1, 55, 56, 63, 64, 65, 119, 120, 127, 128,
                     4095, 4096, 4097, 1291396]:
            with self.subTest(size=size):
                self.refresh((bytes(range(256)) * ((size+255)//256))[:size], metadata())

    def test_variable_width_psb_arrays(self):
        for width in range(1, 5):
            with self.subTest(width=width):
                self.refresh(b'abc', metadata(width))

    def test_rejects_bad_metadata_without_modifying_it(self):
        valid = metadata()
        cases = [valid[:n] for n in [0, 4, 43, 44, 56, 79, 80, 95]]
        cases += [b'\0'*400, valid+b'\0'*4096]
        for at, value in [(0, ord('X')), (4, 4), (6, 1), (44, 12), (45, 2),
                          (46, 17), (56, 17), (59, 15)]:
            buf = bytearray(valid)
            buf[at] = value
            cases.append(buf)
        for at in [24, 28, 32]:
            buf = bytearray(valid)
            struct.pack_into('<I', buf, at, 0xffffffff)
            cases.append(buf)
        for n, meta in enumerate(cases):
            with self.subTest(case=n):
                self.refresh(b'abc', meta, success=False)

    def test_missing_data_preserves_metadata(self):
        path = self.root / 'only-meta.bin'
        path.write_bytes(metadata())
        result = subprocess.run([str(self.exe), str(self.root/'missing'), str(path)])
        self.assertEqual(result.returncode, 1)
        self.assertEqual(path.read_bytes(), metadata())


if __name__ == '__main__':
    unittest.main()
