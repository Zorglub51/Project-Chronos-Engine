/* Native catalogue closures. IO/JSON work runs in one transient worker; the
 * staged DOM is converted and released only by takeFolder on the VM thread.
 * Stock link-time elimination removed sq_newarray, so JSON arrays cross this
 * ABI as integer-key tables with __chronos_array=<length>. ChronosLibrary
 * converts these to ordinary Squirrel arrays before exposing the snapshot. */
#ifndef CHRONOS_CATALOG_RUNTIME_H
#define CHRONOS_CATALOG_RUNTIME_H
#include "catalog_library.h"
#include "folder_worker.h"
#include "save_files.h"

typedef void (*catalog_sq_newtable_t)(HSQUIRRELVM);
typedef void (*catalog_sq_integer_t)(HSQUIRRELVM, SQInteger);
typedef void (*catalog_sq_float_t)(HSQUIRRELVM, float);
typedef void (*catalog_sq_bool_t)(HSQUIRRELVM, SQBool);
typedef void (*catalog_sq_null_t)(HSQUIRRELVM);
typedef SQInteger (*catalog_sq_gettop_t)(HSQUIRRELVM);
typedef void (*catalog_sq_settop_t)(HSQUIRRELVM, SQInteger);
static catalog_sq_newtable_t c_sq_newtable = (catalog_sq_newtable_t)(0x8f518 | 1);
static catalog_sq_integer_t c_sq_integer = (catalog_sq_integer_t)(0x8f398 | 1);
static catalog_sq_float_t c_sq_float = (catalog_sq_float_t)(0x8f43c | 1);
static catalog_sq_bool_t c_sq_bool = (catalog_sq_bool_t)(0x8f400 | 1);
static catalog_sq_null_t c_sq_null = (catalog_sq_null_t)(0x8f334 | 1);
static catalog_sq_gettop_t c_sq_gettop = (catalog_sq_gettop_t)(0x900d4 | 1);
static catalog_sq_settop_t c_sq_settop = (catalog_sq_settop_t)(0x900dc | 1);
static int g_catalog_supported;
static struct folder_worker g_catalog_worker = FOLDER_WORKER_INIT;
static struct catalog_snapshot *g_catalog_stage;
static unsigned g_catalog_ticket, g_catalog_taken_ticket;
static int g_catalog_state; /* 0=no request,1=busy,2=ready,3=failed,4=taken */
static char g_catalog_error[224];
static struct { dev_t device; ino_t inode; char lineup[3], folder[CATALOG_FOLDER_SIZE]; } g_catalog_taken;

/* Check the exact executable API instructions before invoking new ABI calls.
 * sq_pushfloat is ARM hard-float (s0); SQInteger/SQBool are signed32. */
static int catalog_verify_api(void)
{
    static const struct { uintptr_t address; unsigned char bytes[8]; } api[] = {
        {0x8f518, {0x70,0xb5,0x05,0x46,0x82,0xb0,0x2c,0x20}},
        {0x8f398, {0x00,0xb5,0x83,0xb0,0x02,0x23,0xc0,0xf2}},
        {0x8f400, {0x0a,0x1c,0x4f,0xf0,0x08,0x03,0x00,0xb5}},
        {0x8f43c, {0x00,0xb5,0x04,0x23,0xc0,0xf2,0x00,0x53}},
        {0x8f334, {0x4a,0xf2,0xe0,0x71,0xc0,0xf2,0x3e,0x11}},
        {0x900d4, {0x02,0x6b,0x40,0x6b,0x10,0x1a,0x70,0x47}},
        {0x900dc, {0xf8,0xb5,0x0b,0x46,0x05,0x6b,0x04,0x46}},
    };
    for (unsigned i = 0; i < sizeof(api) / sizeof(api[0]); ++i)
        if (memcmp((const void *)api[i].address, api[i].bytes, 8)) return 0;
    return 1;
}

static SQInteger catalog_status(HSQUIRRELVM v, const char *status)
{ p_sq_pushstring(v, status, -1); return 1; }

