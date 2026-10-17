/* Exactly one native common-SRAM working slot. Canonical ownership is stable
 * game identity, never a menu position. Every replacement invalidates the owner
 * marker first, then updates the native full-file digest, then commits ownership.
 * Small NAND recovery snapshots preserve an owner when its USB key is absent. */
#ifndef CHRONOS_SAVE_OWNER_H
#define CHRONOS_SAVE_OWNER_H
#include "save_files.h"
#include "save_digest.h"
#include "save_slice.h"
#include <dirent.h>

#define SAVE_OWNER_OFFSET 0x5e84UL /* native schema; V1's four-byte shift is not reused */
#define SAVE_OWNER_BYTES 8448UL

struct save_owner {
    int known;
    char game_id[SAVE_ROUTE_ID_SIZE];
    char directory[SAVE_ROUTE_DIR_SIZE];
};

static int save_owner_parent(const char *path, char *leaf, size_t size)
{
    char copy[SAVE_ROUTE_PATH_SIZE];
    if (snprintf(copy, sizeof(copy), "%s", path) >= (int)sizeof(copy)) { errno = ENAMETOOLONG; return -1; }
    char *slash = strrchr(copy, '/');
    if (!slash || !slash[1] || strlen(slash + 1) >= size) { errno = EINVAL; return -1; }
    snprintf(leaf, size, "%s", slash + 1);
    if (slash == copy) slash[1] = 0; else *slash = 0;
    return open(copy, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
}

static int save_owner_marker(const struct save_owner *owner, const char *path, const char *state)
{
    char bytes[SAVE_ROUTE_ID_SIZE + SAVE_ROUTE_DIR_SIZE + 24], leaf[100];
    int n = snprintf(bytes, sizeof(bytes), "2\n%s\n%s\n%s\n", state, owner->game_id, owner->directory);
    if (n < 0 || (size_t)n >= sizeof(bytes)) { errno = ENAMETOOLONG; return -1; }
    int directory = save_owner_parent(path, leaf, sizeof(leaf));
    if (directory < 0) return -1;
    int result = save_files_atomic(directory, leaf, bytes, (size_t)n), error = errno;
    close(directory); errno = error; return result;
}

static int save_owner_load(struct save_owner *owner, const char *path)
{
    memset(owner, 0, sizeof(*owner));
    int fd = open(path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) return errno == ENOENT ? 0 : -1;
    struct stat st;
    int stat_result = fstat(fd, &st);
    if (stat_result || !S_ISREG(st.st_mode)) {
        int error = stat_result ? errno : EINVAL; close(fd); errno = error; return -1;
    }
    char bytes[SAVE_ROUTE_ID_SIZE + SAVE_ROUTE_DIR_SIZE + 24];
    ssize_t n;
    do { n = read(fd, bytes, sizeof(bytes) - 1); } while (n < 0 && errno == EINTR);
    int error = errno; close(fd); errno = error;
    if (n < 0) return -1;
    bytes[n] = 0;
    if (n == (ssize_t)sizeof(bytes) - 1 || (size_t)n != strlen(bytes)) { errno = EINVAL; return -1; }
    char *line[4], *cursor = bytes;
    for (unsigned i = 0; i < 4; ++i) {
        line[i] = cursor;
        char *end = strchr(cursor, '\n');
        if (!end) { errno = EINVAL; return -1; }
        *end = 0; cursor = end + 1;
    }
    if (*cursor || strcmp(line[0], "2") ||
        (strcmp(line[1], "ready") && strcmp(line[1], "pending")) ||
        !save_route_game_id(line[2]) || !save_route_relative(line[3])) {
        errno = EINVAL; return -1;
    }
    if (!strcmp(line[1], "pending")) return 0;
    snprintf(owner->game_id, sizeof(owner->game_id), "%s", line[2]);
    snprintf(owner->directory, sizeof(owner->directory), "%s", line[3]);
    owner->known = 1;
    return 0;
}

static int save_owner_read_live(const char *live, unsigned char bytes[SAVE_OWNER_BYTES])
{
    int fd = open(live, O_RDONLY | O_CLOEXEC);
    if (fd < 0) return -1;
    struct stat st;
    int result = fstat(fd, &st);
    if (!result && st.st_size < (off_t)(SAVE_OWNER_OFFSET + SAVE_OWNER_BYTES)) { errno = EINVAL; result = -1; }
    if (!result) result = slice_read(fd, bytes, SAVE_OWNER_BYTES, SAVE_OWNER_OFFSET, 0);
    int error = errno; close(fd); errno = error; return result;
}

/* Return1 only for a truly absent file. An empty or malformed file is an error. */
static int save_owner_read_at(int directory, const char *name, unsigned char bytes[SAVE_OWNER_BYTES])
{
    int fd = openat(directory, name, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) return errno == ENOENT ? 1 : -1;
    struct stat st;
    int result = fstat(fd, &st);
    if (!result && (!S_ISREG(st.st_mode) || st.st_size != SAVE_OWNER_BYTES)) { errno = EINVAL; result = -1; }
    if (!result) result = slice_read(fd, bytes, SAVE_OWNER_BYTES, 0, 0);
    int error = errno; close(fd); errno = error; return result;
}

static int save_owner_store_at(int directory, const char *name, const unsigned char bytes[SAVE_OWNER_BYTES])
{
    unsigned char old[SAVE_OWNER_BYTES];
    int read_result = save_owner_read_at(directory, name, old);
    if (read_result < 0) return -1;
    if (!read_result && !memcmp(old, bytes, SAVE_OWNER_BYTES)) return 0;
    return save_files_atomic(directory, name, bytes, SAVE_OWNER_BYTES);
}

static int save_owner_game_root(const struct save_routes *routes, const struct save_owner *owner)
{
    int saves = save_files_game_directory(routes->library, owner->directory, 0);
    if (saves < 0) return -1;
    if (save_files_identity(saves, owner->game_id, 0)) {
        int error = errno; close(saves); errno = error; return -1;
    }
    int game = openat(saves, "..", O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    int error = errno; close(saves); errno = error; return game;
}

/* Existing SRAM without a V2 schema marker might be the old four-byte-shifted
 * representation. Never infer or repair it inside the console hook. */
static int save_owner_schema(int game, int create)
{
    int saves = openat(game, "saves", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    if (saves < 0) return -1;
    int fd = openat(saves, ".chronos-sram-schema", O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC);
    int result = -1;
    if (fd >= 0) {
        char bytes[3];
        ssize_t n;
        do { n = read(fd, bytes, sizeof(bytes)); } while (n < 0 && errno == EINTR);
        if (n == 2 && bytes[0] == '2' && bytes[1] == '\n') result = 0;
        else if (n >= 0) errno = EPROTO;
        int error = errno; close(fd); errno = error;
    } else if (errno == ENOENT) {
        struct stat st;
        if (!fstatat(game, "sram.bin", &st, AT_SYMLINK_NOFOLLOW)) errno = EPROTO;
        else if (errno == ENOENT) result = create ? save_files_atomic(saves, ".chronos-sram-schema", "2\n", 2) : 0;
    }
    int error = errno; close(saves); errno = error; return result;
}

static int save_owner_recovery_directory(const char *recovery_root, int create)
{
    char leaf[100];
    int live = save_owner_parent(recovery_root, leaf, sizeof(leaf));
    if (live < 0) return -1;
    if (create) {
        int made = mkdirat(live, leaf, 0755);
        if ((made && errno != EEXIST) || (!made && fsync(live))) {
            int error = errno; close(live); errno = error; return -1;
        }
    }
    int directory = openat(live, leaf, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    int error = errno; close(live); errno = error;
    if (directory < 0) return -1;
    int schema = openat(directory, ".chronos-sram-schema", O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC);
    if (schema >= 0) {
        char bytes[3];
        ssize_t n;
        do { n = read(schema, bytes, sizeof(bytes)); } while (n < 0 && errno == EINTR);
        int valid = n == 2 && bytes[0] == '2' && bytes[1] == '\n';
        int error = errno; close(schema); errno = error;
        if (!valid) { close(directory); errno = EPROTO; return -1; }
    } else if (errno == ENOENT) {
        int duplicate = dup(directory);
        if (duplicate < 0) { int error = errno; close(directory); errno = error; return -1; }
        DIR *entries = fdopendir(duplicate);
        if (!entries) { int error = errno; close(duplicate); close(directory); errno = error; return -1; }
        int empty = 1;
        struct dirent *entry;
        while ((entry = readdir(entries)))
            if (strcmp(entry->d_name, ".") && strcmp(entry->d_name, "..")) { empty = 0; break; }
        closedir(entries);
        if (!empty) { close(directory); errno = EPROTO; return -1; }
        /* A crash before the first marker/snapshot can leave an empty folder.
         * There are no bytes to interpret; readonly lookup means no recovery. */
        if (!create) { close(directory); errno = ENOENT; return -1; }
        if (save_files_atomic(directory, ".chronos-sram-schema", "2\n", 2)) {
            int error = errno; close(directory); errno = error; return -1;
        }
    } else {
        int error = errno == ENOENT ? EPROTO : errno;
        close(directory); errno = error; return -1;
    }
    return directory;
}

static int save_owner_check_marker(const struct save_owner *owner, const char *marker)
{
    struct save_owner actual;
    if (save_owner_load(&actual, marker) || !actual.known ||
        strcmp(owner->game_id, actual.game_id) || strcmp(owner->directory, actual.directory)) {
        errno = ESTALE; return -1;
    }
    return 0;
}

/* Return1 when safely retained on NAND because canonical storage is unavailable. */
static int save_owner_flush(const struct save_owner *owner, const struct save_routes *routes,
                            const char *live, const char *meta, const char *marker, const char *recovery_root)
{
    if (!owner->known) return 0;
    if (save_owner_check_marker(owner, marker)) return -1;
    if (save_digest_process(live, meta, 0) != 1) { errno = EAGAIN; return -1; }
    unsigned char bytes[SAVE_OWNER_BYTES];
    if (save_owner_read_live(live, bytes)) return -1;
    int game = save_owner_game_root(routes, owner);
    if (game >= 0) {
        int result = save_owner_schema(game, 1);
        if (!result) result = save_owner_store_at(game, "sram.bin", bytes);
        int error = errno;
        close(game); errno = error;
        if (!result) {
            int recovery = save_owner_recovery_directory(recovery_root, 0);
            if (recovery >= 0) {
                char name[SAVE_ROUTE_ID_SIZE + 5];
                snprintf(name, sizeof(name), "%s.bin", owner->game_id);
                if (!unlinkat(recovery, name, 0)) fsync(recovery);
                close(recovery);
            }
            return 0;
        }
    }
    int recovery = save_owner_recovery_directory(recovery_root, 1);
    if (recovery < 0) return -1;
    char name[SAVE_ROUTE_ID_SIZE + 5];
    snprintf(name, sizeof(name), "%s.bin", owner->game_id);
    int result = save_owner_store_at(recovery, name, bytes), error = errno;
    close(recovery); errno = error;
    return result ? -1 : 1;
}

/* Reserved Activate("") semantics for BACK/empty-folder bootstrap. Native
 * dummy-bank writes after this point must not be attributed to the last game. */
static int save_owner_deactivate(struct save_owner *owner, const struct save_routes *routes,
                                 const char *live, const char *meta, const char *marker,
                                 const char *recovery_root)
{
    if (!owner->known) return 0;
    if (save_owner_flush(owner, routes, live, meta, marker, recovery_root) < 0 ||
        save_owner_marker(owner, marker, "pending")) return -1;
    owner->known = 0;
    return 0;
}

static int save_owner_activate(struct save_owner *owner, const struct save_routes *routes,
                               const char *game_id, const char *live, const char *meta,
                               const char *marker, const char *recovery_root)
{
    if (routes->enabled && game_id && !*game_id)
        return save_owner_deactivate(owner, routes, live, meta, marker, recovery_root);
    const struct save_binding *binding = save_route_find(routes, game_id);
    if (!routes->enabled || !binding) { errno = ENOENT; return -1; }
    struct save_owner next = {.known = 1};
    snprintf(next.game_id, sizeof(next.game_id), "%s", binding->game_id);
    snprintf(next.directory, sizeof(next.directory), "%s", binding->directory);
    int game = save_owner_game_root(routes, &next);
    if (game < 0) return -1;
    if (save_owner_schema(game, 0)) { int error = errno; close(game); errno = error; return -1; }
    struct stat canonical;
    int exists = fstatat(game, "sram.bin", &canonical, AT_SYMLINK_NOFOLLOW);
    if ((!exists && (!S_ISREG(canonical.st_mode) || canonical.st_size != SAVE_OWNER_BYTES)) ||
        (exists && errno != ENOENT)) {
        int error = !exists && S_ISLNK(canonical.st_mode) ? ELOOP : !exists ? EINVAL : errno;
        close(game); errno = error; return -1;
    }
    if (owner->known) {
        if (save_owner_check_marker(owner, marker)) { close(game); return -1; }
        if (save_digest_process(live, meta, 0) != 1) { close(game); errno = EAGAIN; return -1; }
        if (!strcmp(owner->game_id, game_id)) {
            /* Same identity at boot (or after a move): live data may be newer. */
            int result = save_owner_marker(&next, marker, "ready"), error = errno;
            close(game); errno = error;
            if (result) return -1;
            *owner = next;
            return save_owner_flush(owner, routes, live, meta, marker, recovery_root) < 0 ? -1 : 0;
        }
        if (save_owner_flush(owner, routes, live, meta, marker, recovery_root) < 0) { close(game); return -1; }
    }
    unsigned char bytes[SAVE_OWNER_BYTES];
    int recovery = save_owner_recovery_directory(recovery_root, 0), restored = 1;
    if (recovery >= 0) {
        char name[SAVE_ROUTE_ID_SIZE + 5];
        snprintf(name, sizeof(name), "%s.bin", game_id);
        restored = save_owner_read_at(recovery, name, bytes);
        int error = errno; close(recovery); errno = error;
        if (restored < 0) { close(game); return -1; }
    } else if (errno != ENOENT) { close(game); return -1; }
    if (restored) {
        restored = save_owner_read_at(game, "sram.bin", bytes);
        if (restored < 0) { close(game); return -1; }
        if (restored) memset(bytes, 0, sizeof(bytes));
    }
    close(game);
    if (save_owner_marker(&next, marker, "pending")) return -1;
    /* Once invalidated, a failed import must never be flushed as the old owner. */
    owner->known = 0;
    int fd = open(live, O_RDWR | O_CLOEXEC);
    if (fd < 0) return -1;
    struct stat st;
    int result = fstat(fd, &st);
    if (!result && st.st_size < (off_t)(SAVE_OWNER_OFFSET + SAVE_OWNER_BYTES)) { errno = EINVAL; result = -1; }
    if (!result) result = slice_write(fd, bytes, sizeof(bytes), SAVE_OWNER_OFFSET);
    if (!result) result = fsync(fd);
    int error = errno; close(fd); errno = error;
    if (result || save_digest_refresh(live, meta)) return -1;
    if (save_owner_marker(&next, marker, "ready")) return -1;
    *owner = next;
    return save_owner_flush(owner, routes, live, meta, marker, recovery_root) < 0 ? -1 : 0;
}
#endif
