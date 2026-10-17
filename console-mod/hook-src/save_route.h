/* Pure, bounded native save path routing. Callers serialize registry updates.
 * Disabled mode leaves every native path untouched. V2 rejects unbound native
 * state slots instead of exposing a previous game's positional save. */
#ifndef CHRONOS_SAVE_ROUTE_H
#define CHRONOS_SAVE_ROUTE_H
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define SAVE_ROUTE_CAPACITY 125 /* native EMU_STATE capacity: 500 / four slots */
#define SAVE_ROUTE_ID_SIZE 33 /* fixed32 lowercase hex characters */
#define SAVE_ROUTE_DIR_SIZE 225
#define SAVE_ROUTE_PATH_SIZE 768
#define SAVE_ROUTE_TAG_SIZE 15 /* fourteen characters plus terminator */

struct save_binding {
    char game_id[SAVE_ROUTE_ID_SIZE];
    char directory[SAVE_ROUTE_DIR_SIZE];
    char native_tag[SAVE_ROUTE_TAG_SIZE];
    unsigned char segment;
};
struct save_routes {
    int enabled;
    char library[SAVE_ROUTE_PATH_SIZE];
    char live_alias[SAVE_ROUTE_PATH_SIZE];
    struct save_binding bindings[SAVE_ROUTE_CAPACITY];
};

static int save_route_decimal(const char *value, unsigned limit, unsigned *out)
{
    if (!value || !*value) { errno = EINVAL; return -1; }
    unsigned n = 0;
    for (; *value; ++value) {
        if (*value < '0' || *value > '9' || n > limit / 10) { errno = EINVAL; return -1; }
        n = n * 10 + (unsigned)(*value - '0');
        if (n > limit) { errno = EINVAL; return -1; }
    }
    *out = n;
    return 0;
}

static int save_route_ident(const char *value, size_t max)
{
    if (!value || !*value || strlen(value) > max) return 0;
    for (; *value; ++value)
        if (!((*value >= 'a' && *value <= 'z') || (*value >= 'A' && *value <= 'Z') ||
              (*value >= '0' && *value <= '9') || *value == '-' || *value == '_')) return 0;
    return 1;
}

static int save_route_game_id(const char *value)
{
    if (!value || strlen(value) != 32) return 0;
    for (; *value; ++value)
        if (!((*value >= '0' && *value <= '9') || (*value >= 'a' && *value <= 'f'))) return 0;
    return 1;
}

static int save_route_relative(const char *value)
{
    if (!value || strlen(value) >= SAVE_ROUTE_DIR_SIZE ||
        (strncmp(value, "jp/", 3) && strncmp(value, "us/", 3))) return 0;
    unsigned components = 0;
    for (const char *part = value; *part;) {
        const char *end = strchr(part, '/');
        size_t n = end ? (size_t)(end - part) : strlen(part);
        if (!n || (n == 1 && part[0] == '.') ||
            (n == 2 && part[0] == '.' && part[1] == '.')) return 0;
        for (size_t i = 0; i < n; ++i)
            if ((unsigned char)part[i] < 32 || part[i] == '\\' || part[i] == ':') return 0;
        if (++components > 3) return 0;
        if (!end) break;
        part = end + 1;
        if (!*part) return 0;
    }
    return components >= 2;
}

static int save_route_bind(struct save_routes *routes, const char *index,
                           const char *game_id, const char *directory,
                           const char *tag, const char *segment)
{
    unsigned at, type;
    if (!routes->enabled || save_route_decimal(index, SAVE_ROUTE_CAPACITY - 1, &at) ||
        save_route_decimal(segment, 12, &type) || (type != 11 && type != 12) ||
        !save_route_game_id(game_id) ||
        !save_route_ident(tag, SAVE_ROUTE_TAG_SIZE - 1) || !save_route_relative(directory)) {
        errno = EINVAL; return -1;
    }
    for (unsigned i = 0; i < SAVE_ROUTE_CAPACITY; ++i) {
        const struct save_binding *old = &routes->bindings[i];
        if (!old->game_id[0]) continue;
        if ((!strcmp(old->game_id, game_id) && strcmp(old->directory, directory)) ||
            (!strcmp(old->directory, directory) && strcmp(old->game_id, game_id)) ||
            (i == at && (strcmp(old->game_id, game_id) || old->segment != type))) {
            errno = EEXIST; return -1;
        }
    }
    struct save_binding *binding = &routes->bindings[at];
    snprintf(binding->game_id, sizeof(binding->game_id), "%s", game_id);
    snprintf(binding->directory, sizeof(binding->directory), "%s", directory);
    snprintf(binding->native_tag, sizeof(binding->native_tag), "%s", tag);
    binding->segment = (unsigned char)type;
    return 0;
}

static const struct save_binding *save_route_find(const struct save_routes *routes,
                                                 const char *game_id)
{
    if (!game_id) return NULL;
    for (unsigned i = 0; i < SAVE_ROUTE_CAPACITY; ++i)
        if (routes->bindings[i].game_id[0] && !strcmp(routes->bindings[i].game_id, game_id))
            return &routes->bindings[i];
    return NULL;
}

