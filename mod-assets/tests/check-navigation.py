#!/usr/bin/env python3
"""Exercise the shipped folder recovery and empty title message in Squirrel 2.2."""
import argparse
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def method(source, name):
    match = re.search(r"\bfunction\s+" + re.escape(name) + r"\s*\(", source)
    if not match:
        raise ValueError(f"Missing shipped method: {name}")
    start = source.index("{", match.end())
    depth, state, quote, index = 1, "code", None, start + 1
    while index < len(source):
        char = source[index]
        pair = source[index:index + 2]
        if state == "line":
            if char == "\n":
                state = "code"
        elif state == "block":
            if pair == "*/":
                state = "code"
                index += 1
        elif state == "quote":
            if char == "\\":
                index += 1
            elif char == quote:
                state = "code"
        elif pair in ("//", "/*"):
            state = "line" if pair == "//" else "block"
            index += 1
        elif char in ('"', "'"):
            state, quote = "quote", char
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return source[match.start():index + 1]
        index += 1
    raise ValueError(f"Unclosed shipped method: {name}")


MOCK = r'''
function check(value, message) { if (!value) throw message; }
printf <- function(a, b = null, c = null, d = null) {};
LINEUP_JP <- 0;
LINEUP_US <- 1;
KEYWAIT <- 20;
INDEX_LENGTH_MAX <- 700;
s_titleNum <- 4;
s_last_linenp <- LINEUP_JP;
s_last_sortType <- 4;
s_last_index <- 2;
s_in_folder <- false;
active_pools <- 0;
peak_pools <- 0;
failed_constructor_folder <- null;
failed_selection_folder <- null;
failed_read_folder <- null;
remembered_folder <- "_root";
bound_folder <- "_root";
save_flag <- 1;
background_frames <- 0;
kun_execs <- 0;
background_updates <- 0;
artwork_waits <- 0;
cover_primes <- 0;
g_bg_count <- 0;
function wait(frame) { background_frames++; }
function get_yure(count, range, strength) { return 0; }
function conv_path(path) { return path; }
function IndicatorSetXScale(indicator, size) { indicator.scale_bound <- size; }
g_frameCount <- { setSaveExec = function(value) { save_flag = value; } };
catalogue <- { remember_cursor = function(index) {}, snapshot = { lineup = "jp", folder = "_root" }, reads = [],
    read = function(lineup, folder, animate = null) {
        check(folder != failed_read_folder, "simulated previous-folder read error");
        if (animate != null) animate();
        catalogue.reads.append(folder);
        catalogue.snapshot = { lineup = lineup, folder = folder };
        remembered_folder = folder;
    } };
function chronos_library() { return catalogue; }
function refresh_pack_save_context() { bound_folder = catalogue.snapshot.folder; save_flag = 0; }
function util_load_config(path) {
    check(path == "config/title_mode_top.psb", "unexpected config reload");
    local count = catalogue.snapshot.folder == "_root" ? 4 : 5;
    return { titleNum = count, titleNumTG = count, folder = catalogue.snapshot.folder };
}

class MenuModeSelectPkg {
    config = null;
    index = 0;
    sort = null;
    visible = false;
    closed = false;
    fade_in = null;
    animate_wait = null;
    constructor(value, bg, down, resource, fade = true, animate = null) {
        config = value;
        fade_in = fade;
        animate_wait = animate;
        active_pools++;
        if (active_pools > peak_pools) peak_pools = active_pools;
        check(active_pools <= 1, "folder recovery retains two image pools");
        // A source folder's thumbnail/resource waits must keep the stock
        // translation updates alive, not only native Motion frame playback.
        check(animate != null, "folder artwork has no animation callback");
        for (local frame = 0; frame < 3; frame++) {
            animate();
            artwork_waits++;
            wait(0);
        }
        if (config.folder == failed_constructor_folder) {
            // Real constructor error cleanup is checked in check-covers.py.
            active_pools--;
            closed = true;
            throw "simulated cover constructor error";
        }
    }
    function setImageID(value, update = true) {
        sort = value;
        if (update) {
            cover_primes++;
            check(config.folder != failed_selection_folder, "simulated sorted-cover error");
        }
    }
    function setIndex(value) {
        cover_primes++;
        check(config.folder != failed_selection_folder, "simulated sorted-cover error");
        index = value;
    }
    function getIndex() { return index; }
    function setVisible(value) { visible = value; }
    function close() { if (!closed) active_pools--; closed = true; visible = false; animate_wait = null; }
}
'''

