// Ready Image PSBs only: desktop preparation handles PNG/PSB work. Seven large
// slots cover the five visible cards plus one neighbor in either direction;
// small square thumbnails belong only to the current folder.
class ChronosCoverPool
{
    config = null;
    main_layer = null;
    thumb_layer = null;
    slots = null;
    thumbnails = null;
    thumbnail_resource = null;
    clear_image = null;
    clear_resource = null;
    clear_psb = null;
    entry_slots = null;
    offset = 0;
    count = 0;
    animate = null;
    initialized = false;
    stopped = false;
    pending = null;
    visible_entries = null;
    wanted_entries = null;
    prefetch_errors = null;

    constructor(view, layer, small_layer, animate_wait = null)
    {
        config = view;
        main_layer = layer;
        thumb_layer = small_layer;
        offset = ::chronos_library().snapshot.lineup == "jp" ? 0 : 50;
        count = ::chronos_library().snapshot.entries.len();
        animate = animate_wait;
        slots = [];
        thumbnails = [];
        entry_slots = {};
        visible_entries = [];
        wanted_entries = [];
        prefetch_errors = {};
        try {
            local clear = load_psb("system/image/chronos_transparent.image.psb");
            clear_resource = clear.resource;
            clear_psb = clear.psb;
            clear_image = Image(clear.psb, "clear");
            if (!clear_image.getValid()) throw "Chronos: invalid transparent image";
            for (local i = 0; i < 7; i++) {
                local record = { entry = -1, revision = "", images = null, info = null, ids = [], resource = null, psb = null };
                record.ids = register_slot(i, null, null);
                slots.append(record);
            }
            local paths = [];
            for (local index = 0; index < count; index++) {
                local entry = config.items[offset + index].chronos_entry;
                local descriptor = entry.assets;
                if (descriptor == null || descriptor.schema != 1) throw "Chronos: cover is not prepared";
                paths.append(::chronos_native_resource_path(asset_path(entry, descriptor.thumb_path)));
            }
            // All thumbnails have the same folder lifetime. One native batch
            // avoids a separate loading barrier and Resource wrapper per file.
            if (paths.len() > 0) {
                thumbnail_resource = Resource();
                thumbnail_resource.load(paths);
                wait_ready(thumbnail_resource);
            }
            for (local index = 0; index < count; index++) {
                local object = thumbnail_resource.find(paths[index]);
                if (object == null) throw "Chronos: cannot load thumbnail " + paths[index];
                local record = { image = null, id = null, psb = object };
                thumbnails.append(record); // Cleanup owns it before any native operation.
                record.image = Image(object, "thumb");
                if (!record.image.getValid() || record.image.getWidth() != 48 || record.image.getHeight() != 48)
                    throw "Chronos: invalid thumbnail";
                record.id = thumb_layer.registerMotionSourceIconImage(
                    record.image, "chronos_cover", "thumb_" + index.tostring(), 24.0, 24.0);
            }
            // Placeholder keys make motion construction safe. The caller's
            // final sorted cursor primes the seven actual images exactly once.
        } catch (error) {
            close();
            throw error;
        }
    }

    function wait_ready(resource)
    {
        while (resource.loading) {
            if (animate != null) animate();
            wait(0);
        }
    }

    function load_psb(path)
    {
        path = ::chronos_native_resource_path(path);
        local resource = Resource();
        try {
            resource.load(path);
            wait_ready(resource);
            local object = resource.find(path);
            if (object == null) throw "Chronos: cannot load image " + path;
            return { resource = resource, psb = object };
        } catch (error) {
            resource.unload();
            throw error;
        }
    }

    function asset_path(entry, leaf)
    {
        return "/mnt/usb/library/" + entry.relative_path + "/" + leaf;
    }