static SQInteger catalog_native_error(HSQUIRRELVM v, const char *message)
{
    char text[256]; snprintf(text, sizeof(text), "error:%s", message);
    return catalog_status(v, text);
}

static int catalog_emit_json(HSQUIRRELVM v, const struct catalog_json *dom, uint32_t id)
{
    if (!id) { c_sq_null(v); return 0; }
    const struct catalog_json_node *n = &dom->nodes[id];
    switch (n->type) {
    case CJ_NULL: c_sq_null(v); return 0;
    case CJ_BOOL: c_sq_bool(v, n->value.integer); return 0;
    case CJ_INT: c_sq_integer(v, n->value.integer); return 0;
    case CJ_FLOAT: c_sq_float(v, n->value.number); return 0;
    case CJ_STRING: p_sq_pushstring(v, dom->strings + n->value.string, -1); return 0;
    case CJ_ARRAY: case CJ_OBJECT:
        c_sq_newtable(v);
        if (n->type == CJ_ARRAY) {
            p_sq_pushstring(v, "__chronos_array", -1); c_sq_integer(v, n->count);
            if (p_sq_newslot(v, -3, 0) < 0) return -1;
        }
        unsigned index = 0;
        for (uint32_t child = n->child; child;) {
            uint32_t value = child;
            if (n->type == CJ_OBJECT) {
                p_sq_pushstring(v, catalog_json_string(dom, child), -1);
                value = dom->nodes[child].next;
            } else c_sq_integer(v, (SQInteger)index++);
            if (catalog_emit_json(v, dom, value) || p_sq_newslot(v, -3, 0) < 0) return -1;
            child = dom->nodes[value].next;
        }
        return 0;
    default: return -1;
    }
}

static int catalog_emit_text(HSQUIRRELVM v, const char *key, const char *value)
{
    p_sq_pushstring(v, key, -1); p_sq_pushstring(v, value ? value : "", -1);
    return p_sq_newslot(v, -3, 0) < 0 ? -1 : 0;
}
static int catalog_emit_number(HSQUIRRELVM v, const char *key, unsigned value)
{
    p_sq_pushstring(v, key, -1); c_sq_integer(v, (SQInteger)value);
    return p_sq_newslot(v, -3, 0) < 0 ? -1 : 0;
}
static int catalog_emit_member(HSQUIRRELVM v, const char *key, const struct catalog_json *dom, uint32_t id)
{
    p_sq_pushstring(v, key, -1);
    return catalog_emit_json(v, dom, id) || p_sq_newslot(v, -3, 0) < 0 ? -1 : 0;
}

static int catalog_emit_snapshot(HSQUIRRELVM v, const struct catalog_snapshot *s)
{
    c_sq_newtable(v);
    if (catalog_emit_number(v, "schema", 2) || catalog_emit_number(v, "ticket", g_catalog_ticket) ||
        catalog_emit_text(v, "lineup", s->lineup) || catalog_emit_text(v, "folder", s->folder) ||
        catalog_emit_text(v, "path", s->path) || catalog_emit_number(v, "entry_count", s->count) ||
        catalog_emit_number(v, "playable_count", s->playable_count) ||
        catalog_emit_member(v, "settings", &s->json, s->settings) ||
        catalog_emit_member(v, "folder_metadata", &s->json, s->folder_metadata)) return -1;
    p_sq_pushstring(v, "entries", -1); c_sq_newtable(v);
    if (catalog_emit_number(v, "__chronos_array", s->count)) return -1;
    for (unsigned i = 0; i < s->count; ++i) {
        const struct catalog_entry *e = &s->entries[i];
        c_sq_integer(v, (SQInteger)i); c_sq_newtable(v);
        if (catalog_emit_text(v, "role", e->role == CATALOG_GAME ? "game" : e->role == CATALOG_FOLDER ? "folder" : "back") ||
            catalog_emit_number(v, "menu_index", i) || catalog_emit_number(v, "image_index", i)) return -1;
        const struct { const char *key; uint32_t node; } fields[] = {
            {"folder",e->name}, {"game_id",e->game_id}, {"save_path",e->save_path},
            {"native_region_tag",e->native_tag}, {"cover_path",e->cover_path}, {"rom_path",e->rom_path},
            {"relative_path",e->relative_path}, {"game",e->game}, {"sort",e->sort},
            {"folder_metadata",e->folder_metadata}, {"assets",e->assets},
        };
        for (unsigned j = 0; j < sizeof(fields) / sizeof(fields[0]); ++j)
            if (catalog_emit_member(v, fields[j].key, &s->json, fields[j].node)) return -1;
        if (p_sq_newslot(v, -3, 0) < 0) return -1;
    }
    return p_sq_newslot(v, -3, 0) < 0 ? -1 : 0;
}

