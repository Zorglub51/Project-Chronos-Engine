// Per-game persistence adapter. Slots are always 0..3 internally and 1..4
// in the UI. Folder/BACK cards never acquire bindings or SRAM ownership.
s_chronos_save_schema <- 0;
s_chronos_active_game_id <- null;
s_chronos_pending_game_id <- null;
s_chronos_save_failed <- false;

function chronos_abort_game_init()
{
    // Block every later write before cleanup, even if the USB disappears and
    // deactivation itself fails. Never let the old core run as the new owner.
    ::s_chronos_save_failed = true;
    ::s_chronos_active_game_id = null;
    ::s_chronos_pending_game_id = null;
    if (::g_emu_task != null) {
        ::g_emu_task.pause = true;
        ::g_emu_task.visible = false;
    }
    local status = ::chronosSaveActivate("");
    if (status != "ok") printf("[CHRONOS-SAVE] cleanup: %s\n", status);
}

function chronos_save_enabled()
{
    local profile = ::get_current_title_prof();
    return profile != null && "m2epi" in profile &&
           "chronos_save_schema" in profile.m2epi &&
           profile.m2epi.chronos_save_schema == 2;
}

function chronos_require_save_status(result, operation)
{
    if (result != "ok") throw "Chronos " + operation + ": " + result;
}

// The current catalogue supplies the complete alias map, including games
// whose saves are only being previewed. A future ChronosLibrary snapshot can
// pass the same table directly; no save path depends on sorting or image slots.
function chronos_refresh_save_bindings(config = null)
{
    local enabled = ::chronos_save_enabled();
    if (!("chronosSaveResetBindings" in getroottable())) {
        if (enabled) throw "This library requires the Chronos per-game save hook";
        return false;
    }
    if (!enabled) {
        ::chronos_require_save_status(::chronosSaveResetBindings("legacy"), "legacy save context");
        ::s_chronos_save_schema = 1;
        ::s_chronos_active_game_id = null;
        ::s_chronos_pending_game_id = null;
        return false;
    }
    if (config == null) config = ::util_load_config(::conv_path("config/title_mode_top.psb"));
    if (!("chronos_save_schema" in config) || config.chronos_save_schema != 2)
        throw "Chronos catalogue/save schema mismatch";
    ::chronos_require_save_status(::chronosSaveResetBindings("v2"), "reset save bindings");
    // Slots100..199 are translated views of0..99, not additional games.
    for (local native_index = 0; native_index < 100; native_index++) {
        local item = config.items[native_index];
        if (!("game_id" in item) || item.game_id == "") continue;
        if (item.csize == 10 || item.csize == 11) throw "Navigation card has a save identity";
        ::chronos_require_save_status(::chronosSaveBind(native_index.tostring(),
            item.game_id, item.save_path, item.regionTag, item.save_type.tostring()),
            "bind game " + item.game_id);
    }
    ::s_chronos_save_schema = 2;
    return true;
}

// Load only SRAM from the native aggregate. Never restore old menu settings
// or apply this bank to the outgoing emulator during a folder transition.
function chronos_reload_sram()
{
    local control = ::get_backup_control(BackupSegmentTypes.TITLE_DATA);
    local segment = control.get_backup_segment(BackupSegmentTypes.TITLE_DATA);
    if (!control.load_single(segment, true)) throw "Cannot load Chronos SRAM bank";
    while (segment.running) wait(0);
    if (!segment.success) throw "Chronos SRAM bank could not be read";
    // systemdata.nut declares this const after utils has compiled. Use the
    // native field key here; a forward const reference becomes a root lookup.
    local saved = ::g_systemdata.get_current_backup_struct_title_value()["_92_sram_datas"];
    local sram = ::g_systemdata.get_value(SystemDataValueIndex.SRAM_DATA);
    if (sram.m_emu_sram == null || sram.m_emu_sram.len() != 1) sram.init();
    if (!BinaryUtil.CopyStructValue(sram.get_current_data()[0], saved[0]))
        throw "Cannot copy Chronos SRAM bank";
    sram.load_from_struct(saved);
    sram.complete(false);
    printf("[CHRONOS-SAVE] bank ready\n");
}

function chronos_flush_sram()
{
    if (!::chronos_save_enabled()) return;
    if (::s_chronos_save_failed) throw "Chronos save context failed; restart the menu";
    // The hook retains the previous owner even after its folder is left.
    ::chronos_require_save_status(::chronosSaveFlush(), "flush SRAM");
}

// Capture directly from the owned task: the current folder's region table
// can already describe a different game. No native SRAM entries is normal
// for a HuCard; a failed capture must not replace the last verified bank.
function chronos_capture_sram()
{
    if (::chronos_save_enabled() && ::s_chronos_save_failed)
        throw "Chronos save context failed; restart the menu";
    if (!::chronos_save_enabled() || ::s_chronos_active_game_id == null ||
        ::g_emu_task == null || !::g_emu_task.initialized) return;
    local sram = ::g_systemdata.get_value(SystemDataValueIndex.SRAM_DATA);
    local had_sram = sram.get_current_data()[0]["_00_sram_entries"][0]["_03_size_dec"] > 0;
    if (sram.m_emu_sram[0].capture_from_emu(::g_emu_task)) {
        if (!sram.m_emu_sram[0].copy_to(sram.get_current_data()[0]))
            throw "Cannot copy current Chronos SRAM";
    } else if (had_sram || ::s_init_emulator_arch_current == "tg16cd") {
        throw "Cannot capture current Chronos SRAM";
    }
}