    function register_slot(slot_index, images, info)
    {
        local prefix = "slot_" + slot_index.tostring() + "_";
        local ids = [];
        try {
            local center = images == null ? clear_image : images.cover;
            ids.append(main_layer.registerMotionSourceIconImage(center, "chronos_cover", prefix + "cover",
                info == null ? 0.0 : info.front_origin_x, info == null ? 0.0 : info.front_origin_y));
            ids.append(main_layer.registerMotionSourceIconImage(center, "chronos_cover", prefix + "center",
                info == null ? 0.0 : info.origin_x, info == null ? 0.0 : info.origin_y));
            foreach (side in ["left", "right"]) {
                local geometry = info == null ? null : info[side];
                local image = images == null || geometry == null ? clear_image : images[side];
                ids.append(main_layer.registerMotionSourceIconImage(image, "chronos_cover", prefix + side,
                    geometry == null ? 0.0 : geometry.origin_x,
                    geometry == null ? 0.0 : geometry.origin_y));
            }
            return ids;
        } catch (error) {
            foreach (id in ids) main_layer.unregisterMotionSourceIconImage(id);
            throw error;
        }
    }

    function begin_prepare(entry_index)
    {
        local entry = config.items[offset + entry_index].chronos_entry;
        local info = entry.assets;
        local resource = Resource();
        local path = ::chronos_native_resource_path(asset_path(entry, info.cover_path));
        try {
            resource.load(path);
            return { entry = entry_index, revision = info.revision, images = null, info = info,
                     ids = [], resource = resource, psb = null, path = path };
        } catch (error) {
            resource.unload();
            throw error;
        }
    }

    function finish_prepare(record)
    {
        try {
            local object = record.resource.find(record.path);
            if (object == null) throw "Chronos: cannot load image " + record.path;
            record.psb = object;
            local info = record.info;
            local images = { cover = Image(object, "cover"), left = null, right = null };
            record.images = images;
            if (!images.cover.getValid() || images.cover.getWidth() != info.width || images.cover.getHeight() != info.height)
                throw "Chronos: invalid cover image";
            foreach (side in ["left", "right"]) {
                if (info[side] == null) continue;
                images[side] = Image(object, info[side].label);
                if (!images[side].getValid() || images[side].getWidth() != info[side].width ||
                    images[side].getHeight() != info[side].height)
                    throw "Chronos: invalid sleeve image";
            }
            return record;
        } catch (error) {
            release_record(record);
            throw error;
        }
    }

    function contains(entries, entry)
    {
        foreach (value in entries) if (value == entry) return true;
        return false;
    }

    function release_record(record)
    {
        record.images = null;
        record.psb = null;
        if (record.resource != null) record.resource.unload();
        record.resource = null;
    }

    function install_record(next, keep)
    {
        local available = -1;
        foreach (index, record in slots) {
            if (!contains(keep, record.entry)) { available = index; break; }
        }
        if (available < 0) {
            release_record(next);
            throw "Chronos: cover pool exhausted";
        }
        local old = slots[available];
        local retired = old.resource != null;
        foreach (id in old.ids) main_layer.unregisterMotionSourceIconImage(id);
        try { next.ids = register_slot(available, next.images, next.info); }
        catch (error) {
            release_record(next);
            old.ids = register_slot(available, old.images, old.info);
            throw error;
        }
        if (old.entry in entry_slots) entry_slots.rawdelete(old.entry);
        release_record(old);
        slots[available] = next;
        entry_slots[next.entry] <- available;
        // Unloaded PSBs otherwise remain in the native idle cache. Active
        // images stay owned; reclaim retired entries before another prepare.
        if (retired) System.clearResourceCache();
    }

    function discard_pending()
    {
        if (pending == null) return;
        wait_ready(pending.resource);
        local record = pending;
        pending = null;
        release_record(record);
        System.clearResourceCache();
    }

    function finish_pending(block = false)
    {
        if (pending == null) return;
        if (block) wait_ready(pending.resource);
        if (pending.resource.loading) return;
        local record = pending;
        pending = null;
        if (!contains(wanted_entries, record.entry) || record.entry in entry_slots) {
            release_record(record);
            System.clearResourceCache();
            return;
        }
        try {
            finish_prepare(record);
            install_record(record, wanted_entries);
        } catch (error) {
            release_record(record);
            prefetch_errors[record.entry] <- error;
            System.clearResourceCache();
            // An offscreen failure must not interrupt an otherwise valid menu.
            // Becoming visible requires this exact image, never slot zero.
            printf("[CHRONOS-COVER] prefetch rejected: %s\n", error.tostring());
        }
    }