RECOVERY_CLASS = r'''
class RecoveryHarness {
    m_selectPkg = null;
    m_config = null;
    m_motion_bg = null;
    m_motion_bg_down = null;
    m_rsc = null;
    m_current_sortType = 4;
    m_current_linenp = 0;
    m_current_index = 2;
    m_keywait = 0;
    m_pceKun = null;
    m_motion_bg_back = null;
    errors = null;
    constructor() {
        errors = [];
        // Match startup: PceKun instances do not exist until after artwork.
        local owner = this;
        m_motion_bg_back = [{setVariable = function(key, value) { background_updates++; }}];
        local before = kun_execs;
        m_selectPkg = MenuModeSelectPkg({folder = "_root"}, null, null, null, true,
            function():(owner) { owner.animate_folder_wait(); });
        check(kun_execs == before, "startup tried to advance PceKun before construction");
        m_pceKun = [];
        for (local i = 0; i < 11; i++) m_pceKun.append({exec = function() { kun_execs++; }});
        m_selectPkg.setImageID(4, false);
        m_selectPkg.setIndex(2);
        m_selectPkg.setVisible(true);
    }
    function show_folder_error(message) {
        check(m_selectPkg != null && !m_selectPkg.closed && m_selectPkg.visible,
              "error shown without a restored visible menu");
        errors.append(message);
    }
'''

RECOVERY_CHECK = r'''
foreach (failure in ["none", "constructor", "sorted_selection"]) {
    catalogue.snapshot = {lineup = "jp", folder = "_root"};
    remembered_folder = "_root";
    bound_folder = "_root";
    save_flag = 1;
    catalogue.reads = [];
    local owner = RecoveryHarness();
    local previous = {lineup = "jp", folder = "_root", index = 2, sort = 4};
    catalogue.read("jp", "FOLDER_NEW");
    if (failure == "constructor") failed_constructor_folder = "FOLDER_NEW";
    if (failure == "sorted_selection") failed_selection_folder = "FOLDER_NEW";
    local kun_before = kun_execs, waits_before = artwork_waits, updates_before = background_updates;
    local primes_before = cover_primes;
    local succeeded = owner.finish_source_folder_swap(previous, 1);
    if (failure == "none") {
        check(succeeded && owner.errors.len() == 0, "successful folder transition reports error");
        check(owner.m_selectPkg.config.folder == "FOLDER_NEW" && owner.m_selectPkg.index == 1,
              "new folder/cursor not activated");
        check(remembered_folder == "FOLDER_NEW" && bound_folder == "FOLDER_NEW" && s_in_folder,
              "native metadata/save context differs from visible new folder");
        check(cover_primes == primes_before + 1, "folder loads intermediate physical/sorted windows before the final cursor");
    } else {
        check(!succeeded && owner.errors.len() == 1, "bad artwork not reported");
        check(owner.m_selectPkg.config.folder == "_root" && owner.m_selectPkg.index == 2 &&
              owner.m_selectPkg.sort == 4 && !owner.m_selectPkg.fade_in,
              "previous cursor/sort/menu not restored");
        check(remembered_folder == "_root" && bound_folder == "_root" && !s_in_folder,
              "recovery restores only a script view, not native ownership/path");
        check(catalogue.reads.len() == 2 && catalogue.reads[1] == "_root", "previous source folder not reread");
    }
    check(owner.m_selectPkg.visible && active_pools == 1 && save_flag == 0,
          "menu remains hidden, resources retained or save gate locked");
    local waited = artwork_waits - waits_before;
    check(waited >= 3 && kun_execs - kun_before >= waited * 11,
          "characters animate on the spot while folder artwork loads");
    check(background_updates - updates_before >= waited * 8,
          "script-driven background stops while folder artwork loads");
    failed_constructor_folder = null;
    failed_selection_folder = null;
    owner.m_selectPkg.close();
}
// A removed previous folder cannot be reconstructed, but the diagnostic must
// include both failures and must not leave save IO blocked or an Image pool.
catalogue.snapshot = {lineup = "jp", folder = "_root"};
local owner = RecoveryHarness();
catalogue.read("jp", "FOLDER_NEW");
failed_constructor_folder = "FOLDER_NEW";
failed_read_folder = "_root";
local diagnostic = null;
try { owner.finish_source_folder_swap({lineup = "jp", folder = "_root", index = 2, sort = 4}, 1); }
catch (error) { diagnostic = error; }
check(diagnostic != null && diagnostic.find("cover constructor error") != null &&
      diagnostic.find("previous-folder read error") != null, "secondary failure loses useful diagnostics");
check(active_pools == 0 && save_flag == 0, "secondary error leaks resources or blocks saves");
failed_constructor_folder = null;
failed_read_folder = null;
check(peak_pools == 1, "recovery exceeded one resident Image pool");
'''

