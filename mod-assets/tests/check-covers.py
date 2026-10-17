#!/usr/bin/env python3
"""Check the shipped cover pool's bounds and resource lifetimes in Squirrel 2.2."""
import argparse
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CHECK = r'''
function check(value, message) { if (!value) throw message; }
printf <- function(a, b = null, c = null, d = null, e = null, f = null) {};
resources <- [];
layers <- [];
resource_peak <- { cover = 0, thumb = 0, clear = 0 };
delay_frames <- 0;
wait_count <- 0;
allow_wait <- true;
cover_loads <- 0;
thumbnail_batches <- 0;
images_created <- 0;
prefetch_reports <- 0;
failed_load_path <- null;
failed_find_path <- null;
failed_image_path <- null;
failed_image_label <- null;
failed_registration_key <- null;
cache_clears <- 0;
registered_total <- 0;
unregistered_total <- 0;
next_id <- 1;
catalogue <- { snapshot = { lineup = "jp", entries = [] } };
s_indextable <- [];
printf = function(a, b = null, c = null, d = null, e = null, f = null) {
    if (a.find("prefetch rejected") != null) prefetch_reports++;
};

function advance_resources() {
    foreach (resource in resources) if (resource.alive && resource.loading) {
        resource.remaining--;
        if (resource.remaining == 0) resource.loading = false;
    }
}
function wait(value) {
    check(allow_wait, "unexpected blocking wait");
    wait_count++;
    advance_resources();
}
function without_wait(operation) {
    allow_wait = false;
    try { operation(); }
    catch (error) { allow_wait = true; throw error; }
    allow_wait = true;
}

function chronos_library() { return catalogue; }
function chronos_native_resource_path(path) { return path; }
function alive_count(kind) {
    local count = 0;
    foreach (resource in resources) if (resource.alive && resource.kind == kind) count += resource.paths.len();
    return count;
}
function assert_bounds() {
    check(alive_count("cover") <= 8, "more than seven covers plus one pending preparation");
    check(alive_count("thumb") <= 50, "more than fifty thumbnail resources");
    check(alive_count("clear") <= 1, "more than one transparent resource");
    foreach (layer in layers) {
        check(layer.registrations.len() <= (layer.small ? 50 : 28), "registration pool grows while scrolling");
        foreach (id, record in layer.registrations)
            check(record.image.psb.owner.alive, "registered image outlives its PSB resource");
    }
}
System <- { clearResourceCache = function() { cache_clears++; assert_bounds(); } };

class Resource {
    alive = false;
    closed = false;
    loading = false;
    path = null;
    kind = null;
    psb = null;
    paths = null;
    remaining = 0;
    constructor() { resources.append(this); }
    function load(name) {
        check(!alive && !closed, "resource reused after unload");
        paths = typeof name == "array" ? clone name : [name];
        check(paths.len() > 0, "empty batch");
        path = paths[0];
        kind = path.find("chronos_transparent") != null ? "clear" :
               (path.find("thumb-") != null ? "thumb" : "cover");
        if (kind == "cover") cover_loads += paths.len();
        if (kind == "thumb") thumbnail_batches++;
        foreach (name in paths) if (name == failed_load_path) throw "simulated Resource load error";
        alive = true;
        psb = {};
        foreach (name in paths) psb[name] <- { owner = this, path = name };
        remaining = delay_frames;
        loading = remaining > 0;
        local count = alive_count(kind);
        if (count > resource_peak[kind]) resource_peak[kind] = count;
        // Loading a replacement before evicting the old slot may retain one
        // extra full PSB temporarily. The committed pool must remain seven.
        check(count <= (kind == "cover" ? 8 : (kind == "thumb" ? 50 : 1)),
              "resource preparation is not bounded");
    }
    function find(name) {
        check(alive && !loading && name in psb, "find before loading completed or on wrong resource");
        if (name == failed_find_path) return null;
        return psb[name];
    }
    function unload() {
        check(!closed, "resource unloaded twice");
        check(!loading, "resource unloaded while a native read is still pending");
        foreach (layer in layers) foreach (id, record in layer.registrations)
            check(record.image.psb.owner != this, "resource unloaded before image unregistration");
        alive = false;
        closed = true;
        psb = null;
    }
}

class Image {
    psb = null;
    label = null;
    constructor(value, name) {
        check(value.owner.alive, "Image constructed from an unloaded PSB");
        check(!value.owner.loading, "Image constructed before native IO completed");
        images_created++;
        if (value.path == failed_image_path && name == failed_image_label)
            throw "simulated Image construction error";
        psb = value;
        label = name;
    }
    function getValid() { return psb.owner.alive; }
    function getWidth() { return label == "thumb" ? 48 : (label == "left" ? 20 : (label == "right" ? 12 : 240)); }
    function getHeight() { return label == "thumb" ? 48 : 240; }
}

class Layer {
    small = false;
    registrations = null;
    keys = null;
    constructor(thumbnail) {
        small = thumbnail;
        registrations = {};
        keys = {};
        layers.append(this);
    }
    function registerMotionSourceIconImage(image, source, key, x, y) {
        check(image.getValid(), "registering an invalid image");
        check(source == "chronos_cover", "unexpected image source");
        check(!(key in keys), "image key registered twice");
        if (key == failed_registration_key) {
            failed_registration_key = null;
            throw "simulated registration error";
        }
        local id = next_id++;
        registrations[id] <- { image = image, key = key, x = x, y = y };
        keys[key] <- id;
        registered_total++;
        check(registrations.len() <= (small ? 50 : 28), "too many registered images");
        return id;
    }
    function unregisterMotionSourceIconImage(id) {
        check(id in registrations, "unknown image unregistered");
        keys.rawdelete(registrations[id].key);
        registrations.rawdelete(id);
        unregistered_total++;
    }
}

function entry(index) {
    local sleeve = index % 3 == 0;
    return { relative_path = "jp/GAME" + index.tostring(), assets = {
        schema = 1, revision = "revision" + index.tostring(),
        cover_path = "runtime/cover-" + index.tostring() + ".image.psb",
        thumb_path = "runtime/thumb-" + index.tostring() + ".image.psb",
        width = 240, height = 240, origin_x = 120.0, origin_y = 120.0,
        front_origin_x = sleeve ? 116.0 : 120.0, front_origin_y = 120.0,
        left = sleeve ? { label = "left", width = 20, height = 240, origin_x = 140.0, origin_y = 120.0 } : null,
        right = sleeve ? { label = "right", width = 12, height = 240, origin_x = -120.0, origin_y = 120.0 } : null
    } };
}
function make_view(count, lineup) {
    local view = { items = array(200, null) };
    catalogue.snapshot = { lineup = lineup, entries = [] };
    s_indextable = [];
    for (local index = 0; index < count; index++) {
        local game = entry(index);
        catalogue.snapshot.entries.append(game);
        view.items[(lineup == "jp" ? 0 : 50) + index] = { chronos_entry = game };
        s_indextable.append(index);
    }
    return view;
}
function close_and_check(pool, main, small) {
    pool.close();
    check(main.registrations.len() == 0 && small.registrations.len() == 0, "close leaves registered images");
    check(alive_count("cover") == 0 && alive_count("thumb") == 0 && alive_count("clear") == 0,
          "close leaks a PSB resource");
    check(pool.slots.len() == 0 && pool.thumbnails.len() == 0 && pool.entry_slots.len() == 0,
          "close retains a cache record");
    check(pool.config == null && pool.clear_image == null && pool.clear_psb == null && pool.clear_resource == null &&
          pool.thumbnail_resource == null && pool.pending == null && pool.animate == null,
          "close retains the transparent resource or old folder");
    pool.close();
}

function drain(pool) {
    for (local frame = 0; frame < 20; frame++) {
        advance_resources();
        without_wait(function():(pool) { pool.tick(); });
        if (pool.pending == null) return;
    }
    throw "pending preparation never completed";
}

function rotated(count, start) {
    local work = [];
    for (local index = 0; index < count; index++) work.append((start + index + count) % count);
    return work;
}
function expect_error(operation, text) {
    local failed = false;
    try { operation(); }
    catch (error) { failed = true; check(error.find(text) != null, "unexpected error: " + error); }
    check(failed, "expected failure: " + text);
}

// Sorted indexes are logical positions, not entry indexes. Every wanted card
// must retain its own images under successive sort permutations and scrolls.
foreach (lineup in ["jp", "us"]) {
    local view = make_view(50, lineup);
    local main = Layer(false), small = Layer(true);
    local pool = ChronosCoverPool(view, main, small);
    check(main.registrations.len() == 28 && small.registrations.len() == 50, "initial image pool shape");
    check(pool.entry_slots.len() == 0 && alive_count("cover") == 0, "constructor decoded unsorted covers");
    foreach (multiplier in [1, 7, 13, 49]) {
        for (local index = 0; index < 50; index++) s_indextable[index] = (index * multiplier + 17) % 50;
        for (local cursor = 0; cursor < 120; cursor++) {
            local work = [];
            for (local index = 0; index < 50; index++) work.append((cursor + index) % 50);
            pool.sync(work);
            drain(pool);
            assert_bounds();
            check(pool.entry_slots.len() == 7, "scroll keeps more than seven live entries");
            foreach (position in [0, 1, 2, 3, 4, 49, 5]) {
                local logical = work[position];
                local actual = s_indextable[logical];
                local slot = pool.slot(logical);
                check(pool.slots[slot].entry == actual, "sort/scroll displays the wrong entry");
                check(pool.slots[slot].resource.alive && pool.slots[slot].psb != null,
                      "visible image lost its resource owner");
            }
        }
    }
    // Image preparation failure occurs before old registrations are touched.
    local fresh = 0;
    while (fresh in pool.entry_slots) fresh++;
    local before = clone main.registrations;
    local old_entries = clone pool.entry_slots;
    failed_image_path = pool.asset_path(view.items[pool.offset + fresh].chronos_entry,
                                       view.items[pool.offset + fresh].chronos_entry.assets.cover_path);
    failed_image_label = "cover";
    expect_error(function():(pool, fresh) { pool.sync_entries([fresh]); }, "Image construction error");
    failed_image_path = null;
    failed_image_label = null;
    check(main.registrations.len() == before.len(), "failed Image changed registrations");
    foreach (id, record in before) check(id in main.registrations && main.registrations[id] == record,
                                       "failed Image replaced an old registration");
    foreach (index, slot in old_entries) check(pool.entry_slots[index] == slot, "failed Image changed entry mapping");
    assert_bounds();
    // A native registration failure must restore all prior keys/images and
    // discard the newly prepared resource, including partially added keys.
    local images_by_key = {};
    foreach (id, record in main.registrations) images_by_key[record.key] <- record.image;
    failed_registration_key = "slot_0_center";
    expect_error(function():(pool, fresh) { pool.sync_entries([fresh]); }, "registration error");
    check(main.registrations.len() == images_by_key.len(), "registration rollback loses an old key");
    foreach (key, image in images_by_key)
        check(key in main.keys && main.registrations[main.keys[key]].image == image,
              "registration rollback changes an old image");
    foreach (index, slot in old_entries) check(pool.entry_slots[index] == slot, "registration rollback changes entry mapping");
    assert_bounds();
    close_and_check(pool, main, small);
}

// Empty and very small folders have no hidden registrations/resources beyond
// their actual thumbnails; the transparent placeholder remains owned.
foreach (count in [0, 1, 2, 3, 4, 5, 6, 7]) {
    local view = make_view(count, "jp");
    local main = Layer(false), small = Layer(true);
    local pool = ChronosCoverPool(view, main, small);
    check(small.registrations.len() == count, "small folder thumbnail count");
    local loads_before = cover_loads;
    pool.sync(rotated(count, 0));
    for (local step = 0; step < 20; step++) pool.sync(rotated(count, count > 0 ? step % count : 0));
    check(cover_loads - loads_before == count, "small folder reloads already retained images");
    check(pool.entry_slots.len() == (count < 7 ? count : 7), "small folder full-cover count");
    assert_bounds();
    close_and_check(pool, main, small);
}

// The first request already has the final sorting permutation and cursor.
// No physical-index prime or intermediate sort may decode extra full covers.
{
    local view = make_view(11, "jp");
    for (local index = 0; index < 11; index++) s_indextable[index] = (index * 7 + 3) % 11;
    local main = Layer(false), small = Layer(true);
    local before_loads = cover_loads, before_batches = thumbnail_batches;
    local before_clears = cache_clears, before_waits = wait_count;
    local animated = { frames = 0 };
    delay_frames = 2;
    local pool = ChronosCoverPool(view, main, small, function():(animated) { animated.frames++; });
    check(cover_loads == before_loads, "constructor still primes physical indexes");
    check(thumbnail_batches == before_batches + 1, "thumbnails are not one native batch");
    pool.sync(rotated(11, 8));
    check(cover_loads - before_loads == 7, "sorted cursor initial load is not exactly seven covers");
    check(cache_clears == before_clears, "first prime clears a cache with no retired full images");
    check(animated.frames == wait_count - before_waits && animated.frames > 0,
          "blocking IO did not animate once for every yielded frame");
    close_and_check(pool, main, small);
    delay_frames = 0;
}

// With seven prepared entries, a one-step move already has all five visible
// images. Its missing far neighbor must never block the menu frame.
{
    local view = make_view(11, "jp");
    local main = Layer(false), small = Layer(true);
    local animated = { frames = 0 };
    local pool = ChronosCoverPool(view, main, small, function():(animated) { animated.frames++; });
    pool.sync(rotated(11, 0));
    local baseline = clone pool.entry_slots;
    local before_loads = cover_loads, before_images = images_created;
    delay_frames = 3;
    without_wait(function():(pool) { pool.sync(rotated(11, 1)); });
    check(pool.pending != null && pool.pending.entry == 6, "wrong next neighbor");
    check(cover_loads == before_loads + 1 && alive_count("cover") == 8, "pending preparation is not bounded to one");
    foreach (logical in [1, 2, 3, 4, 5]) check(pool.slots[pool.slot(logical)].entry == logical, "visible image not ready");
    without_wait(function():(pool) { pool.tick(); });
    check(pool.pending.resource.loading, "tick should poll, never wait");
    // Reverse before completion: discard the stale decoded PSB without Image
    // creation or replacing a currently useful slot.
    without_wait(function():(pool) { pool.sync(rotated(11, 0)); });
    drain(pool);
    check(images_created == before_images, "stale neighbor was uploaded despite reversal");
    foreach (entry, slot in baseline) check(pool.entry_slots[entry] == slot, "reversal evicted a useful image");
    check(pool.entry_slots.len() == 7 && alive_count("cover") == 7, "reversal retained stale payloads");
    // A faster second move overtakes IO. Only this genuinely visible demand
    // waits; the same pending request is promoted instead of loaded twice.
    without_wait(function():(pool) { pool.sync(rotated(11, 1)); });
    local prior = cover_loads, waited = wait_count, animation = animated.frames;
    pool.sync(rotated(11, 2));
    check(pool.slots[pool.slot(6)].entry == 6, "overtaken neighbor was not made visible");
    check(cover_loads == prior + 1, "visible pending image was loaded twice"); // only new neighbor7
    check(wait_count > waited && animated.frames - animation == wait_count - waited,
          "visible demand stopped folder animation while waiting");
    expect_error(function():(pool) { pool.slot(9); }, "visible cover is not ready");
    check(pool.slot(9, false) == 0, "hidden motion fallback changed");
    close_and_check(pool, main, small); // Drain an outstanding native read.
    delay_frames = 0;
}

// A failed speculative read/Image/registration is reported once, retains the
// old pool, and is retried only by an explicit demand for that visible image.
foreach (phase in ["load", "find", "image", "registration"]) {
    local view = make_view(11, "jp");
    local main = Layer(false), small = Layer(true);
    local pool = ChronosCoverPool(view, main, small);
    pool.sync(rotated(11, 0));
    local old_entries = clone pool.entry_slots;
    local old_images = {};
    foreach (id, record in main.registrations) old_images[record.key] <- record.image;
    local target = pool.asset_path(view.items[6].chronos_entry, view.items[6].chronos_entry.assets.cover_path);
    if (phase == "load") failed_load_path = target;
    if (phase == "find") failed_find_path = target;
    if (phase == "image") { failed_image_path = target; failed_image_label = "cover"; }
    if (phase == "registration") failed_registration_key = "slot_5_center";
    local reports = prefetch_reports, loads = cover_loads;
    without_wait(function():(pool) { pool.sync(rotated(11, 1)); });
    drain(pool);
    for (local frame = 0; frame < 20; frame++) without_wait(function():(pool) { pool.tick(); });
    check(prefetch_reports == reports + 1 && cover_loads == loads + 1, "failed prefetch loops every frame");
    foreach (entry, slot in old_entries) check(pool.entry_slots[entry] == slot, "failed prefetch evicted an entry");
    foreach (key, image in old_images)
        check(key in main.keys && main.registrations[main.keys[key]].image == image, "failed prefetch damaged a live key");
    failed_load_path = null; failed_find_path = null; failed_image_path = null; failed_image_label = null;
    pool.sync(rotated(11, 2));
    check(pool.slots[pool.slot(6)].entry == 6, "explicit visible retry failed");
    assert_bounds();
    close_and_check(pool, main, small);
}

// Constructor failures also have to close resources not yet appended to a
// retained record; a failed final thumbnail is especially easy to miss.
foreach (phase in ["clear", "thumbnail_psb", "thumbnail_image", "thumbnail_registration"]) {
    local view = make_view(50, "jp");
    local main = Layer(false), small = Layer(true);
    if (phase == "clear") {
        failed_image_path = "system/image/chronos_transparent.image.psb";
        failed_image_label = "clear";
    } else if (phase == "thumbnail_psb") {
        failed_find_path = "/mnt/usb/library/jp/GAME49/runtime/thumb-49.image.psb";
    } else if (phase == "thumbnail_image") {
        failed_image_path = "/mnt/usb/library/jp/GAME49/runtime/thumb-49.image.psb";
        failed_image_label = "thumb";
    } else {
        failed_registration_key = "thumb_49";
    }
    expect_error(function():(view, main, small) { ChronosCoverPool(view, main, small); },
                 phase == "thumbnail_registration" ? "registration error" :
                 (phase == "thumbnail_psb" ? "cannot load thumbnail" : "Image construction error"));
    failed_find_path = null;
    failed_image_path = null;
    failed_image_label = null;
    failed_registration_key = null;
    check(main.registrations.len() == 0 && small.registrations.len() == 0,
          "failed constructor leaves registered images");
    check(alive_count("cover") == 0 && alive_count("thumb") == 0 && alive_count("clear") == 0,
          "failed constructor leaks a PSB resource: " + phase);
}

check(resource_peak.cover == 8 && resource_peak.thumb == 50 && resource_peak.clear == 1,
      "expected bounded resource maxima were not exercised");
check(registered_total == unregistered_total, "not every image was unregistered");
check(cache_clears > 100, "scroll resource-cache release was not exercised");
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checker", type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="chronos-covers-check-") as temporary:
        script = Path(temporary) / "covers.nut"
        script.write_bytes((ROOT / "scripts-src/chronos_covers.nut").read_bytes() +
                           b"\n" + CHECK.encode())
        subprocess.run([str(args.checker.resolve()), "-r", str(script)], check=True)
    print("PASS: single sorted prime, batched thumbnails, animated waits, asynchronous neighbors/reversal/retry, bounded JP/US pools and failure cleanup")


if __name__ == "__main__":
    main()