    function start_neighbor()
    {
        if (stopped || !initialized || pending != null) return;
        foreach (entry_index in wanted_entries) {
            if (entry_index in entry_slots || entry_index in prefetch_errors) continue;
            try { pending = begin_prepare(entry_index); }
            catch (error) {
                prefetch_errors[entry_index] <- error;
                printf("[CHRONOS-COVER] prefetch rejected: %s\n", error.tostring());
            }
            // At most one preparation is started in a frame, including errors.
            return;
        }
    }

    // Poll from the normal menu frame. Native IO can proceed while the five
    // already prepared visible cards and the stock animations keep moving.
    function tick()
    {
        if (stopped) return;
        finish_pending();
        start_neighbor();
    }

    function sync(work)
    {
        local wanted = [];
        for (local i = 0; i < work.len() && i < 5; i++) wanted.append(s_indextable[work[i]]);
        visible_entries = clone wanted;
        if (work.len() > 5) wanted.append(s_indextable[work[work.len() - 1]]);
        if (work.len() > 6) wanted.append(s_indextable[work[5]]);
        wanted_entries = wanted;
        if (!initialized) {
            sync_entries(wanted);
            initialized = true;
            printf("[CHRONOS-COVER] ready: %d full slots, %d thumbnails\n", entry_slots.len(), thumbnails.len());
            return;
        }
        finish_pending();
        foreach (entry_index in visible_entries) {
            if (entry_index in entry_slots) continue;
            // A jump/sort may overtake the one outstanding read. Drain it
            // before starting another so preparation never exceeds one image.
            finish_pending(true);
            if (entry_index in entry_slots) continue;
            // An explicit visible demand may retry a transient prefetch error.
            // Passive ticks never retry failed neighbors on every frame.
            if (entry_index in prefetch_errors) prefetch_errors.rawdelete(entry_index);
            local next = begin_prepare(entry_index);
            try {
                wait_ready(next.resource);
                finish_prepare(next);
                install_record(next, wanted_entries);
            } catch (error) {
                release_record(next);
                prefetch_errors[entry_index] <- error;
                throw error;
            }
        }
        start_neighbor();
    }

    function sync_entries(wanted)
    {
        discard_pending();
        foreach (entry_index in wanted) {
            if (entry_index in entry_slots) continue;
            // Decode/upload first. A failure preserves the old live key/image.
            local next = begin_prepare(entry_index);
            try {
                wait_ready(next.resource);
                finish_prepare(next);
                install_record(next, wanted);
            } catch (error) {
                release_record(next);
                throw error;
            }
        }
    }

    function slot(logical_index, required = true)
    {
        local entry = s_indextable[logical_index];
        if (entry in entry_slots) return entry_slots[entry];
        if (required) throw "Chronos: visible cover is not ready";
        return 0;
    }

    function close()
    {
        stopped = true;
        discard_pending();
        if (slots != null) foreach (record in slots) {
            foreach (id in record.ids) main_layer.unregisterMotionSourceIconImage(id);
            release_record(record);
        }
        if (thumbnails != null) foreach (record in thumbnails) {
            if (record.id != null) thumb_layer.unregisterMotionSourceIconImage(record.id);
            record.image = null;
            record.psb = null;
        }
        if (thumbnail_resource != null) {
            wait_ready(thumbnail_resource);
            thumbnail_resource.unload();
        }
        thumbnail_resource = null;
        slots = [];
        thumbnails = [];
        entry_slots = {};
        visible_entries = [];
        wanted_entries = [];
        prefetch_errors = {};
        clear_image = null;
        clear_psb = null;
        if (clear_resource != null) clear_resource.unload();
        clear_resource = null;
        config = null;
        animate = null;
        System.clearResourceCache();
    }
}