EMPTY_CLASS = r'''
class EmptyHarness {
    m_chronos_covers = null;
    m_isExec = false;
    m_isVisible = false;
    m_text_Title = null;
    m_motion_bg = null;
    m_motion_bg_down = null;
    m_cursol_motion = null;
    m_thumbnail_cursol_motion = null;
    constructor() {
        m_text_Title = { visible = false, prints = [], print = function(text) { this.prints.append(text); } };
        m_motion_bg = { values = {}, setVariable = function(name, value) { this.values[name] <- value; } };
        m_motion_bg_down = { values = {}, setVariable = function(name, value) { this.values[name] <- value; } };
        m_cursol_motion = {visible = true};
        m_thumbnail_cursol_motion = {visible = true};
    }
'''

EMPTY_CHECK = r'''
s_titleNum = 0;
local empty = EmptyHarness();
empty.setVisible(true);
check(empty.m_text_Title.prints.len() == 1 &&
      empty.m_text_Title.prints[0] == "#{color,000000FF}No games in this folder",
      "empty message is missing or white on a white titlebar");
for (local frame = 0; frame < 10; frame++) empty.exec();
check(empty.m_text_Title.prints.len() == 1, "empty text is rebuilt every frame");
check(empty.m_text_Title.visible && !empty.m_cursol_motion.visible && !empty.m_thumbnail_cursol_motion.visible,
      "empty menu shows cursors or hides its message");
check(empty.m_text_Title.scale_bound == 700 && empty.m_motion_bg.values.titlebar == 0 &&
      empty.m_motion_bg.values.saveslot4 == 0, "empty titlebar retains game decorations");
local ticks = {count = 0};
empty.m_chronos_covers = {tick = function():(ticks) { ticks.count++; }};
for (local frame = 0; frame < 10; frame++) empty.exec();
check(ticks.count == 10, "cover prefetch is not polled once per menu frame");
empty.m_isExec = true;
empty.exec();
check(ticks.count == 10, "unrelated cover prefetch runs during the launch animation");
'''

SCHEDULING_CLASS = r'''
fade_movements <- 0;
class SchedulingHarness {
    m_sort = null;
    sort_builds = 0;
    cover_syncs = 0;
    m_pkgselect_motion = null;
    m_pkgselect_s_motion = null;
    m_cursol_motion = null;
    m_thumbnail_cursol_motion = null;
    m_animate_wait = null;
    constructor() {
        m_pkgselect_motion = [{opacity = 255}, {opacity = 255}];
        m_pkgselect_s_motion = [{opacity = 255}, {opacity = 255}];
        m_cursol_motion = {opacity = 255};
        m_thumbnail_cursol_motion = {opacity = 255};
        m_animate_wait = function() { fade_movements++; };
    }
    function indexTableUpdate() { sort_builds++; }
    function selectPkgUpdate() { cover_syncs++; }
'''

