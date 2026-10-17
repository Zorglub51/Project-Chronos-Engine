"""Bounded catalogue/worker tests using disposable metadata, never ROMs or a console."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

HOOK = Path(__file__).resolve().parents[1]
PROBE = r'''
#define _GNU_SOURCE
#include <assert.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
static long fail_allocation=-1, allocation_count;
static int allocation_failed;
static int fail_next_allocation(void){if(allocation_count++==fail_allocation){allocation_failed=1;errno=ENOMEM;return 1;}return 0;}
static void*checked_malloc(size_t n){return fail_next_allocation()?NULL:malloc(n);}
static void*checked_calloc(size_t n,size_t size){return fail_next_allocation()?NULL:calloc(n,size);}
static void*checked_realloc(void*p,size_t n){return fail_next_allocation()?NULL:realloc(p,n);}
#define malloc checked_malloc
#define calloc checked_calloc
#define realloc checked_realloc
#include "catalog_library.h"
#undef malloc
#undef calloc
#undef realloc
#include "sq_string_arg.h"
typedef void *HSQUIRRELVM;
typedef int32_t SQInteger;
typedef int32_t SQRESULT;
typedef int32_t SQBool;
struct fake_vm {const char *args[4]; char text[256]; int top, stack[256], calls, fail;
               int expert_gain, expert_false, expert_offset, expert_hblank;
               pthread_t owner;};
static void thread_check(struct fake_vm *v){assert(pthread_equal(v->owner,pthread_self()));++v->calls;}
static void push(HSQUIRRELVM vm,int kind){struct fake_vm *v=vm;thread_check(v);assert(v->top<256);v->stack[v->top++]=kind;}
static void string(HSQUIRRELVM vm,const char *s,SQInteger len){(void)len;struct fake_vm *v=vm;snprintf(v->text,sizeof(v->text),"%s",s);push(v,CJ_STRING);}
static SQRESULT newslot(HSQUIRRELVM vm,SQInteger idx,SQBool stat){(void)stat;struct fake_vm *v=vm;thread_check(v);
 int i=idx<0?v->top+idx:idx-1;assert(i>=0&&v->stack[i]==CJ_OBJECT);assert(v->top>=3);
 assert(v->stack[v->top-2]==CJ_STRING||v->stack[v->top-2]==CJ_INT);
 if(v->fail){v->fail=0;return -1;}v->top-=2;return 0;}
static void table(HSQUIRRELVM v){push(v,CJ_OBJECT);}
static void integer(HSQUIRRELVM v,SQInteger n){struct fake_vm *vm=v;if(n==-8)++vm->expert_offset;if(n==100)++vm->expert_hblank;push(v,CJ_INT);}
static void floating(HSQUIRRELVM v,float n){struct fake_vm *vm=v;if(n>6.93999f&&n<6.94001f)++vm->expert_gain;push(v,CJ_FLOAT);}
static void boolean(HSQUIRRELVM v,SQBool n){struct fake_vm *vm=v;if(!n)++vm->expert_false;push(v,CJ_BOOL);}
static void null_value(HSQUIRRELVM v){push(v,CJ_NULL);}
static SQInteger gettop(HSQUIRRELVM vm){struct fake_vm*v=vm;thread_check(v);return v->top;}
static void settop(HSQUIRRELVM vm,SQInteger n){struct fake_vm*v=vm;thread_check(v);v->top=n;}
static const char *vm_get_string(HSQUIRRELVM vm,SQInteger idx){struct fake_vm*v=vm;return idx>=2&&idx<=5?v->args[idx-2]:NULL;}
static void (*p_sq_pushstring)(HSQUIRRELVM,const char *,SQInteger)=string;
static SQRESULT (*p_sq_newslot)(HSQUIRRELVM,SQInteger,SQBool)=newslot;
static const char *test_root;
static int g_catalog_boot_mode=-2;
#define SAVE_LIBRARY_ROOT test_root
#include "catalog_runtime.h"
static void reset_vm(struct fake_vm*v){v->top=1;v->stack[0]=CJ_OBJECT;v->text[0]=0;}
static void configure_vm(struct fake_vm*v){memset(v,0,sizeof(*v));v->owner=pthread_self();reset_vm(v);
 c_sq_newtable=table;c_sq_integer=integer;c_sq_float=floating;c_sq_bool=boolean;c_sq_null=null_value;
 c_sq_gettop=gettop;c_sq_settop=settop;g_catalog_supported=1;}
static void ready(void){for(unsigned i=0;i<10000&&g_catalog_state==1;++i){catalog_poll_worker();usleep(100);}assert(g_catalog_state!=1);}
static int descriptors(void){int count=0;for(int fd=0;fd<256;++fd)if(fcntl(fd,F_GETFD)>=0)++count;return count;}
int main(int argc,char **argv){if(argc<3)return 2;test_root=argv[2];
 if(!strcmp(argv[1],"oom")){for(unsigned i=0;i<100;++i){fail_allocation=i;allocation_count=0;allocation_failed=0;
  struct catalog_snapshot*s=catalog_read_folder(test_root,"jp","FOLDER_TEST");
  if(s){if(allocation_failed)assert(s->error[0]);else assert(!s->error[0]);catalog_snapshot_free(s);}}
  puts("ok");return 0;}
 if(!strcmp(argv[1],"args")){int32_t slot;
  assert(chronos_sq_argument_slot(5,8,2,&slot)&&slot==6);
  assert(chronos_sq_argument_slot(5,8,3,&slot)&&slot==7);
  assert(!chronos_sq_argument_slot(5,8,4,&slot));
  assert(!chronos_sq_argument_slot(5,8,0,&slot));
  assert(!chronos_sq_argument_slot(5,8,-1,&slot));
  assert(!chronos_sq_argument_slot(-1,8,1,&slot));
  assert(!chronos_sq_argument_slot(8,5,1,&slot));
  assert(!chronos_sq_argument_slot(INT32_MAX,INT32_MAX,INT32_MAX,&slot));
  assert(chronos_sq_argument_slot(INT32_MAX-2,INT32_MAX,2,&slot)&&slot==INT32_MAX-1);
  puts("ok");return 0;}
 if(!strcmp(argv[1],"enabled")){printf("%d\n",catalog_enabled(test_root));return 0;}
 if(!strcmp(argv[1],"current")){struct fake_vm vm;configure_vm(&vm);native_catalog_current(&vm);puts(vm.text);return 0;}
 if(!strcmp(argv[1],"parse")){struct catalog_json dom={0};uint32_t node=catalog_json_parse(&dom,argv[2],strlen(argv[2]));
  printf("%u %u %u\n",node,dom.node_count,dom.string_size);catalog_json_free(&dom);return 0;}
 if(!strcmp(argv[1],"read")){if(argc!=5)return 2;struct catalog_snapshot*s=catalog_read_folder(test_root,argv[3],argv[4]);
  if(!s)return 3;
  if(s->error[0])printf("error:%s\n",s->error);else {printf("ok %u %u %u %u\n",s->count,s->playable_count,s->json.node_count,s->json.string_size);
   for(unsigned i=0;i<s->count;++i){struct catalog_entry*e=&s->entries[i];printf("entry %u %u %s %s %s\n",i,e->role,catalog_json_string(&s->json,e->name),
    catalog_json_string(&s->json,e->game_id),e->rom_path?catalog_json_string(&s->json,e->rom_path):"");}
   if(s->count&&s->entries[s->count-1].game){uint32_t game=s->entries[s->count-1].game,genre=catalog_json_member(&s->json,game,"genre");
    uint32_t unknown=catalog_json_member(&s->json,game,"unknown"),name=catalog_json_member(&s->json,catalog_json_member(&s->json,game,"display"),"name");
    printf("genre %d unknown %d title %s\n",genre?s->json.nodes[genre].value.integer:-1,unknown!=0,catalog_json_string(&s->json,name));}}
  catalog_snapshot_free(s);return 0;}
 if(!strcmp(argv[1],"expert")){struct catalog_snapshot*s=catalog_read_folder(test_root,"jp","_root");
  assert(s&&!s->error[0]&&s->count==1);uint32_t game=s->entries[0].game;
  uint32_t rom=catalog_json_member(&s->json,game,"rom");
  uint32_t option=catalog_json_member(&s->json,rom,"tg16_option"),analog=catalog_json_member(&s->json,rom,"use_analog");
  uint32_t gain=catalog_json_member(&s->json,rom,"preamp"),split=catalog_json_member(&s->json,rom,"view_split");
  assert(s->json.nodes[option].type==CJ_INT&&s->json.nodes[option].value.integer==5);
  assert(s->json.nodes[analog].type==CJ_BOOL&&!s->json.nodes[analog].value.integer);
  assert(s->json.nodes[gain].type==CJ_FLOAT&&s->json.nodes[gain].value.number>6.93999f);
  assert(s->json.nodes[split].type==CJ_ARRAY&&s->json.nodes[split].count==2);
  struct fake_vm vm;configure_vm(&vm);assert(!catalog_emit_snapshot(&vm,s));
  assert(vm.top==2&&vm.expert_gain==1&&vm.expert_false>=3&&vm.expert_offset==1&&vm.expert_hblank==1);
  catalog_snapshot_free(s);puts("ok expert typed snapshot");return 0;}
 if(!strcmp(argv[1],"protocol")){struct fake_vm vm;configure_vm(&vm);vm.args[0]="jp";vm.args[1]="_root";
  native_catalog_begin(&vm);char first[16];snprintf(first,sizeof(first),"%.15s",vm.text);assert(first[0]>='1'&&first[0]<='9');
  vm.args[0]=first;ready();assert(g_catalog_state==2);reset_vm(&vm);native_catalog_poll(&vm);assert(!strcmp(vm.text,"ready"));
  reset_vm(&vm);vm.fail=1;native_catalog_take(&vm);assert(!strncmp(vm.text,"error:",6));assert(g_catalog_state==2&&g_catalog_stage);
  reset_vm(&vm);native_catalog_take(&vm);assert(vm.top==2&&vm.stack[1]==CJ_OBJECT);assert(!g_catalog_stage&&g_catalog_state==4);
  reset_vm(&vm);native_catalog_remember(&vm);assert(!strcmp(vm.text,"ok"));
  reset_vm(&vm);native_catalog_current(&vm);assert(!strcmp(vm.text,"jp/_root"));
  reset_vm(&vm);vm.args[0]="us";vm.args[1]="_root";native_catalog_begin(&vm);char second[16];snprintf(second,sizeof(second),"%.15s",vm.text);
  assert(strcmp(first,second));vm.args[0]=first;reset_vm(&vm);native_catalog_take(&vm);assert(!strcmp(vm.text,"stale"));
  vm.args[0]=second;ready();assert(g_catalog_state==3);reset_vm(&vm);native_catalog_poll(&vm);assert(!strncmp(vm.text,"error:",6));
  assert(!g_catalog_stage);puts("ok");return 0;}
 if(!strcmp(argv[1],"stale")){struct fake_vm vm;configure_vm(&vm);vm.args[0]="jp";vm.args[1]="_root";
  native_catalog_begin(&vm);char ticket[16];snprintf(ticket,sizeof(ticket),"%.15s",vm.text);ready();assert(g_catalog_state==2);
  char moved[1024];snprintf(moved,sizeof(moved),"%s.moved",test_root);assert(!rename(test_root,moved));assert(!mkdir(test_root,0755));
  vm.args[0]=ticket;reset_vm(&vm);native_catalog_take(&vm);assert(!strncmp(vm.text,"error:",6)&&!g_catalog_stage);
  assert(!rmdir(test_root));assert(!rename(moved,test_root));puts("ok");return 0;}
 if(!strcmp(argv[1],"repeat")){int before=descriptors();for(unsigned i=0;i<300;++i){struct catalog_snapshot*s=catalog_read_folder(test_root,"jp","_root");
  assert(s&&!s->error[0]);catalog_snapshot_free(s);}
  struct fake_vm vm;configure_vm(&vm);
  for(unsigned i=0;i<100;++i){reset_vm(&vm);vm.args[0]="jp";vm.args[1]="_root";native_catalog_begin(&vm);
   char ticket[16];snprintf(ticket,sizeof(ticket),"%.15s",vm.text);ready();assert(g_catalog_state==2);vm.args[0]=ticket;
   reset_vm(&vm);native_catalog_take(&vm);assert(vm.top==2&&!g_catalog_stage&&!g_catalog_worker.active);}
  assert(descriptors()==before);puts("ok");return 0;}
 return 2;}
'''


class CatalogueRuntimeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.build = tempfile.TemporaryDirectory()
        directory = Path(cls.build.name)
        source = directory / 'probe.c'
        source.write_text(PROBE)
        cls.exe = directory / 'probe'
        flags = ['-fsanitize=address,undefined', '-fno-omit-frame-pointer'] if os.environ.get('CATALOG_SANITIZE') else []
        subprocess.run([os.environ.get('CC', 'cc'), '-std=c11', '-D_POSIX_C_SOURCE=200809L',
                        '-D_DARWIN_C_SOURCE', '-Wall', '-Wextra', '-Werror',
                        '-Wno-unused-function', '-Wno-unused-variable', '-O2', '-pthread',
                        *flags, '-I' + str(HOOK), str(source), '-o', str(cls.exe)], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.build.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name) / 'library'
        self.root.mkdir()
        (self.root / '.chronos-library-v2').write_text('2\n')
        (self.root / 'published/roms').mkdir(parents=True)
        self.write('settings_games.json', {'genres': ['RPG', 'Action'], 'alpha_exclusions': ['The '],
                                          'unknown_settings': {'retain': True}})
        self.write('jp/gamelist.json', [])
        self.write('us/gamelist.json', [])
        self.assets('runtime/back')

    def tearDown(self):
        self.temp.cleanup()

    def write(self, relative, data):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(data, ensure_ascii=False))

    def game(self, relative='jp/GAME000', identity='a' * 32, tag='GAME000'):
        data = {'game_id': identity, 'native_region_tag': tag,
                'display': {'name': '大魔界村 🎮', 'name_eng': 'Test', 'titlebar': 7},
                'rom': {'arch': 'tg16', 'rom': 'test.pce', 'preamp': 0.64},
                'genre': 2, 'unknown': {'list': [True, None, 1.2]}}
        self.write(relative + '/game.json', data)
        (self.root / 'published/roms/test.pce.m').write_bytes(b'fixture')
        self.assets(relative)
        return data

    def assets(self, relative):
        self.write(relative + '/runtime/assets.json', {'schema': 1, 'revision': '1' * 32,
                                                      'cover_path': 'runtime/cover.image.psb',
                                                      'thumb_path': 'runtime/thumb.image.psb',
                                                      'left': None, 'right': None})
        for name in ['cover', 'thumb']:
            (self.root / relative / ('runtime/' + name + '.image.psb.m')).write_bytes(b'fixture')

    def run_probe(self, mode, *args):
        return subprocess.run([str(self.exe), mode, str(self.root), *args],
                              check=True, capture_output=True, text=True).stdout.strip()

    def read(self, lineup='jp', folder='_root'):
        return self.run_probe('read', lineup, folder)

    def test_empty_root_and_empty_folder_are_valid_distinct_snapshots(self):
        self.assertTrue(self.read().startswith('ok 0 0 '))
        self.write('jp/FOLDER_EMPTY/gamelist.json', [])
        self.write('jp/FOLDER_EMPTY/folder.json', {'name': 'Empty'})
        result = self.read(folder='FOLDER_EMPTY')
        self.assertTrue(result.startswith('ok 1 0 '))
        self.assertIn('entry 0 2 BACK', result)

    def test_raw_metadata_and_authoritative_order_survive(self):
        self.game('jp/Game with spaces')
        self.write('jp/gamelist.json', [{'folder': 'Game with spaces', 'sor_date': 9, 'sor_genr': 3}])
        result = self.read()
        self.assertTrue(result.startswith('ok 1 1 '))
        self.assertIn('roms/test.pce', result)
        self.assertIn('genre 2 unknown 1 title 大魔界村 🎮', result)

    def test_full_expert_fixture_reaches_the_typed_vm_snapshot(self):
        self.game('jp/GAME700')
        fixture = HOOK.parents[1] / 'editor/tests/fixtures/expert-game.json'
        self.write('jp/GAME700/game.json', json.loads(fixture.read_text()))
        (self.root / 'published/roms/fixture.pce.m').write_bytes(b'fixture')
        self.write('jp/gamelist.json', [{'folder': 'GAME700'}])
        self.assertEqual(self.run_probe('expert'), 'ok expert typed snapshot')

    def test_folder_cards_and_back_have_no_save_identity(self):
        self.game('jp/FOLDER_TEST/GAME000')
        self.write('jp/FOLDER_TEST/folder.json', {'name': 'Folder'})
        self.write('jp/FOLDER_TEST/gamelist.json', [{'folder': 'GAME000'}])
        self.assets('jp/FOLDER_TEST')
        self.write('jp/gamelist.json', [{'folder': 'FOLDER_TEST'}])
        self.assertTrue(self.read().startswith('ok 1 0 '))
        self.assertIn('entry 0 1 FOLDER_TEST', self.read())
        self.assertTrue(self.read(folder='FOLDER_TEST').startswith('ok 2 1 '))

    def test_invalid_marker_does_not_silently_enable_legacy(self):
        self.assertEqual(self.run_probe('enabled'), '1')
        (self.root / '.chronos-library-v2').unlink()
        self.assertEqual(self.run_probe('enabled'), '0')
        (self.root / '.chronos-library-v2').write_text('3\n')
        self.assertEqual(self.run_probe('enabled'), '-1')

    def test_missing_vm_arguments_and_integer_overflow_are_rejected(self):
        self.assertEqual(self.run_probe('args'), 'ok')

    def test_allocation_failure_always_returns_error_and_keeps_back_safe(self):
        self.game('jp/FOLDER_TEST/GAME000')
        self.write('jp/FOLDER_TEST/gamelist.json', [{'folder': 'GAME000'}])
        self.write('jp/FOLDER_TEST/folder.json', {'name': 'Test folder'})
        self.assertEqual(self.run_probe('oom'), 'ok')

    def test_remembered_folder_rejects_embedded_nul_and_extra_path_components(self):
        path = self.root / '.current-v2'
        path.write_bytes(b'jp/_root\0garbage\n')
        self.assertTrue(self.run_probe('current').startswith('error:'))
        path.write_bytes(b'jp/FOLDER_TEST/../_root\n')
        self.assertTrue(self.run_probe('current').startswith('error:'))
        path.write_bytes(b'jp/_root\n')
        self.assertEqual(self.run_probe('current'), 'jp/_root')

    def test_missing_game_and_malformed_json_are_errors(self):
        self.write('jp/gamelist.json', [{'folder': 'MISSING'}])
        self.assertTrue(self.read().startswith('error:'))
        (self.root / 'jp/gamelist.json').write_text('[{},]')
        self.assertIn('Invalid JSON', self.read())

    def test_symlinks_and_path_traversal_are_rejected(self):
        self.game()
        self.write('jp/gamelist.json', [{'folder': '../GAME000'}])
        self.assertTrue(self.read().startswith('error:'))
        self.write('jp/gamelist.json', [{'folder': 'ALIAS'}])
        (self.root / 'jp/ALIAS').symlink_to('GAME000')
        self.assertTrue(self.read().startswith('error:'))
        self.write('jp/gamelist.json', [{'folder': 'GAME000'}])
        rom = self.root / 'published/roms/test.pce.m'
        rom.unlink()
        rom.symlink_to('/etc/passwd')
        self.assertIn('Shared ROM missing', self.read())

    def test_stable_identity_and_native_tag_collisions_fail(self):
        self.game('jp/A')
        self.game('jp/B')
        self.write('jp/gamelist.json', [{'folder': 'A'}, {'folder': 'B'}])
        self.assertIn('Duplicate game identity', self.read())

    def test_missing_identity_requires_explicit_migration(self):
        data = self.game()
        del data['game_id']
        self.write('jp/GAME000/game.json', data)
        self.write('jp/gamelist.json', [{'folder': 'GAME000'}])
        self.assertIn('identity migration', self.read())

    def test_nested_folder_and_51_cards_are_rejected(self):
        self.write('jp/FOLDER_OUTER/gamelist.json', [{'folder': 'FOLDER_INNER'}])
        (self.root / 'jp/FOLDER_OUTER/FOLDER_INNER').mkdir()
        self.assertIn('Nested folders', self.read(folder='FOLDER_OUTER'))
        self.write('jp/gamelist.json', [{'folder': 'G' + str(i)} for i in range(51)])
        self.assertIn('50 entries', self.read())

    def test_metadata_file_and_dom_memory_are_bounded(self):
        (self.root / 'jp/gamelist.json').write_text(' ' * (32 * 1024 + 1))
        self.assertTrue(self.read().startswith('error:'))
        valid = '[1]' * 1  # Numeric JSON paths below use the parser directly.
        proc = subprocess.run([str(self.exe), 'parse', valid], check=True, capture_output=True, text=True)
        self.assertNotEqual(proc.stdout.split()[0], '0')
        for text in ['[1,]', '{"a":1,"a":2}', '{"__chronos_array":0}', '01', '1e9999',
                     '2147483648', '"\\u0000"', '"\\ud800"', '[' * 18 + '0' + ']' * 18]:
            proc = subprocess.run([str(self.exe), 'parse', text], check=True, capture_output=True, text=True)
            self.assertEqual(proc.stdout.split()[0], '0', text)

    def test_async_ticket_commit_rollback_and_vm_thread_affinity(self):
        self.game()
        self.write('jp/gamelist.json', [{'folder': 'GAME000'}])
        (self.root / 'us/gamelist.json').write_text('{broken')
        self.assertEqual(self.run_probe('protocol'), 'ok')
        self.assertEqual((self.root / '.current-v2').read_text(), 'jp/_root\n')

    def test_usb_replacement_after_ready_rejects_stale_snapshot(self):
        self.game()
        self.write('jp/gamelist.json', [{'folder': 'GAME000'}])
        self.assertEqual(self.run_probe('stale'), 'ok')

    def test_small_and_maximum_catalogues_do_not_invent_entries(self):
        for i in range(50):
            self.game('jp/G' + str(i), identity=f'{i+1:032x}', tag='G' + str(i))
        for count in [0, 1, 2, 3, 4, 5, 50]:
            self.write('jp/gamelist.json', [{'folder': 'G' + str(i)} for i in range(count)])
            self.assertTrue(self.read().startswith(f'ok {count} {count} '))

    def test_repeated_folder_reads_release_allocations_and_descriptors(self):
        self.game()
        self.write('jp/gamelist.json', [{'folder': 'GAME000'}])
        self.assertEqual(self.run_probe('repeat'), 'ok')


if __name__ == '__main__':
    unittest.main()
