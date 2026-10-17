#!/usr/bin/env python3
"""Exercise Expert JSON through the shipped SQ2 views and emulator option readers."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def squirrel(value):
    # Match the hook's public VM ABI: arrays arrive as integer-keyed tables.
    if isinstance(value, dict):
        return '{' + ','.join('[' + json.dumps(key) + ']=' + squirrel(child)
                              for key, child in value.items()) + '}'
    if isinstance(value, list):
        return '{__chronos_array=' + str(len(value)) + ',' + ','.join(
            '[' + str(i) + ']=' + squirrel(child) for i, child in enumerate(value)) + '}'
    return json.dumps(value, ensure_ascii=False)


CHECK = r'''
printf <- function(a, b = null, c = null, d = null, e = null, f = null) {};
function check(value, message) { if (!value) throw message; }
function beginReadFolder(lineup, folder) { return "1"; }
function pollReadFolder(ticket) { return "ready"; }
function takeFolder(ticket) { return raw_snapshot; }
local catalogue = ChronosLibrary();
catalogue.base_profile = { title = { dev_name = "PceMini" }, player_num = 6,
    controller = {}, m2epi = { overdrive = 0, preamp = 0.0, version = {} }, game_versions = {} };
foreach (lineup in ["jp", "us"]) {
    raw_snapshot.lineup = lineup;
    catalogue.read(lineup, "_root");
    local offset = lineup == "jp" ? 0 : 50;
    foreach (slot in [offset, offset + 100]) {
        local item = catalogue.top.items[slot];
        check(item.ScreenMode == 4 && item.EmuOfsX == -8 && item.EmuOfsY == 2, "framing lost");
        check(item.copy == "Test copyright" && item.desc == "Fixture description", "menu metadata lost");
        check(item.dev_name == "PceMini" && item.image == 0, "managed menu fields overwritten");
    }
    foreach (tag in ["GAME700", "GAME700E"]) {
        local rom = catalogue.profile.m2epi.version[tag];
        check(typeof rom.preamp == "float" && rom.preamp > 6.93999 && rom.preamp < 6.94001, "float gain");
        check(rom.overdrive == 2 && rom.brightness > 0.84 && rom.brightness < 0.86, "display options");
        check(rom.use_analog == false && rom.sram_support == false, "explicit false lost");
        check(typeof rom.view_split == "array" && rom.view_split[0] == 2, "array ABI");
        check(rom.rapid_speed_list[4] == 30 && rom.attr.compat == 17, "complex options");
        check(rom.string.compat == "test" && rom.stringGlobal.unused == "kept", "string attributes");
        check(rom.future_option.values[0] == 0 && rom.future_option.values[1] == false &&
              rom.future_option.values[2] == 1.25, "unknown typed options");
        check(rom.rom == "roms/fixture.pce" && rom.chronos_save_path == "jp/GAME700", "hook routing");
        local packed = _init_emulator_get_arch_sub_info(catalogue.profile.m2epi, tag);
        check(packed == ((7 << 6) | (100 << 9) | (5 << 17) | 1 | 8), "native option word");
        check(_init_emulator_get_arch_extra_info(catalogue.profile.m2epi, tag) == 17, "extra info");
        check(_init_emulator_get_option_by_regionTag(catalogue.profile.m2epi, tag, "overdrive", 0) == 2,
              "native task overdrive option");
    }
}
local clean = chronos_profile_options({overdrive=null, use_analog=false, tg16_option=0,
    regionTag="old", systemdata_sram_data_no=99, chronos_game_id="foreign"});
check(!("overdrive" in clean) && !("regionTag" in clean) && !("systemdata_sram_data_no" in clean), "inheritance/managed fields");
check(clean.use_analog == false && clean.tg16_option == 0, "zero and false retained");
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--checker', type=Path, required=True)
    args = parser.parse_args()
    game = json.loads((ROOT.parent / 'editor/tests/fixtures/expert-game.json').read_text())
    entry = dict(role='game', folder='GAME700', game_id=game['game_id'],
                 native_region_tag='GAME700', save_path='jp/GAME700',
                 rom_path='roms/fixture.pce', game=game, sort={})
    snapshot = dict(schema=2, lineup='jp', folder='_root', path='test',
                    entries=[entry], settings={}, entry_count=1, playable_count=1)
    engine = (ROOT / 'scripts-src/emulator.nut').read_text()
    functions = []
    for name in ['_init_emulator_get_option_by_regionTag', '_init_emulator_get_arch_sub_info',
                 '_init_emulator_get_arch_extra_info']:
        start = engine.index('function ' + name + '(')
        end = re.search(r'^}', engine[start:], re.M).end() + start
        functions.append(engine[start:end])
    constants = 'M2EpiArchSubInfo <- {MD_6B_DISABLE=1,USE_ANALOG=4,DIRECTION_4=8,TG16CD_SUPER_CARD=16,TG16CD_ARCADE_CARD=32,TG16PAD_TYPE_SHIFT=6,TG16_HBLANK_SHIFT=9,TG16_OPTION_SHIFT=17};\n'
    script = (ROOT / 'scripts-src/chronos_library.nut').read_text() + '\n' + constants + '\n'.join(functions)
    script += '\nraw_snapshot <- ' + squirrel(snapshot) + ';\n' + CHECK
    with tempfile.TemporaryDirectory(prefix='chronos-expert-') as temporary:
        path = Path(temporary) / 'expert.nut'
        path.write_text(script)
        subprocess.run([str(args.checker.resolve()), '-r', str(path)], check=True)
    print('PASS: Expert types, framing, aliases, inheritance and native emulator option word')


if __name__ == '__main__':
    main()