SCHEDULING_CHECK = r'''
local schedule = SchedulingHarness();
schedule.setImageID(2);
check(schedule.m_sort == 2 && schedule.sort_builds == 1 && schedule.cover_syncs == 1,
      "standalone sort no longer refreshes the displayed covers");
schedule.setImageID(3, false);
check(schedule.m_sort == 3 && schedule.sort_builds == 2 && schedule.cover_syncs == 1,
      "deferred sort starts an intermediate cover load");
local frames_before = background_frames;
schedule.fadeOut(12);
check(background_frames == frames_before + 12 && fade_movements == 12,
      "fade duration changed or movement callback was skipped");
foreach (motion in schedule.m_pkgselect_motion) check(motion.opacity == 0, "package fade remains opaque");
foreach (motion in schedule.m_pkgselect_s_motion) check(motion.opacity == 0, "thumbnail fade remains opaque");
check(schedule.m_cursol_motion.opacity == 0 && schedule.m_thumbnail_cursol_motion.opacity == 0,
      "cursor fade changed");
schedule.m_animate_wait = null;
schedule.fadeOut(1);
check(fade_movements == 12, "legacy fade requires the optional movement callback");
'''

MOTION_INIT_MOCK = r'''
CUNTERINDEX <- 2; MOTSMOOTHING <- false; PRI_PKG <- 5; PRI_TEXT <- 10; PRI_CSL <- 6;
TEXT_COLOR_NORMAL <- 0; PKGNAME_TEXTOFFSET_Y <- 169; PKG_Y <- -10;
THUMBNAIL_W <- 34; THUMBNAIL_Y <- -194; LINEUPMAX <- 50; DEMOVERSISON <- 0;
CONSOLE <- {ALIGNMENT_CENTER = 0};
s_ui_motionPath <- "ui";
s_rsc <- {find = function(path) { return path; }};
s_indextable <- array(50, 50);
init_state <- null;
function chronos_library_enabled() { return init_state.source; }
function getTitleMultiFontPath() { return "font"; }
function getMotionPath() { return "covers.motion"; }
function getFont32Yoffset() { return 0; }
function getPkgIndex(index, sort, config) { return sort == 2 ? s_titleNum - index - 1 : index; }
function getLinenpOffsetConfigData(index, key) { return key == "image" ? s_indextable[index] : 0; }
class ScaledLayer {
    visible = false; smoothing = false; priority = 0;
    function registerMotionResource(path) {
        if (path != "covers.motion") return;
        check(!init_state.source || s_titleNum == 0 || init_state.ready,
              "native motion resource binds transparent placeholders before final cover prime");
        init_state.registrations++;
    }
}
class Indicator {
    visible = false; fontColor = 0;
    constructor(layer, font) {}
    function setRecognizeTag(value) {}
    function setAlignment(value) {}
    function print(value) {}
    function setCoord(x, y) {}
}
class Motion {
    chara = ""; motion = ""; opacity = 0; visible = false;
    independentLayerInherit = false; left = 0; top = 0;
    values = null; cached_ready = false;
    constructor(layer) {
        init_state.motions++;
        values = {pkg = 0};
    }
    function progress() {
        // Model the retail cache: assigning pkg=0 again does not invalidate
        // the texture captured when the native motion was constructed.
        cached_ready = !init_state.source || s_titleNum == 0 || init_state.ready;
        check(cached_ready, "native Motion captures transparent slot zero");
    }
    function setVariable(key, value) {
        if (!(key in values) || values[key] != value)
            cached_ready = !init_state.source || s_titleNum == 0 || init_state.ready;
        values[key] <- value;
    }
}
class ChronosCoverPool {
    closed = false;
    constructor(config, main, small, animate = null) { init_state.pools++; }
    function sync(work) {
        if (init_state.ready) return;
        init_state.primes++;
        if (init_state.fail) throw "simulated final cover prime failure";
        init_state.final_work = clone work;
        init_state.ready = true;
    }
    function slot(index, required = true) { return index % 7; }
    function close() { if (!closed) init_state.pools--; closed = true; }
}
'''

