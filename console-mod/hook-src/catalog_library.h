/* One folder, read-only filesystem catalogue. No ROM/image decoding, directory
 * enumeration ordering or whole-library cache. All paths use directory FDs. */
#ifndef CHRONOS_CATALOG_LIBRARY_H
#define CHRONOS_CATALOG_LIBRARY_H
#include <stdio.h>
#include <strings.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>
#include "catalog_json.h"
#include "save_route.h"

#define CATALOG_MAX_ENTRIES 50u
#define CATALOG_FOLDER_SIZE 64u
enum catalog_role { CATALOG_GAME, CATALOG_FOLDER, CATALOG_BACK };
struct catalog_entry {
    uint32_t name, game, sort, folder_metadata, assets;
    uint32_t game_id, save_path, native_tag, cover_path, rom_path, relative_path;
    unsigned char role;
};
struct catalog_snapshot {
    struct catalog_json json;
    struct catalog_entry entries[CATALOG_MAX_ENTRIES];
    unsigned count, playable_count;
    uint32_t settings, folder_metadata;
    dev_t device; ino_t inode;
    char lineup[3], folder[CATALOG_FOLDER_SIZE], path[SAVE_ROUTE_PATH_SIZE];
    char error[192];
};

static int catalog_component(const char *name, size_t maximum)
{
    if (!name || !*name || strlen(name) > maximum || !strcmp(name, ".") || !strcmp(name, "..")) return 0;
    for (; *name; ++name)
        if ((unsigned char)*name < 32 || *name == '/' || *name == '\\' || *name == ':') return 0;
    return 1;
}

static int catalog_request(const char *lineup, const char *folder)
{
    return lineup && (!strcmp(lineup, "jp") || !strcmp(lineup, "us")) &&
           catalog_component(folder, CATALOG_FOLDER_SIZE - 1) &&
           (!strcmp(folder, "_root") || !strncmp(folder, "FOLDER", 6));
}

static int catalog_directory(int parent, const char *name)
{ return openat(parent, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC); }

static int catalog_error(struct catalog_snapshot *s, const char *message, const char *name)
{
    int error = errno ? errno : EINVAL;
    snprintf(s->error, sizeof(s->error), "%s%s%s (%s)", message, name ? ": " : "", name ? name : "", strerror(error));
    errno = error; return -1;
}

static int catalog_read_file(int directory, const char *name, char **out, size_t *length, int optional)
{
    int fd = openat(directory, name, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) return optional && errno == ENOENT ? 1 : -1;
    struct stat before, after; char *bytes = NULL; int result = -1;
    if (fstat(fd, &before)) goto done;
    if (!S_ISREG(before.st_mode) || before.st_size < 0 || (uint64_t)before.st_size > CATALOG_JSON_FILE_LIMIT) {
        errno = E2BIG; goto done;
    }
    size_t size = (size_t)before.st_size, at = 0;
    bytes = malloc(size + 1);
    if (!bytes) goto done;
    while (at < size) {
        ssize_t got = read(fd, bytes + at, size - at);
        if (got < 0 && errno == EINTR) continue;
        if (got <= 0) { if (!got) errno = ESTALE; goto done; }
        at += (size_t)got;
    }
    char extra; ssize_t got;
    do { got = read(fd, &extra, 1); } while (got < 0 && errno == EINTR);
    if (got || fstat(fd, &after)) { if (got > 0) errno = ESTALE; goto done; }
    if (before.st_dev != after.st_dev || before.st_ino != after.st_ino || before.st_size != after.st_size ||
        before.st_mtime != after.st_mtime || before.st_ctime != after.st_ctime) { errno = ESTALE; goto done; }
#if defined(__APPLE__)
    if (before.st_mtimespec.tv_nsec != after.st_mtimespec.tv_nsec ||
        before.st_ctimespec.tv_nsec != after.st_ctimespec.tv_nsec) { errno = ESTALE; goto done; }
#elif defined(__linux__)
    if (before.st_mtim.tv_nsec != after.st_mtim.tv_nsec ||
        before.st_ctim.tv_nsec != after.st_ctim.tv_nsec) { errno = ESTALE; goto done; }
#endif
    bytes[size] = 0; *out = bytes; *length = size; bytes = NULL; result = 0;
done:
    { int error = errno; free(bytes); close(fd); errno = error; }
    return result;
}

