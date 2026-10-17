// Filesystem catalogue adapter. Only the current folder owns metadata; native
// filesystem work runs off-thread and all Squirrel/native graphics stay here.
s_chronos_library <- null;

function chronos_library_enabled()
{
    if (!("chronosLibraryEnabled" in getroottable())) return false;
    local status = ::chronosLibraryEnabled();
    if (status == "legacy") return false;
    if (status != "v2") throw "Chronos library: " + status;
    return true;
}

function chronos_dynamic_config(path)
{
    return path.find("/config/title_prof.psb") != null ||
           path.find("/config/title_prof_specdepend.psb") != null ||
           path.find("/config/title_mode_top.psb") != null;
}

function chronos_field(object, key, fallback)
{
    return object != null && key in object && object[key] != null ? object[key] : fallback;
}

function chronos_profile_options(source)
{
    // Optional nulls from older editors mean inheritance. Passing them to
    // the stock option reader would replace a valid global default with null.
    local result = {};
    foreach (key, value in source) {
        if (value == null || key == "regionTag" || key == "systemdata_sram_data_no" ||
            key.find("chronos_") == 0) continue;
        result[key] <- value;
    }
    return result;
}

function chronos_normalize_json(value)
{
    // The retail link omits sq_newarray. The verified C API emits temporary
    // integer-keyed tables; convert their arrays here without private VM ABI.
    if (typeof value != "table") return value;
    if ("__chronos_array" in value) {
        local result = array(value.__chronos_array);
        for (local i = 0; i < result.len(); i++) result[i] = ::chronos_normalize_json(value[i]);
        return result;
    }
    foreach (key, child in value) value[key] = ::chronos_normalize_json(child);
    return value;
}

function chronos_native_resource_path(path)
{
    // The retail resource loader roots paths at /usr/game, including a leading
    // slash. Resolve library assets without requiring extra console mounts.
    if (path.len() > 0 && path[0] == '/') return "../.." + path;
    return path;
}

function chronos_library()
{
    if (::s_chronos_library == null) ::s_chronos_library = ChronosLibrary();
    return ::s_chronos_library;
}

class ChronosLibrary
{
    snapshot = null;
    top = null;
    profile = null;
    base_profile = null;
    // Navigation memory contains names/cursors, never old folder snapshots.
    locations = null;

    constructor()
    {
        locations = { jp = { folder = "_root", cursor = 0 },
                      us = { folder = "_root", cursor = 0 } };
    }

    function read(lineup, folder, animate = null)
    {
        local ticket = ::beginReadFolder(lineup, folder);
        if (ticket.find("error:") == 0) throw ticket;
        local status = ::pollReadFolder(ticket);
        while (status == "busy") {
            if (animate != null) animate();
            wait(0);
            status = ::pollReadFolder(ticket);
        }
        if (status != "ready") throw "Chronos folder: " + status;
        local incoming = ::chronos_normalize_json(::takeFolder(ticket));
        if (typeof incoming != "table" || incoming.schema != 2)
            throw "Chronos folder: invalid snapshot";
        if (incoming.entries.len() > 50) throw "Chronos folder: more than 50 menu entries";
        local view = build_top(incoming);
        local config = build_profile(incoming, view);
        // Preparation may fail, but the old live folder remains intact until
        // both typed views are ready. Native bindings refresh after this commit.
        snapshot = incoming;
        top = view;
        profile = config;
        locations[lineup].folder = folder;
        if ("chronosRememberFolder" in getroottable()) {
            local saved = ::chronosRememberFolder(ticket);
            if (saved != "ok") printf("[CHRONOS-LIB] cursor persistence: %s\n", saved);
        }
        printf("[CHRONOS-LIB] %s/%s: %d entries, %d games\n",
               lineup, folder, incoming.entry_count, incoming.playable_count);
    }

    function remember_cursor(index)
    {
        if (snapshot == null) return;
        locations[snapshot.lineup].cursor = index >= 0 && index < snapshot.entry_count ? index : 0;
    }