static int catalog_worker_read(const char *lineup, const char *folder)
{
    g_catalog_stage = catalog_read_folder(SAVE_LIBRARY_ROOT, lineup, folder);
    return g_catalog_stage && !g_catalog_stage->error[0] ? 0 : -1;
}

static int catalog_ticket_matches(const char *ticket)
{ unsigned value; return !save_route_decimal(ticket, INT32_MAX, &value) && value && value == g_catalog_ticket; }

static void catalog_poll_worker(void)
{
    if (g_catalog_state != 1) return;
    int result = folder_worker_poll(&g_catalog_worker);
    if (result == 1) return;
    if (!result && !catalog_snapshot_current(SAVE_LIBRARY_ROOT, g_catalog_stage)) g_catalog_state = 2;
    else {
        snprintf(g_catalog_error, sizeof(g_catalog_error), "%s", g_catalog_stage && g_catalog_stage->error[0]
                 ? g_catalog_stage->error : "Library changed/disconnected or catalogue allocation failed");
        catalog_snapshot_free(g_catalog_stage); g_catalog_stage = NULL; g_catalog_state = 3;
    }
}

static SQInteger native_catalog_enabled(HSQUIRRELVM v)
{
    if (g_catalog_boot_mode == -2) g_catalog_boot_mode = catalog_enabled(SAVE_LIBRARY_ROOT);
    int result = g_catalog_boot_mode;
    if (!result) return catalog_status(v, "legacy");
    if (!g_catalog_supported) return catalog_native_error(v, "unsupported native Squirrel catalogue ABI");
    if (result < 0) return catalog_native_error(v, "invalid or unreadable V2 library marker");
    return catalog_status(v, "v2");
}

static SQInteger native_catalog_begin(HSQUIRRELVM v)
{
    const char *lineup = vm_get_string(v, 2), *folder = vm_get_string(v, 3);
    if (!g_catalog_supported) return catalog_native_error(v, "unsupported native Squirrel catalogue ABI");
    if (!catalog_request(lineup, folder)) return catalog_native_error(v, "invalid lineup/folder path");
    if (catalog_enabled(SAVE_LIBRARY_ROOT) != 1) return catalog_native_error(v, "V2 library marker missing or invalid");
    catalog_poll_worker();
    if (g_catalog_state == 1) return catalog_native_error(v, "catalogue worker busy");
    catalog_snapshot_free(g_catalog_stage); g_catalog_stage = NULL;
    g_catalog_taken_ticket = 0; g_catalog_state = 0; g_catalog_error[0] = 0;
    int error = folder_worker_begin(&g_catalog_worker, lineup, folder, catalog_worker_read);
    if (error) return catalog_native_error(v, "cannot start catalogue worker");
    g_catalog_ticket = g_catalog_ticket == INT32_MAX ? 1 : g_catalog_ticket + 1;
    g_catalog_state = 1;
    char ticket[16]; snprintf(ticket, sizeof(ticket), "%u", g_catalog_ticket);
    return catalog_status(v, ticket);
}

static SQInteger native_catalog_poll(HSQUIRRELVM v)
{
    if (!catalog_ticket_matches(vm_get_string(v, 2))) return catalog_status(v, "stale");
    catalog_poll_worker();
    if (g_catalog_state == 3) return catalog_native_error(v, g_catalog_error);
    return catalog_status(v, g_catalog_state == 1 ? "busy" : g_catalog_state == 2 ? "ready" : "stale");
}