/* 1=v2,0=legacy,-1=invalid. Missing library is legacy; malformed present
 * markers fail explicitly rather than reverting to positional save storage. */
static int catalog_enabled(const char *root)
{
    int directory = open(root, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    if (directory < 0) return errno == ENOENT ? 0 : -1;
    char *bytes = NULL; size_t size = 0;
    int result = catalog_read_file(directory, ".chronos-library-v2", &bytes, &size, 1), error = errno;
    close(directory);
    if (result == 1) return 0;
    if (!result && (size != 2 || memcmp(bytes, "2\n", 2))) { result = -1; error = EPROTO; }
    free(bytes); errno = error;
    return result ? -1 : 1;
}

static uint32_t catalog_read_json(struct catalog_snapshot *s, int directory, const char *name, int optional, unsigned type)
{
    char *bytes = NULL; size_t length = 0;
    int result = catalog_read_file(directory, name, &bytes, &length, optional);
    if (result == 1) return catalog_json_new(&s->json, type);
    if (result) { catalog_error(s, "Cannot read metadata", name); return 0; }
    uint32_t node = catalog_json_parse(&s->json, bytes, length); free(bytes);
    if (!node || s->json.nodes[node].type != type) {
        errno = EINVAL; catalog_error(s, "Invalid JSON metadata", name); return 0;
    }
    return node;
}

static uint32_t catalog_text(struct catalog_snapshot *s, const char *text)
{ return catalog_json_string_copy(&s->json, text ? text : ""); }

static const char *catalog_field(struct catalog_snapshot *s, uint32_t object, const char *key)
{ return catalog_json_string(&s->json, catalog_json_member(&s->json, object, key)); }

static int catalog_path(struct catalog_snapshot *s, uint32_t *out, const char *base, const char *relative)
{
    char value[SAVE_ROUTE_PATH_SIZE];
    int n = snprintf(value, sizeof(value), "%s/%s", base, relative);
    if (n < 0 || (size_t)n >= sizeof(value)) { errno = ENAMETOOLONG; return -1; }
    *out = catalog_text(s, value); return *out ? 0 : -1;
}

static int catalog_regular(int directory, const char *name)
{
    struct stat st;
    if (fstatat(directory, name, &st, AT_SYMLINK_NOFOLLOW)) return -1;
    if (!S_ISREG(st.st_mode)) { errno = EINVAL; return -1; }
    return 0;
}

/* A resource leaf may include one runtime/ component. Native Resource.load
 * appends .m; descriptions use the logical filename without that suffix. */
static int catalog_resource_path(const char *name)
{
    if (!name || !*name || strlen(name) > 220) return 0;
    const char *slash = strchr(name, '/');
    if (!slash) return catalog_component(name, 220);
    return slash - name == 7 && !memcmp(name, "runtime", 7) && catalog_component(slash + 1, 212);
}

static int catalog_load_assets(struct catalog_snapshot *s, struct catalog_entry *e, int directory, const char *game_path)
{
    int runtime = catalog_directory(directory, "runtime");
    if (runtime < 0) return catalog_error(s, "Prepared cover assets missing", game_path);
    e->assets = catalog_read_json(s, runtime, "assets.json", 0, CJ_OBJECT);
    int error = errno; close(runtime); errno = error;
    if (!e->assets) return -1;
    uint32_t schema = catalog_json_member(&s->json, e->assets, "schema");
    if (!schema || s->json.nodes[schema].type != CJ_INT || s->json.nodes[schema].value.integer != 1) {
        errno = EPROTO; return catalog_error(s, "Unknown prepared cover schema", game_path);
    }
    const char *revision = catalog_field(s, e->assets, "revision");
    if (!save_route_game_id(revision)) { errno = EINVAL; return catalog_error(s, "Invalid cover revision", game_path); }
    const char *cover = catalog_field(s, e->assets, "cover_path"), *thumb = catalog_field(s, e->assets, "thumb_path");
    if (!catalog_resource_path(cover) || !catalog_resource_path(thumb)) {
        errno = EINVAL; return catalog_error(s, "Invalid prepared cover path", game_path);
    }
    for (unsigned i = 0; i < 2; ++i) {
        const char *logical = i ? thumb : cover;
        char physical[256]; int n = snprintf(physical, sizeof(physical), "%s.m", logical);
        if (n < 0 || (size_t)n >= sizeof(physical)) { errno = ENAMETOOLONG; return -1; }
        /* Resolve runtime separately: fstatat on an unchecked parent could
         * otherwise traverse a symlink even with AT_SYMLINK_NOFOLLOW. */
        int fd = !strncmp(logical, "runtime/", 8) ? catalog_directory(directory, "runtime") : dup(directory);
        if (fd < 0) return -1;
        int result = catalog_regular(fd, !strncmp(logical, "runtime/", 8) ? physical + 8 : physical);
        error = errno; close(fd); errno = error;
        if (result) return catalog_error(s, "Prepared image missing", logical);
    }
    return catalog_path(s, &e->cover_path, game_path, cover);
}

static int catalog_load_game(struct catalog_snapshot *s, struct catalog_entry *e, int directory,
                             int roms, const char *root, const char *relative)
{
    e->game = catalog_read_json(s, directory, "game.json", 0, CJ_OBJECT);
    if (!e->game) return -1;
    const char *id = catalog_field(s, e->game, "game_id"), *tag = catalog_field(s, e->game, "native_region_tag");
    if (!save_route_game_id(id) || !save_route_ident(tag, 14) || !save_route_relative(relative)) {
        errno = EINVAL; return catalog_error(s, "Game requires V2 identity migration", relative);
    }
    /* Copy small identifiers before any arena append can relocate strings. */
    char game_id[33], native_tag[15]; strcpy(game_id, id); strcpy(native_tag, tag);
    for (unsigned i = 0; i + 1 < s->count; ++i) {
        const struct catalog_entry *old = &s->entries[i];
        if (old->role != CATALOG_GAME) continue;
        if (!strcmp(game_id, catalog_json_string(&s->json, old->game_id)) ||
            !strcmp(native_tag, catalog_json_string(&s->json, old->native_tag))) {
            errno = EEXIST; return catalog_error(s, "Duplicate game identity/native tag", relative);
        }
    }
    e->game_id = catalog_text(s, game_id); e->native_tag = catalog_text(s, native_tag);
    e->save_path = catalog_text(s, relative);
    uint32_t rom = catalog_json_member(&s->json, e->game, "rom");
    const char *name = catalog_field(s, rom, "rom"), *arch = catalog_field(s, rom, "arch");
    if (!catalog_component(name, 240) || !arch ||
        (strcmp(arch, "tg16") && strcmp(arch, "sgx") && strcmp(arch, "tg16cd"))) {
        errno = EINVAL; return catalog_error(s, "Unsupported ROM reference/architecture", relative);
    }
    char filename[256]; snprintf(filename, sizeof(filename), "%s", name);
    size_t n = strlen(filename);
    if ((n > 4 && (!strcasecmp(filename + n - 4, ".pce") || !strcasecmp(filename + n - 4, ".sgx"))))
        strcat(filename, ".m");
    else if (n > 2 && !strcasecmp(filename + n - 2, ".m")) filename[n - 1] = 'm';
    if (catalog_regular(roms, filename)) return catalog_error(s, "Shared ROM missing", filename);
    /* Native loader appends .m internally to compressed HuCard references. */
    n = strlen(filename); if (n > 2 && !strcasecmp(filename + n - 2, ".m")) filename[n - 2] = 0;
    (void)root;
    /* Native emulator paths are relative to its system resource root. The
     * existing ROM bind exposes the already-validated shared pool there. */
    if (catalog_path(s, &e->rom_path, "roms", filename)) return -1;
    ++s->playable_count; return 0;
}

static void catalog_snapshot_free(struct catalog_snapshot *s)
{ if (s) { catalog_json_free(&s->json); free(s); } }

static struct catalog_snapshot *catalog_read_folder(const char *root, const char *lineup, const char *folder)
{
    struct catalog_snapshot *s = calloc(1, sizeof(*s));
    if (!s) return NULL;
    int completed = 0;
    int anchor = -1, lineup_fd = -1, folder_fd = -1, published = -1, roms = -1;
    if (!catalog_request(lineup, folder)) { errno = EINVAL; catalog_error(s, "Invalid folder request", NULL); goto done; }
    strcpy(s->lineup, lineup); strcpy(s->folder, folder);
    int n = snprintf(s->path, sizeof(s->path), "%s/%s%s%s", root, lineup,
                     strcmp(folder, "_root") ? "/" : "", strcmp(folder, "_root") ? folder : "");
    if (n < 0 || (size_t)n >= sizeof(s->path)) { errno = ENAMETOOLONG; goto fail; }
    if (catalog_enabled(root) != 1) { errno = EPROTO; catalog_error(s, "V2 library marker missing or invalid", NULL); goto done; }
    anchor = open(root, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    if (anchor < 0) goto fail;
    struct stat st; if (fstat(anchor, &st)) goto fail;
    s->device = st.st_dev; s->inode = st.st_ino;
    s->settings = catalog_read_json(s, anchor, "settings_games.json", 1, CJ_OBJECT);
    if (!s->settings) goto done;
    lineup_fd = catalog_directory(anchor, lineup);
    if (lineup_fd < 0) goto fail;
    folder_fd = !strcmp(folder, "_root") ? dup(lineup_fd) : catalog_directory(lineup_fd, folder);
    if (folder_fd < 0) goto fail;
    uint32_t list = catalog_read_json(s, folder_fd, "gamelist.json", 0, CJ_ARRAY);
    if (!list) goto done;
    if ((unsigned)s->json.nodes[list].count + (unsigned)(strcmp(folder, "_root") != 0) > CATALOG_MAX_ENTRIES) {
        errno = E2BIG; catalog_error(s, "Folder exceeds 50 entries including BACK", NULL); goto done;
    }
    published = catalog_directory(anchor, "published"); if (published < 0) goto fail;
    roms = catalog_directory(published, "roms"); if (roms < 0) goto fail;
    if (strcmp(folder, "_root")) {
        s->folder_metadata = catalog_read_json(s, folder_fd, "folder.json", 1, CJ_OBJECT);
        if (!s->folder_metadata) goto done;
        struct catalog_entry *back = &s->entries[s->count++]; back->role = CATALOG_BACK;
        back->name = catalog_text(s, "BACK"); back->game_id = catalog_text(s, "");
        back->save_path = catalog_text(s, ""); back->native_tag = catalog_text(s, "FOLDER_BACK");
        back->relative_path = catalog_text(s, "runtime/back");
        int runtime = catalog_directory(anchor, "runtime");
        int directory = runtime < 0 ? -1 : catalog_directory(runtime, "back");
        int error = errno;
        if (runtime >= 0) close(runtime);
        errno = error;
        if (directory < 0) { catalog_error(s, "Shared BACK assets missing", NULL); goto done; }
        char back_path[SAVE_ROUTE_PATH_SIZE];
        n = snprintf(back_path, sizeof(back_path), "%s/runtime/back", root);
        int result = n < 0 || (size_t)n >= sizeof(back_path) ? -1 : catalog_load_assets(s, back, directory, back_path);
        error = errno; close(directory); errno = error;
        if (result) goto done;
        if (!back->name || !back->game_id || !back->save_path || !back->native_tag || !back->relative_path)
            goto fail;
    }
    for (uint32_t node = s->json.nodes[list].child; node; node = s->json.nodes[node].next) {
        if (s->json.nodes[node].type != CJ_OBJECT) { errno = EINVAL; catalog_error(s, "gamelist entry must be an object", NULL); goto done; }
        const char *name = catalog_field(s, node, "folder");
        if (!catalog_component(name, 120)) { errno = EINVAL; catalog_error(s, "Invalid gamelist directory", NULL); goto done; }
        char component[121]; strcpy(component, name);
        for (unsigned i = 0; i < s->count; ++i)
            if (!strcmp(component, catalog_json_string(&s->json, s->entries[i].name))) {
                errno = EEXIST; catalog_error(s, "Duplicate gamelist directory", component); goto done;
            }
        int directory = catalog_directory(folder_fd, component);
        if (directory < 0) { catalog_error(s, "Game directory missing or linked", component); goto done; }
        struct catalog_entry *e = &s->entries[s->count++]; e->sort = node;
        e->name = catalog_text(s, component);
        char relative[SAVE_ROUTE_DIR_SIZE], absolute[SAVE_ROUTE_PATH_SIZE];
        n = snprintf(relative, sizeof(relative), "%s%s%s/%s", lineup,
                     strcmp(folder, "_root") ? "/" : "", strcmp(folder, "_root") ? folder : "", component);
        int result = 0;
        if (n < 0 || (size_t)n >= sizeof(relative)) { errno = ENAMETOOLONG; result = -1; }
        e->relative_path = catalog_text(s, relative);
        n = snprintf(absolute, sizeof(absolute), "%s/%s", root, relative);
        if (n < 0 || (size_t)n >= sizeof(absolute)) { errno = ENAMETOOLONG; result = -1; }
        if (!result && !strncmp(component, "FOLDER", 6)) {
            if (strcmp(folder, "_root")) { errno = EINVAL; result = catalog_error(s, "Nested folders are not supported", component); }
            else {
                e->role = CATALOG_FOLDER;
                e->folder_metadata = catalog_read_json(s, directory, "folder.json", 1, CJ_OBJECT);
                e->game_id = catalog_text(s, ""); e->save_path = catalog_text(s, "");
                char tag[15]; snprintf(tag, sizeof(tag), "FOLDER_%03u", s->count - 1);
                e->native_tag = catalog_text(s, tag);
                if (!e->folder_metadata) result = -1;
            }
        } else if (!result) result = catalog_load_game(s, e, directory, roms, root, relative);
        if (!result) result = catalog_load_assets(s, e, directory, absolute);
        int error = errno; close(directory); errno = error;
        if (result) { if (!s->error[0]) catalog_error(s, "Invalid catalogue entry", component); goto done; }
        if (!e->name || !e->relative_path || !e->game_id || !e->save_path || !e->native_tag) goto fail;
    }
    completed = 1;
    goto done;
fail:
    catalog_error(s, "Cannot read library folder", s->path);
done:
    if (!completed && !s->error[0]) catalog_error(s, "Catalogue loading did not complete", s->path);
    { int error = errno;
      if (roms >= 0) close(roms);
      if (published >= 0) close(published);
      if (folder_fd >= 0) close(folder_fd);
      if (lineup_fd >= 0) close(lineup_fd);
      if (anchor >= 0) close(anchor);
      errno = error; }
    return s;
}

static int catalog_snapshot_current(const char *root, const struct catalog_snapshot *s)
{
    struct stat st;
    if (lstat(root, &st)) return -1;
    if (!S_ISDIR(st.st_mode) || st.st_dev != s->device || st.st_ino != s->inode || catalog_enabled(root) != 1) {
        errno = ESTALE; return -1;
    }
    return 0;
}
#endif