MOTION_INIT_CHECK = r'''
// Run the actual shipped constructor and initialization/selection methods.
// Only graphics/filesystem native objects are mocked.
foreach (source in [true, false]) {
    foreach (count in [0, 11]) {
        s_titleNum = count;
        init_state = {source = source, ready = false, fail = false, pools = 0,
                      primes = 0, registrations = 0, motions = 0, final_work = null};
        local package = MotionInitHarness({}, null, null, s_rsc);
        if (source && count > 0) {
            check(init_state.primes == 0 && init_state.registrations == 0 && init_state.motions == 0,
                  "source constructor binds native resources/motions before final selection");
            check(package.m_cursol_motion == null, "source cursor was initialized prematurely");
        } else {
            check(init_state.registrations == 2 && init_state.motions == count * 2 + 2 &&
                  package.m_cursol_motion != null, "legacy/empty constructor no longer initializes immediately");
        }
        package.setImageID(2, false);
        check(init_state.primes == 0, "sort starts an intermediate source cover prime");
        package.setIndex(4);
        check(init_state.primes == (source && count > 0 ? 1 : 0), "final source selection did not prime exactly once");
        check(init_state.registrations == 2 && init_state.motions == count * 2 + 2,
              "deferred initialization has missing or duplicate native motions/resources");
        check(package.m_pending_motion_resource == null, "initialized package retains its pending PSB");
        if (count > 0) {
            foreach (motion in package.m_pkgselect_motion)
                check(motion.cached_ready, "first visible package still caches the transparent image");
            for (local index = 0; index < count; index++)
                check(package.m_pkgselect_s_motion[index].values.pkg == s_indextable[index],
                      "deferred thumbnails lost the final sorted mapping");
            if (source) {
                foreach (index, value in init_state.final_work)
                    check(package.m_pkgselect_work[index] == value, "prime used the cursor before its final rotation");
            }
        }
        package._init();
        check(init_state.registrations == 2 && init_state.motions == count * 2 + 2,
              "package initializes native motions twice");
        package.close();
        check(init_state.pools == 0 && package.m_pending_motion_resource == null,
              "package close leaves its pool or deferred PSB alive");
    }
}
s_titleNum = 11;
init_state = {source = true, ready = false, fail = true, pools = 0,
              primes = 0, registrations = 0, motions = 0, final_work = null};
local failed = MotionInitHarness({}, null, null, s_rsc);
failed.setImageID(2, false);
local prime_error = null;
try { failed.setIndex(4); } catch (error) { prime_error = error; }
check(prime_error != null && init_state.registrations == 0 && init_state.motions == 0,
      "failed prime constructs native motions against incomplete bindings");
failed.close();
check(init_state.pools == 0 && failed.m_pending_motion_resource == null,
      "failed prime leaks its deferred native resource or image pool");
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checker", type=Path, required=True)
    args = parser.parse_args()
    source = (ROOT / "scripts-src/mode_title_select.nut").read_text()
    # SQ2.2 resolves compiler constants only after their declaration; a later
    # declaration leaves earlier methods trying a missing runtime root slot.
    assert source.index("const INDEX_LENGTH_MAX") < source.index("class MenuModeTitleSelectSub"), \
        "Title width must be declared before the submenu error method"
    package = source[source.index("class MenuModeSelectPkg"):]
    constructor = package[:package.index("\n\tfunction _init()")].replace(
        "class MenuModeSelectPkg", "class MotionInitHarness", 1)
    script_text = MOCK + RECOVERY_CLASS + "\n".join(
        method(source, name) for name in ["rebuild_source_folder_package", "finish_source_folder_swap", "animate_folder_wait"]
    ) + "\n}\n" + RECOVERY_CHECK + EMPTY_CLASS + "\n".join(
        method(package, name) for name in ["setVisible", "nameTextUpdate", "exec"]
    ) + "\n}\n" + EMPTY_CHECK + SCHEDULING_CLASS + "\n".join(
        method(package, name) for name in ["setImageID", "fadeOut"]
    ) + "\n}\n" + SCHEDULING_CHECK + MOTION_INIT_MOCK + constructor + "\n".join(
        method(package, name) for name in ["_init", "setImageID", "setIndex", "move_l", "move_r",
                                          "selectPkgUpdate", "indexTableUpdate", "close"]
    ) + '\nfunction nameTextUpdate() { check(m_cursol_motion != null, "title update before cursor initialization"); }\n}\n' + MOTION_INIT_CHECK
    with tempfile.TemporaryDirectory(prefix="chronos-navigation-check-") as temporary:
        script = Path(temporary) / "navigation.nut"
        script.write_text(script_text)
        subprocess.run([str(args.checker.resolve()), "-r", str(script)], check=True)
    print("PASS: folder artwork keeps movement/background updates, primes once and recovers with one pool; prefetch ticks once per frame and stops during launch; empty message remains stable")


if __name__ == "__main__":
    main()
