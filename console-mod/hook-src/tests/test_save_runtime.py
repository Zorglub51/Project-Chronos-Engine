"""Portable stable-save routing and SRAM ownership tests. No ROMs or hardware."""
import hashlib
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest

HOOK = Path(__file__).resolve().parents[1]
A, B = "a" * 32, "b" * 32
OFFSET, SRAM_BYTES = 0x5E84, 8448

PROBE = r'''
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <stdlib.h>
#include <unistd.h>
static int test_fsync(int fd) {
    if(getenv("FAIL_SYNC")){errno=EIO;return -1;}return fsync(fd);
}
#define fsync test_fsync
#include "save_owner.h"
static const char *A="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
static const char *B="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
static void routes_init(struct save_routes *r, const char *root) {
    memset(r,0,sizeof(*r));r->enabled=1;
    snprintf(r->library,sizeof(r->library),"%s",root);
    snprintf(r->live_alias,sizeof(r->live_alias),"/rootfs_data/save");
    assert(!save_route_bind(r,"1",A,"jp/A","g000a","012"));
    assert(!save_route_bind(r,"51",B,"us/B","g000b","011"));
}
static void routing(void) {
    struct save_routes r;routes_init(&r,"/library");char out[768],name[120];
    for(unsigned slot=0;slot<4;++slot) {
        snprintf(name,sizeof(name),"/usr/game//save/data_012_%04u.bin",4+slot);
        assert(save_route_path(&r,name,NULL,out,sizeof(out))==1);
        snprintf(name,sizeof(name),"/library/jp/A/saves/state_%u.bin",slot);assert(!strcmp(out,name));
        snprintf(name,sizeof(name),"./save//meta_011_%04u.bin",204+slot);
        assert(save_route_path(&r,name,"/usr/game",out,sizeof(out))==1);
        snprintf(name,sizeof(name),"/library/us/B/saves/state_%u_meta.bin",slot);assert(!strcmp(out,name));
    }
    assert(save_route_path(&r,"data_012_0004.bin","/rootfs_data/save",out,sizeof(out))==1);
    assert(save_route_path(&r,"/usr/game/save/data_012_0004.bin.XXXXXX",NULL,out,sizeof(out))==1);
    assert(!strcmp(out,"/library/jp/A/saves/state_0.bin.XXXXXX"));
    assert(save_route_path(&r,"save/meta_012_0007.tmp","/usr/game",out,sizeof(out))==1);
    assert(!strcmp(out,"/library/jp/A/saves/state_3_meta.bin.tmp"));
    assert(save_route_path(&r,"/usr/game/save/data_012_0000.bin",NULL,out,sizeof(out))==-1);
    assert(save_route_path(&r,"/usr/game/save/data_011_0004.bin",NULL,out,sizeof(out))==-1);
    assert(save_route_path(&r,"/usr/game/save/data_012_0500.bin",NULL,out,sizeof(out))==-1);
    assert(save_route_path(&r,"/usr/game/save/../save/data_012_0004.bin",NULL,out,sizeof(out))==-1);
    assert(save_route_path(&r,"/usr/game/save/save/data_012_0004.bin",NULL,out,sizeof(out))==-1);
    assert(save_route_path(&r,"/usr/game/save/data_012_0004.bin../x",NULL,out,sizeof(out))==-1);
    assert(save_route_path(&r,"/other/save/data_012_0004.bin",NULL,out,sizeof(out))==0);
    assert(save_route_path(&r,"/usr/game/save/data_008_0000.bin",NULL,out,sizeof(out))==0);
    assert(save_route_bind(&r,"125",A,"jp/A","g000a","012")==-1);
    assert(save_route_bind(&r,"2",B,"jp/A","g000b","011")==-1);
    assert(save_route_bind(&r,"2",A,"jp/C","g000a","012")==-1);
    assert(save_route_bind(&r,"2","../bad","jp/C","g000a","012")==-1);
    assert(save_route_bind(&r,"2",A,"jp/../C","g000a","012")==-1);
    assert(save_route_bind(&r,"2",A,"jp/C","fifteenlettersxx","012")==-1);
    r.enabled=0;assert(save_route_path(&r,"/usr/game/save/data_012_0000.bin",NULL,out,sizeof(out))==0);
}
int main(int argc,char **argv) {
    if(argc==2&&!strcmp(argv[1],"routing")){routing();puts("ok");return 0;}
    if(argc!=9)return 2;
    struct save_routes routes;routes_init(&routes,argv[2]);
    const char *op=argv[1],*id=argv[6];int result=-1;
    struct save_owner owner;
    if(save_owner_load(&owner,argv[5])){printf("-1 %d\n",errno);return 0;}
    if(!strcmp(op,"flush-stale")){
        struct save_owner other={.known=1};snprintf(other.game_id,sizeof(other.game_id),"%s",B);
        snprintf(other.directory,sizeof(other.directory),"us/B");
        assert(!save_owner_marker(&other,argv[5],"ready"));
        result=save_owner_flush(&owner,&routes,argv[3],argv[4],argv[5],argv[8]);
        printf("%d %d\n",result,errno);return 0;
    }
    if(!strcmp(op,"activate"))result=save_owner_activate(&owner,&routes,id,argv[3],argv[4],argv[5],argv[8]);
    else if(!strcmp(op,"flush"))result=save_owner_flush(&owner,&routes,argv[3],argv[4],argv[5],argv[8]);
    else if(!strcmp(op,"refresh"))result=save_digest_refresh(argv[3],argv[4]);
    else {
        const struct save_binding *b=save_route_find(&routes,id);
        int dir=b?save_files_game_directory(routes.library,b->directory,1):-1;
        if(dir>=0) {
            if(!strcmp(op,"identity"))result=save_files_identity(dir,id,1);
            else if(!strcmp(op,"locks-get")){unsigned mask;result=save_files_get_locks(dir,&mask);if(!result){printf("0 %u\n",mask);close(dir);return 0;}}
            else if(!strcmp(op,"locks-set")){unsigned mask;if(!save_route_decimal(argv[7],15,&mask))result=save_files_set_locks(dir,mask);}
            int error=errno;close(dir);errno=error;
        }
    }
    printf("%d %d\n",result,errno);return 0;
}
'''


