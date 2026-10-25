#!/usr/bin/env python3
"""Exercise shipped Squirrel views with the same SQ2.2 VM as the retail engine."""
import argparse
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CHECK = r'''
printf <- function(a, b = null, c = null, d = null, e = null, f = null) {};
function check(value, message) { if (!value) throw message; }
incoming <- null;
failure <- false;
function beginReadFolder(lineup, folder) { return failure ? "error:unavailable" : "1"; }
function pollReadFolder(ticket) { return "ready"; }
function takeFolder(ticket) { return incoming; }
function chronosRememberFolder(ticket) { return "ok"; }
function chronosLibraryEnabled() { return "v2"; }

function entry(index, role = "game") {
    return { folder = role == "game" ? "GAME" + index.tostring() : "FOLDER_TEST",
        role = role, game_id = role == "game" ? "id" + index.tostring() : "",
        native_region_tag = "GAME" + index.tostring(),
        save_path = "jp/GAME" + index.tostring(),
        game = { display = { name = "INTERNAL" + index.tostring(), tname = "JP" + index.tostring(),
                   name_eng = "ENG" + index.tostring(), titlebar = 6, ccolor = 3, csize = 1 },
                 rom = { arch = "sgx", country = "jp", preamp = 0.64, rom = "old.pce", tg16_option = 3,
                   future_option = { enabled = true } },
                 emulator = { ScreenMode = 3, EmuOfsX = -2, EmuOfsY = 1 } },
        rom_path = "roms/test" + index.tostring() + ".pce",
        folder_metadata = { name = "Folder" }, sort = { sor_date = 100 - index,
                   sor_demo = index, sor_genr = index % 3, sor_name = index, sor_pnum = 1 } };
}

local catalogue = ChronosLibrary();
catalogue.base_profile = { title = { dev_name = "PceMini" }, player_num = 6,
    controller = { kept = true }, m2epi = { overdrive = 0, version = {} }, game_versions = {} };
s_chronos_library = catalogue;
foreach (count in [0, 1, 2, 3, 4, 5, 50]) {
    foreach (lineup in ["jp", "us"]) {
        incoming = { schema = 2, lineup = lineup, folder = "_root", path = "test",
            entries = [], settings = {}, entry_count = count, playable_count = count };
        for (local i = 0; i < count; i++) incoming.entries.append(entry(i));
        catalogue.read(lineup, "_root");
        check(catalogue.top.items.len() == 200, "native table shape");
        check(catalogue.top[lineup == "jp" ? "titleNum" : "titleNumTG"] == count, "count");
        check(catalogue.top[lineup == "jp" ? "titleNumTG" : "titleNum"] == 0, "inactive lineup");
        local offset = lineup == "jp" ? 0 : 50;
        for (local i = 0; i < count; i++) {
            local original = catalogue.top.items[offset + i];
            local english = catalogue.top.items[offset + 100 + i];
            check(original.game_id == english.game_id && original.save_path == english.save_path,
                  "translated save identity");
            check(original.tname == "JP" + i.tostring() && english.tname == "ENG" + i.tostring(), "names");
            check(original.name == "INTERNAL" + i.tostring() && english.name == original.name,
                  "internal name is preserved separately from both spine titles");
            check(original.chronos_entry == english.chronos_entry, "single metadata owner");
            check(original.titlebar == 6 && original.ccolor == 3, "US titlebar/packaging retained");
            local rom = catalogue.profile.m2epi.version[original.regionTag];
            check(rom.tg16_option == 3 && rom.future_option.enabled && rom.rom == incoming.entries[i].rom_path,
                  "complete emulator options");
        }
        foreach (key in ["sor_name", "sor_date", "sor_genr", "sor_pnum", "sor_demo"]) {
            local ranks = {};
            for (local i = 0; i < count; i++) {
                local rank = catalogue.top.items[offset + i][key];
                check(rank >= 0 && rank < count && !(rank in ranks), "dense stable sort permutation");
                ranks[rank] <- true;
            }
        }
        if (!count) {
            check(catalogue.profile.game_versions_default.japan == "CHRONOS_EMPTY", "empty bootstrap");
            check(catalogue.profile.m2epi.version.CHRONOS_EMPTY.chronos_game_id == "", "empty has no owner");
        }
    }
}
// Stock-like examples deliberately keep the internal name, Japanese-menu
// spine title and other-language spine title distinct. Both lineups must use
// the same four-block contract, including original capitalization.
foreach (lineup in ["jp", "us"]) {
    local offset = lineup == "jp" ? 0 : 50;
    local titles = [
        ["天外魔境2", "天外魔境II 卍MARU", "Tengai Makyō II Manjimaru"],
        ["R-TYPE", "R-TYPE", "R-Type"],
        ["Power Golf", "POWER GOLF", "Power Golf"]
    ];
    incoming = { schema = 2, lineup = lineup, folder = "_root", path = "test",
        entries = [], settings = {}, entry_count = titles.len(), playable_count = titles.len() };
    foreach (index, titleset in titles) {
        local row = entry(index);
        row.game.display.name = titleset[0];
        row.game.display.tname = titleset[1];
        row.game.display.name_eng = titleset[2];
        incoming.entries.append(row);
    }
    catalogue.read(lineup, "_root");
    foreach (index, titleset in titles) {
        local japanese = catalogue.top.items[offset + index];
        local translated = catalogue.top.items[offset + 100 + index];
        check(japanese.name == titleset[0] && translated.name == titleset[0], "stock internal title");
        check(japanese.tname == titleset[1], "Japanese-menu spine uses the base-block tname");
        check(translated.tname == titleset[2], "other-language spine uses the block +100 tname");
        check(japanese.regionTag == translated.regionTag, "language change retains the game identity");
    }
}
incoming = { schema = 2, lineup = "jp", folder = "FOLDER_TEST", path = "test",
    entries = [entry(0, "back"), entry(1)], settings = {}, entry_count = 2, playable_count = 1 };
catalogue.read("jp", "FOLDER_TEST");
check(catalogue.top.items[0].csize == 11 && catalogue.top.items[0].game_id == "", "BACK owns no save");
check(catalogue.top.items[0].chronos_folder == "_root", "BACK target");
check(catalogue.top.items[0].sor_date == 0, "BACK pinned before custom sort");
local previous = catalogue.snapshot;
failure = true;
try { catalogue.read("us", "_root"); throw "read should fail"; }
catch (error) { check(error != "read should fail", "reject IO error"); }
check(catalogue.snapshot == previous, "failed read preserves old snapshot");
failure = false;
incoming = { schema = 2, lineup = "jp", folder = "_root", path = "test",
    entries = [entry(1), entry(1)], settings = {}, entry_count = 2, playable_count = 2 };
incoming.entries[1].game_id = "foreign";
try { catalogue.read("jp", "_root"); throw "duplicate should fail"; }
catch (error) { check(error != "duplicate should fail", "reject ambiguous profile"); }
check(catalogue.snapshot == previous, "failed profile preserves old snapshot");
local cursor_test = ChronosLibrary();
cursor_test.base_profile = catalogue.base_profile;
cursor_test.snapshot = previous;
cursor_test.remember_cursor(1);
check(cursor_test.locations.jp.cursor == 1, "remember current folder selection");
failure = false;
incoming = { schema = 2, lineup = "us", folder = "_root", path = "test",
    entries = [entry(2)], settings = {}, entry_count = 1, playable_count = 1 };
cursor_test.read("us", "_root");
cursor_test.remember_cursor(9);
check(cursor_test.locations.us.cursor == 0 && cursor_test.locations.jp.cursor == 1,
      "independent bounded per-lineup cursor memory");
local raw = { entries = { __chronos_array = 2, [0] = null,
    [1] = { nested = { __chronos_array = 1, [0] = true } } } };
local converted = chronos_normalize_json(raw);
check(typeof converted.entries == "array" && converted.entries.len() == 2 &&
      converted.entries[0] == null && converted.entries[1].nested[0], "verified array ABI adapter");

bindings <- [];
function get_current_title_prof() { return s_chronos_library.profile; }
function chronosSaveResetBindings(mode) { bindings = []; return "ok"; }
function chronosSaveBind(index, id, directory, tag, kind) {
    bindings.append([index, id, directory, tag, kind]); return "ok";
}
chronos_refresh_save_bindings(catalogue.top);
check(bindings.len() == 1 && bindings[0][0] == "1" && bindings[0][1] == "id1", "only real primary save binding");
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checker", type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="chronos-library-check-") as tmp:
        script = Path(tmp) / "library.nut"
        script.write_bytes((ROOT / "scripts-src/chronos_saves.nut").read_bytes() + b"\n" +
                           (ROOT / "scripts-src/chronos_library.nut").read_bytes() + b"\n" + CHECK.encode())
        subprocess.run([str(args.checker.resolve()), "-r", str(script)], check=True)
    print("PASS: source views 0–50, four JP/US language blocks, distinct internal/spine titles, BACK, options, sorting, rollback and save aliases")


if __name__ == "__main__":
    main()