    function ensure()
    {
        if (snapshot != null) return;
        local lineup = "jp";
        local folder = "_root";
        if ("chronosCurrentFolder" in getroottable()) {
            local current = ::chronosCurrentFolder();
            local slash = current.find("/");
            if (slash != null) {
                lineup = current.slice(0, slash);
                folder = current.slice(slash + 1);
            }
        }
        try { read(lineup, folder); }
        catch (error) {
            // A deleted last folder falls back only within the requested lineup;
            // an empty root is valid and never silently switches JP/US.
            if (folder == "_root") throw error;
            printf("[CHRONOS-LIB] previous folder unavailable: %s\n", error.tostring());
            read(lineup, "_root");
        }
    }

    function config(path)
    {
        ensure();
        if (path.find("title_mode_top.psb") != null) return top;
        if (path.find("title_prof_specdepend.psb") != null) return { root = {} };
        return { root = profile };
    }

    function static_profile()
    {
        if (base_profile != null) return base_profile;
        local resource = Resource();
        local path = ::conv_path("config/title_prof.psb");
        local spec = ::conv_path("config/title_prof_specdepend.psb");
        resource.load(path, spec);
        while (resource.loading) wait(0);
        local normal = ::copy_PSBValue(resource.find(path).root).root;
        local platform = ::copy_PSBValue(resource.find(spec).root).root;
        base_profile = ::merge_tables(platform, normal);
        // Stock controller/platform data is a fixed template. Drop its game
        // catalogue immediately; editor metadata supplies every live version.
        base_profile.game_versions <- {};
        if ("game_versions_layout" in base_profile) base_profile.rawdelete("game_versions_layout");
        base_profile.m2epi.version <- {};
        resource.unload();
        return base_profile;
    }

    function build_top(incoming)
    {
        local dummy = { name = "", tname = "", regionTag = "DUMMY", regionTagE = "NO",
            regionTagS1 = "NO", regionTagS2 = "NO", image = 0, players = 0,
            titlebar = 0, ccolor = 0, csize = 0, demo_time = 0, dev_name = "",
            EmuOfsX = 0, EmuOfsY = 0, ScreenMode = 0, action = "Boot",
            game_id = "", save_path = "", save_type = 0, sor_date = 0,
            sor_demo = 0, sor_genr = 0, sor_name = 0, sor_pnum = 0 };
        local result = { chronos_save_schema = 2, chronos_source_schema = 2,
            items = array(200, dummy), titleNum = 0, titleNumTG = 0,
            playable_count = incoming.playable_count };
        local offset = incoming.lineup == "jp" ? 0 : 50;
        result[incoming.lineup == "jp" ? "titleNum" : "titleNumTG"] = incoming.entries.len();
        local dev_name = ::chronos_field(static_profile().title, "dev_name", "PceMini");
        foreach (index, entry in incoming.entries) {
            local item = clone dummy;
            local game = entry.game;
            local display = ::chronos_field(game, "display", {});
            local emulator = ::chronos_field(game, "emulator", {});
            local name = ::chronos_field(display, "name", "");
            local japanese = ::chronos_field(display, "tname", name);
            if (japanese == "") japanese = name;
            local english = ::chronos_field(display, "name_eng", japanese);
            if (english == "") english = japanese;
            if (entry.role != "game") {
                name = entry.role == "back" ? "Back" : ::chronos_field(entry.folder_metadata, "name", entry.folder);
                japanese = entry.role == "back" ? "戻る" : name;
                english = name;
            }
            item.name = name;
            item.tname = japanese;
            item.dev_name = dev_name;
            item.regionTag = entry.role == "game" ? entry.native_region_tag : "NAV" + index.tostring();
            item.image = index;
            item.chronos_entry <- entry;
            item.chronos_role <- entry.role;
            item.chronos_folder <- entry.role == "back" ? "_root" : entry.folder;
            foreach (key in ["players", "titlebar", "ccolor", "csize", "demo_time"])
                item[key] = ::chronos_field(display, key, key == "players" ? 1 : 0);
            if (entry.role != "game") {
                item.csize = entry.role == "back" ? 11 : 10;
                item.titlebar = 0;
                item.ccolor = 0;
                item.players = 0;
                item.demo_time = 0;
            } else {
                item.game_id = entry.game_id;
                item.save_path = entry.save_path;
                item.save_type = game.rom.arch.find("cd") != null ? 11 : 12;
                local alts = ::chronos_field(game, "rom_alt", {});
                foreach (key in ["regionTagE", "regionTagS1", "regionTagS2"])
                    item[key] = ::chronos_field(alts, key, "NO");
            }
            foreach (key in ["EmuOfsX", "EmuOfsY", "ScreenMode"])
                item[key] = ::chronos_field(emulator, key, 0);
            // These retail fields have no identified visible consumer, but
            // remain editable metadata and must survive the live menu path.
            foreach (key in ["action", "copy", "desc"])
                item[key] <- ::chronos_field(display, key, key == "action" ? "Boot" : "");
            result.items[offset + index] = item;
            local translated = clone item;
            translated.tname = english;
            result.items[offset + 100 + index] = translated;
        }
        // Native sorting indexes must be dense permutations. Editor values
        // remain authoritative; stable row-order breaks ties, BACK stays first.
        foreach (key in ["sor_date", "sor_demo", "sor_genr", "sor_name", "sor_pnum"]) {
            local order = [];
            foreach (index, entry in incoming.entries) order.append(index);
            order.sort(function(a, b):(incoming, key) {
                local ea = incoming.entries[a], eb = incoming.entries[b];
                if (ea.role == "back" && eb.role != "back") return -1;
                if (eb.role == "back" && ea.role != "back") return 1;
                local va = ::chronos_field(ea.sort, key, a);
                local vb = ::chronos_field(eb.sort, key, b);
                return va < vb ? -1 : (va > vb ? 1 : (a < b ? -1 : (a > b ? 1 : 0)));
            });
            foreach (rank, index in order) {
                result.items[offset + index][key] = rank;
                result.items[offset + 100 + index][key] = rank;
            }
        }
        return result;
    }