/* No filesystem calls. Reject traversal; accept repeated slashes and ./ aliases. */
static int save_route_normalize(const char *input, const char *cwd, char *out, size_t size)
{
    char joined[SAVE_ROUTE_PATH_SIZE];
    if (!input || !*input) { errno = EINVAL; return -1; }
    int n = input[0] == '/' ? snprintf(joined, sizeof(joined), "%s", input)
                          : snprintf(joined, sizeof(joined), "%s/%s", cwd ? cwd : "/usr/game", input);
    if (n < 0 || (size_t)n >= sizeof(joined)) { errno = ENAMETOOLONG; return -1; }
    size_t used = 0;
    if (size < 2) { errno = ENAMETOOLONG; return -1; }
    out[used++] = '/';
    for (const char *p = joined; *p;) {
        while (*p == '/') ++p;
        if (!*p) break;
        const char *end = strchr(p, '/');
        size_t len = end ? (size_t)(end - p) : strlen(p);
        if (len == 2 && p[0] == '.' && p[1] == '.') { errno = EPERM; return -1; }
        if (!(len == 1 && p[0] == '.')) {
            if (used > 1) {
                if (used + 1 >= size) { errno = ENAMETOOLONG; return -1; }
                out[used++] = '/';
            }
            if (len >= size - used) { errno = ENAMETOOLONG; return -1; }
            memcpy(out + used, p, len); used += len;
        }
        if (!end) break;
        p = end + 1;
    }
    out[used] = 0;
    return 0;
}

/* Return 1 mapped, 0 unrelated/legacy, -1 denied (errno describes why).
 * Only engine backup directories are intercepted; canonical game files are
 * never routed again. Suffixes are carried to that same game's temporary file. */
struct save_route_target { unsigned index, slot; int metadata; char suffix[65]; };
static int save_route_resolve(const struct save_routes *routes, const char *path,
                              const char *cwd, struct save_route_target *out)
{
    if (!routes->enabled) return 0;
    char normalized[SAVE_ROUTE_PATH_SIZE];
    if (save_route_normalize(path, cwd, normalized, sizeof(normalized))) {
        if (path && (strstr(path, "data_011_") || strstr(path, "meta_011_") ||
                     strstr(path, "data_012_") || strstr(path, "meta_012_"))) return -1;
        return 0;
    }
    const char *name = NULL;
    const char *prefixes[] = {"/usr/game/save", routes->live_alias};
    for (unsigned i = 0; i < 2; ++i) {
        size_t n = strlen(prefixes[i]);
        if (n && !strncmp(normalized, prefixes[i], n) && normalized[n] == '/') {
            name = normalized + n + 1; break;
        }
    }
    if (!name) return 0;
    if (strchr(name, '/') && (strstr(name, "data_011_") || strstr(name, "meta_011_") ||
                              strstr(name, "data_012_") || strstr(name, "meta_012_"))) {
        errno = EPERM; return -1;
    }
    if (strncmp(name, "data_", 5) && strncmp(name, "meta_", 5)) return 0;
    if (strlen(name) < 9 || name[8] != '_') return 0;
    unsigned segment = 0;
    for (unsigned i = 5; i < 8; ++i) {
        if (name[i] < '0' || name[i] > '9') return 0;
        segment = segment * 10 + (unsigned)(name[i] - '0');
    }
    if (segment != 11 && segment != 12) return 0;
    if (strlen(name) < 17 || name[13] != '.') { errno = EINVAL; return -1; }
    unsigned id = 0;
    for (unsigned i = 9; i < 13; ++i) {
        if (name[i] < '0' || name[i] > '9') { errno = EINVAL; return -1; }
        id = id * 10 + (unsigned)(name[i] - '0');
    }
    if (id / 4 >= SAVE_ROUTE_CAPACITY) { errno = ENOENT; return -1; }
    const struct save_binding *binding = &routes->bindings[id / 4];
    if (!binding->game_id[0] || binding->segment != segment) { errno = ENOENT; return -1; }
    const char *suffix = name + 17;
    if (strncmp(name + 13, ".bin", 4)) {
        if (!strcmp(name + 13, ".tmp")) suffix = ".tmp";
        else { errno = EINVAL; return -1; }
    }
    if (*suffix && *suffix != '.' && *suffix != '~') { errno = EINVAL; return -1; }
    if (strlen(suffix) > 64) { errno = ENAMETOOLONG; return -1; }
    for (const char *p = suffix; *p; ++p)
        if (!((*p >= 'a' && *p <= 'z') || (*p >= 'A' && *p <= 'Z') ||
              (*p >= '0' && *p <= '9') || *p == '.' || *p == '_' || *p == '-' || *p == '~')) {
            errno = EINVAL; return -1;
        }
    if (strstr(suffix, "..")) { errno = EPERM; return -1; }
    out->index = id / 4;
    out->slot = id % 4;
    out->metadata = name[0] == 'm';
    snprintf(out->suffix, sizeof(out->suffix), "%s", suffix);
    return 1;
}

/* Pure full-path adapter for callers/tests that do not need an anchored FD. */
static inline int save_route_path(const struct save_routes *routes, const char *path,
                           const char *cwd, char *out, size_t size)
{
    struct save_route_target target;
    int result = save_route_resolve(routes, path, cwd, &target);
    if (result != 1) return result;
    const struct save_binding *binding = &routes->bindings[target.index];
    int n = snprintf(out, size, "%s/%s/saves/state_%u%s.bin%s", routes->library,
                     binding->directory, target.slot, target.metadata ? "_meta" : "", target.suffix);
    if (n < 0 || (size_t)n >= size) { errno = ENAMETOOLONG; return -1; }
    return 1;
}
#endif