function chronos_before_game_change()
{
    if (!::chronos_save_enabled()) return;
    if (::s_chronos_active_game_id != null && ::g_emu_task != null && ::g_emu_task.initialized) {
        // The save body captures the outgoing task before serializing slot0.
        if (!(::g_systemdata.save_systemdata(true) & SystemDataOpeBitFlag.TITLE))
            throw "Cannot save outgoing Chronos SRAM";
    }
    ::chronos_flush_sram();
}

// Stock change_title/update_system_all can apply the common SRAM bank to
// the old task. Save and release its owner before that code runs; import the
// incoming owner's SRAM only immediately before native init_emulator.
function chronos_prepare_game_change(regionTag)
{
    if (!::chronos_save_enabled()) return;
    local versions = ::get_current_title_prof().m2epi.version;
    if (regionTag in versions && "chronos_game_id" in versions[regionTag] &&
        versions[regionTag].chronos_game_id == ::s_chronos_active_game_id) return;
    ::chronos_before_game_change();
    ::chronos_require_save_status(::chronosSaveActivate(""), "release outgoing game SRAM");
    ::s_chronos_active_game_id = null;
    ::s_chronos_pending_game_id = null;
}

function chronos_activate_game_saves(regionTag)
{
    if (!::chronos_save_enabled()) return false;
    if (::s_chronos_save_schema != 2) ::chronos_refresh_save_bindings();
    local versions = ::get_current_title_prof().m2epi.version;
    if (!(regionTag in versions) || !("chronos_game_id" in versions[regionTag]) ||
        versions[regionTag].chronos_game_id == "") {
        ::chronos_before_game_change();
        ::chronos_require_save_status(::chronosSaveActivate(""), "clear navigation save owner");
        ::s_chronos_active_game_id = null;
        ::s_chronos_pending_game_id = null;
        return false;
    }
    local id = versions[regionTag].chronos_game_id;
    if (id == ::s_chronos_active_game_id) return false;
    try {
        ::chronos_before_game_change();
        ::chronos_require_save_status(::chronosSaveActivate(id), "activate game " + id);
        ::s_chronos_pending_game_id = id;
        ::chronos_reload_sram();
    } catch (e) {
        ::chronos_abort_game_init();
        throw e;
    }
    return true;
}

function chronos_complete_game_init(success)
{
    if (!::chronos_save_enabled()) return;
    if (::s_chronos_active_game_id == null && ::s_chronos_pending_game_id == null) return;
    if (!success) {
        ::chronos_abort_game_init();
        throw "The selected game could not initialize; its saves were preserved";
    }
    if (::s_chronos_pending_game_id == null) return;
    ::s_chronos_active_game_id = ::s_chronos_pending_game_id;
    ::s_chronos_pending_game_id = null;
    printf("[CHRONOS-SAVE] active=%s slots=1..4\n", ::s_chronos_active_game_id);
}

// A remembered selection can be BACK after resuming a folder. Bootstrap
// the first playable emulator without changing carousel/save indices.
function chronos_resolve_game_region(regionTag)
{
    if (!::chronos_save_enabled()) return regionTag;
    local profile = ::get_current_title_prof();
    local versions = profile.m2epi.version;
    if (regionTag in versions && "chronos_game_id" in versions[regionTag] &&
        versions[regionTag].chronos_game_id != "") return regionTag;
    local first = null;
    local first_index = 1000;
    foreach (tag, entry in profile.game_versions) {
        if (!(tag in versions) || !("chronos_game_id" in versions[tag]) ||
            versions[tag].chronos_game_id == "") continue;
        if (entry[0] < first_index) { first = tag; first_index = entry[0]; }
    }
    if (first == null) {
        first = ::get_first_game_regionTag();
        if (!(first in versions) || !("sram_support" in versions[first]) ||
            versions[first].sram_support) throw "Missing navigation bootstrap profile";
    }
    ::g_systemdata.set_data_setting_etc__game_regionTag(first);
    return first;
}

function chronos_get_save_locks(fallback)
{
    if (!::chronos_save_enabled()) return fallback;
    if (::s_chronos_active_game_id == null) throw "Missing Chronos game save context";
    local mask = ::chronosSaveGetLocks(::s_chronos_active_game_id);
    if (mask == "missing") return 0;
    local value = mask.tointeger();
    if (value < 0 || value > 15) throw "Invalid Chronos save lock mask";
    return value;
}

function chronos_set_save_locks(mask)
{
    if (!::chronos_save_enabled()) return false;
    if (::s_chronos_active_game_id == null) throw "Missing Chronos game save context";
    ::chronos_require_save_status(::chronosSaveSetLocks(::s_chronos_active_game_id,
        (mask & 15).tostring()), "save slot locks");
    return true;
}