def metadata():
    body = bytearray(96)
    struct.pack_into("<4sHH", body, 0, b"PSB\0", 3, 0)
    struct.pack_into("<III", body, 24, 44, 56, 80)
    body[44:48] = bytes([13, 1, 13, 0])
    body[56:60] = bytes([13, 1, 13, 16])
    return body


class StableSaveRuntimeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.compiler_temp = tempfile.TemporaryDirectory()
        root = Path(cls.compiler_temp.name)
        (root / "probe.c").write_text(PROBE)
        cls.exe = root / "probe"
        subprocess.run([os.environ.get("CC", "cc"), "-std=c11", "-D_POSIX_C_SOURCE=200809L",
                        "-D_DARWIN_C_SOURCE", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function",
                        "-O2", "-I" + str(HOOK), str(root / "probe.c"), "-o", str(cls.exe)], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.compiler_temp.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.base = Path(self.temp.name)
        self.library = self.base / "library"
        self.game_a, self.game_b = self.library / "jp/A", self.library / "us/B"
        for game, game_id in [(self.game_a, A), (self.game_b, B)]:
            (game / "saves").mkdir(parents=True)
            (game / "saves/.chronos-game-id").write_text(game_id + "\n")
        self.live, self.meta = self.base / "data_008.bin", self.base / "meta_008.bin"
        self.marker = self.base / ".chronos-sram-owner-v2"
        (self.base / "nand").mkdir()
        self.recovery = self.base / "nand/.chronos-sram-recovery"
        self.live.write_bytes(b"P" * OFFSET + b"L" * SRAM_BYTES + b"Z" * (149 * SRAM_BYTES))
        self.meta.write_bytes(metadata())
        self.assertEqual(self.run_op("refresh")[0], 0)

    def tearDown(self):
        self.temp.cleanup()

    def run_op(self, op, game_id=A, extra="", fail_sync=False):
        env = os.environ.copy()
        if fail_sync:
            env["FAIL_SYNC"] = "1"
        output = subprocess.check_output([str(self.exe), op, str(self.library), str(self.live),
                                          str(self.meta), str(self.marker), game_id, extra, str(self.recovery)],
                                         text=True, env=env, timeout=5)
        return tuple(map(int, output.split()))

    def canonical(self, game, body):
        (game / "sram.bin").write_bytes(body)
        (game / "saves/.chronos-sram-schema").write_text("2\n")

    def owner(self, game_id=A, directory="jp/A", state="ready"):
        self.marker.write_text(f"2\n{state}\n{game_id}\n{directory}\n")

    def slot(self):
        return self.live.read_bytes()[OFFSET:OFFSET + SRAM_BYTES]

    def assert_digest(self):
        self.assertEqual(self.meta.read_bytes()[80:], hashlib.md5(self.live.read_bytes()).digest())

    def test_all_slots_ids_aliases_temporaries_and_registry_validation(self):
        subprocess.run([str(self.exe), "routing"], check=True)

    def test_boot_preserves_newer_correctly_owned_live_slot(self):
        self.canonical(self.game_a, b"O" * SRAM_BYTES)
        self.owner()
        before = self.live.read_bytes()
        self.assertEqual(self.run_op("activate")[0], 0)
        self.assertEqual(self.live.read_bytes(), before)
        self.assertEqual((self.game_a / "sram.bin").read_bytes(), b"L" * SRAM_BYTES)
        self.assert_digest()

    def test_transition_flushes_only_current_slot_and_preserves_other_banks(self):
        self.canonical(self.game_a, b"O" * SRAM_BYTES)
        self.owner()
        before = self.live.read_bytes()
        self.assertEqual(self.run_op("activate", B)[0], 0)
        after = self.live.read_bytes()
        self.assertEqual(after[:OFFSET], before[:OFFSET])
        self.assertEqual(after[OFFSET + SRAM_BYTES:], before[OFFSET + SRAM_BYTES:])
        self.assertEqual(self.slot(), bytes(SRAM_BYTES))
        self.assertEqual((self.game_b / "sram.bin").read_bytes(), bytes(SRAM_BYTES))
        self.assertEqual((self.game_b / "saves/.chronos-sram-schema").read_text(), "2\n")
        self.assertEqual((self.game_a / "sram.bin").read_bytes(), b"L" * SRAM_BYTES)
        self.assert_digest()

    def test_invalid_native_digest_does_not_overwrite_canonical_or_switch(self):
        self.canonical(self.game_a, b"O" * SRAM_BYTES)
        self.owner()
        with self.live.open("r+b") as stream:
            stream.seek(OFFSET)
            stream.write(b"X" * SRAM_BYTES)
        self.assertLess(self.run_op("flush")[0], 0)
        self.assertLess(self.run_op("activate", B)[0], 0)
        self.assertEqual((self.game_a / "sram.bin").read_bytes(), b"O" * SRAM_BYTES)
        self.assertEqual(self.slot(), b"X" * SRAM_BYTES)
        self.assertEqual(self.run_op("refresh")[0], 0)
        self.assertEqual(self.run_op("flush")[0], 0)
        self.assertEqual((self.game_a / "sram.bin").read_bytes(), b"X" * SRAM_BYTES)

    def test_changed_usb_owner_or_missing_live_bank_is_never_misattributed(self):
        self.canonical(self.game_a, b"O" * SRAM_BYTES)
        self.owner()
        self.assertLess(self.run_op("flush-stale")[0], 0)
        self.assertEqual((self.game_a / "sram.bin").read_bytes(), b"O" * SRAM_BYTES)
        self.assertFalse(self.recovery.exists())
        self.owner()
        self.live.unlink()
        self.assertLess(self.run_op("flush")[0], 0)
        self.assertEqual((self.game_a / "sram.bin").read_bytes(), b"O" * SRAM_BYTES)
        self.assertFalse(self.recovery.exists())

    def test_orphan_recovery_survives_usb_replacement_and_valid_zero_sram(self):
        self.canonical(self.game_a, b"O" * SRAM_BYTES)
        self.owner()
        with self.live.open("r+b") as stream:
            stream.seek(OFFSET)
            stream.write(bytes(SRAM_BYTES))
        self.run_op("refresh")
        saved_a = self.base / "detached-A"
        self.game_a.rename(saved_a)
        self.assertEqual(self.run_op("flush")[0], 1)
        recovery = self.recovery / (A + ".bin")
        self.assertEqual(recovery.read_bytes(), bytes(SRAM_BYTES))
        self.assertEqual(self.run_op("activate", B)[0], 0)
        saved_a.rename(self.game_a)
        self.assertEqual(self.run_op("activate", A)[0], 0)
        self.assertEqual(self.slot(), bytes(SRAM_BYTES))
        self.assertEqual((self.game_a / "sram.bin").read_bytes(), bytes(SRAM_BYTES))
        self.assertFalse(recovery.exists())
        self.assert_digest()

    def test_pending_owner_never_attributes_uncommitted_live_sram(self):
        self.canonical(self.game_a, b"C" * SRAM_BYTES)
        self.owner(state="pending")
        self.assertEqual(self.run_op("activate")[0], 0)
        self.assertEqual(self.slot(), b"C" * SRAM_BYTES)

    def test_empty_uncommitted_recovery_directory_does_not_block_activation(self):
        self.canonical(self.game_a, b"C" * SRAM_BYTES)
        self.recovery.mkdir()
        self.assertEqual(self.run_op("activate")[0], 0)
        self.assertEqual(self.slot(), b"C" * SRAM_BYTES)
        self.assert_digest()

    def test_empty_activation_disowns_dummy_bank_without_losing_previous_game(self):
        self.canonical(self.game_a, b"O" * SRAM_BYTES)
        self.owner()
        self.assertEqual(self.run_op("activate", "")[0], 0)
        self.assertIn(b"\npending\n", self.marker.read_bytes())
        self.assertEqual((self.game_a / "sram.bin").read_bytes(), b"L" * SRAM_BYTES)
        with self.live.open("r+b") as stream:
            stream.seek(OFFSET)
            stream.write(b"D" * SRAM_BYTES)
        self.run_op("refresh")
        self.assertEqual(self.run_op("flush")[0], 0)
        self.assertEqual((self.game_a / "sram.bin").read_bytes(), b"L" * SRAM_BYTES)
        self.assertEqual(self.run_op("activate", "")[0], 0)
        self.assertEqual(self.run_op("activate", A)[0], 0)
        self.assertEqual(self.slot(), b"L" * SRAM_BYTES)
        self.assert_digest()

    def test_unknown_schema_empty_and_truncated_sram_are_rejected(self):
        original = self.live.read_bytes()
        (self.game_a / "sram.bin").write_bytes(b"C" * SRAM_BYTES)
        self.assertLess(self.run_op("activate")[0], 0)
        self.assertEqual(self.live.read_bytes(), original)
        (self.game_a / "saves/.chronos-sram-schema").write_text("2\n")
        for body in [b"", b"C" * 8192, b"C" * (SRAM_BYTES + 1)]:
            (self.game_a / "sram.bin").write_bytes(body)
            self.assertLess(self.run_op("activate")[0], 0)
            self.assertEqual(self.live.read_bytes(), original)
            self.assertEqual((self.game_a / "sram.bin").read_bytes(), body)

    def test_identity_mismatch_or_symlink_never_overwrites_another_game(self):
        self.canonical(self.game_a, b"O" * SRAM_BYTES)
        self.owner()
        (self.game_a / "saves/.chronos-game-id").write_text(B + "\n")
        self.assertEqual(self.run_op("flush")[0], 1)
        self.assertEqual((self.game_a / "sram.bin").read_bytes(), b"O" * SRAM_BYTES)
        self.assertLess(self.run_op("activate")[0], 0)
        (self.game_a / "saves/.chronos-game-id").write_text(A + "\n")
        outside = self.base / "outside"
        outside.write_bytes(b"protected")
        (self.game_a / "sram.bin").unlink()
        (self.game_a / "sram.bin").symlink_to(outside)
        self.assertLess(self.run_op("activate")[0], 0)
        self.assertEqual(outside.read_bytes(), b"protected")

    def test_directory_symlink_escape_is_rejected(self):
        detached = self.base / "outside-game"
        self.game_a.rename(detached)
        self.game_a.symlink_to(detached, target_is_directory=True)
        self.assertLess(self.run_op("identity")[0], 0)
        self.game_a.unlink()
        detached.rename(self.game_a)
        detached_saves = self.base / "outside-saves"
        (self.game_a / "saves").rename(detached_saves)
        (self.game_a / "saves").symlink_to(detached_saves, target_is_directory=True)
        self.assertLess(self.run_op("identity")[0], 0)

    def test_locks_have_missing_zero_and_all_four_slot_bits(self):
        self.assertEqual(self.run_op("locks-get")[0], 1)
        for mask in [0, 1, 2, 4, 8, 15]:
            self.assertEqual(self.run_op("locks-set", extra=str(mask))[0], 0)
            self.assertEqual(self.run_op("locks-get"), (0, mask))
        self.assertEqual((self.game_a / "saves/locks.bin").stat().st_size, 1)
        self.assertLess(self.run_op("locks-set", extra="16")[0], 0)
        (self.game_a / "saves/locks.bin").write_bytes(b"\0\0")
        self.assertLess(self.run_op("locks-get")[0], 0)

    def test_failed_fsync_preserves_existing_locks_and_owner_before_import(self):
        self.assertEqual(self.run_op("locks-set", extra="5")[0], 0)
        self.assertLess(self.run_op("locks-set", extra="10", fail_sync=True)[0], 0)
        self.assertEqual((self.game_a / "saves/locks.bin").read_bytes(), b"\x05")
        self.canonical(self.game_a, b"O" * SRAM_BYTES)
        self.canonical(self.game_b, b"B" * SRAM_BYTES)
        self.owner()
        original_marker, original_live = self.marker.read_bytes(), self.live.read_bytes()
        self.assertLess(self.run_op("activate", B, fail_sync=True)[0], 0)
        self.assertEqual(self.marker.read_bytes(), original_marker)
        self.assertEqual(self.live.read_bytes(), original_live)
        self.assertEqual((self.game_a / "sram.bin").read_bytes(), b"O" * SRAM_BYTES)
        self.assertFalse(list(self.game_a.glob(".*.tmp.*")))
        self.assertFalse(list((self.game_a / "saves").glob(".*.tmp.*")))


if __name__ == "__main__":
    unittest.main()