    function build_profile(incoming, view)
    {
        local result = clone static_profile();
        result.m2epi = clone result.m2epi;
        result.m2epi.version = {};
        result.m2epi.chronos_save_schema <- 2;
        result.m2epi.sram_common <- true;
        result.m2epi.sram_support <- true;
        result.game_versions = {};
        local first = null;
        local offset = incoming.lineup == "jp" ? 0 : 50;
        foreach (index, entry in incoming.entries) {
            if (entry.role != "game") continue;
            local rom = ::chronos_profile_options(entry.game.rom);
            // Paths supplied by the hook are already validated against the
            // shared ROM store, and use the native loader's system-root form.
            rom.rom = entry.rom_path;
            rom.chronos_game_id <- entry.game_id;
            rom.chronos_save_path <- entry.save_path;
            local item = view.items[offset + index];
            foreach (tag in [entry.native_region_tag, item.regionTagE, item.regionTagS1, item.regionTagS2]) {
                if (tag == "NO" || tag == "") continue;
                if (tag in result.m2epi.version) {
                    if (result.m2epi.version[tag].chronos_game_id == entry.game_id) continue;
                    throw "Chronos duplicate native tag: " + tag;
                }
                result.m2epi.version[tag] <- rom;
                result.game_versions[tag] <- [index, 0, item.tname];
            }
            if (first == null) first = entry.native_region_tag;
        }
        if (first == null) {
            first = "CHRONOS_EMPTY";
            result.m2epi.version[first] <- { arch = "tg16", country = "japan", preamp = 1.0,
                rom = "roms/CHRONOS_EMPTY.pce", sram_support = false,
                chronos_game_id = "", chronos_save_path = "" };
            result.game_versions[first] <- [0, 0, ""];
        }
        result.game_versions_default <- { japan = first, usa = first, europe = first, asia = first };
        return result;
    }
}