static SQInteger native_catalog_take(HSQUIRRELVM v)
{
    if (!catalog_ticket_matches(vm_get_string(v, 2))) return catalog_status(v, "stale");
    catalog_poll_worker();
    if (g_catalog_state == 1) return catalog_status(v, "busy");
    if (g_catalog_state == 3) return catalog_native_error(v, g_catalog_error);
    if (g_catalog_state != 2 || !g_catalog_stage) return catalog_status(v, "stale");
    if (catalog_snapshot_current(SAVE_LIBRARY_ROOT, g_catalog_stage)) {
        catalog_snapshot_free(g_catalog_stage); g_catalog_stage = NULL; g_catalog_state = 3;
        snprintf(g_catalog_error, sizeof(g_catalog_error), "Library changed before snapshot commit");
        return catalog_native_error(v, g_catalog_error);
    }
    SQInteger top = c_sq_gettop(v);
    if (catalog_emit_snapshot(v, g_catalog_stage)) {
        c_sq_settop(v, top); return catalog_native_error(v, "Squirrel snapshot construction failed; previous folder retained");
    }
    g_catalog_taken.device = g_catalog_stage->device; g_catalog_taken.inode = g_catalog_stage->inode;
    strcpy(g_catalog_taken.lineup, g_catalog_stage->lineup); strcpy(g_catalog_taken.folder, g_catalog_stage->folder);
    g_catalog_taken_ticket = g_catalog_ticket;
    catalog_snapshot_free(g_catalog_stage); g_catalog_stage = NULL; g_catalog_state = 4;
    return 1;
}

static SQInteger native_catalog_current(HSQUIRRELVM v)
{
    int directory = open(SAVE_LIBRARY_ROOT, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    if (directory < 0) return catalog_status(v, "jp/_root");
    char *bytes = NULL; size_t size = 0;
    int result = catalog_read_file(directory, ".current-v2", &bytes, &size, 1); close(directory);
    if (result == 1) return catalog_status(v, "jp/_root");
    if (result) return catalog_native_error(v, "cannot read remembered folder");
    char value[CATALOG_FOLDER_SIZE + 4];
    if (size < 5 || size >= sizeof(value) || bytes[size - 1] != '\n') {
        free(bytes); return catalog_native_error(v, "invalid remembered folder");
    }
    memcpy(value, bytes, size - 1); value[size - 1] = 0; free(bytes);
    if (strlen(value) != size - 1) return catalog_native_error(v, "invalid remembered folder");
    char *slash = strchr(value, '/');
    if (!slash) return catalog_native_error(v, "invalid remembered folder");
    *slash = 0;
    if (!catalog_request(value, slash + 1)) return catalog_native_error(v, "invalid remembered folder");
    *slash = '/'; return catalog_status(v, value);
}

static SQInteger native_catalog_remember(HSQUIRRELVM v)
{
    if (!catalog_ticket_matches(vm_get_string(v, 2)) || g_catalog_taken_ticket != g_catalog_ticket)
        return catalog_status(v, "stale");
    int directory = open(SAVE_LIBRARY_ROOT, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    struct stat st;
    if (directory < 0) return catalog_native_error(v, "cannot persist folder: library disconnected");
    if (fstat(directory, &st) || st.st_dev != g_catalog_taken.device || st.st_ino != g_catalog_taken.inode ||
        catalog_enabled(SAVE_LIBRARY_ROOT) != 1) {
        close(directory); return catalog_native_error(v, "cannot persist folder: library changed");
    }
    char value[CATALOG_FOLDER_SIZE + 5];
    int size = snprintf(value, sizeof(value), "%s/%s\n", g_catalog_taken.lineup, g_catalog_taken.folder);
    int result = save_files_atomic(directory, ".current-v2", value, (size_t)size), error = errno;
    close(directory); errno = error;
    return result ? catalog_native_error(v, "cannot persist folder; previous selection retained") : catalog_status(v, "ok");
}
#endif
