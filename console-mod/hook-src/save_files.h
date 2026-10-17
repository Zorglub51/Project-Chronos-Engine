/* Small atomic metadata files and symlink-safe canonical game directories. */
#ifndef CHRONOS_SAVE_FILES_H
#define CHRONOS_SAVE_FILES_H
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include "save_route.h"

static int save_files_write_all(int fd, const void *data, size_t size)
{
    size_t done = 0;
    while (done < size) {
        ssize_t n = write(fd, (const char *)data + done, size - done);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) { if (!n) errno = EIO; return -1; }
        done += (size_t)n;
    }
    return 0;
}

static int save_files_atomic(int directory, const char *name, const void *data, size_t size)
{
    static unsigned sequence;
    char temporary[128];
    int fd = -1, result = -1;
    for (unsigned attempt = 0; attempt < 100; ++attempt) {
        int n = snprintf(temporary, sizeof(temporary), ".%s.tmp.%ld.%u", name,
                         (long)getpid(), ++sequence);
        if (n < 0 || (size_t)n >= sizeof(temporary)) { errno = ENAMETOOLONG; return -1; }
        fd = openat(directory, temporary, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0644);
        if (fd >= 0 || errno != EEXIST) break;
    }
    if (fd < 0) return -1;
    if (save_files_write_all(fd, data, size) || fsync(fd)) goto done;
    if (close(fd)) { fd = -1; goto done; }
    fd = -1;
    if (renameat(directory, temporary, directory, name) || fsync(directory)) goto done;
    result = 0;
done:
    {
        int error = errno;
        if (fd >= 0) close(fd);
        if (result) unlinkat(directory, temporary, 0);
        errno = error;
    }
    return result;
}

/* root itself is a trusted configured anchor; every relative component is
 * opened with O_NOFOLLOW. Never follow a game or saves-directory symlink. */
static int save_files_game_directory(const char *root, const char *relative, int make_saves)
{
    if (!save_route_relative(relative)) { errno = EINVAL; return -1; }
    int fd = open(root, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    if (fd < 0) return -1;
    char copy[SAVE_ROUTE_DIR_SIZE];
    snprintf(copy, sizeof(copy), "%s", relative);
    char *cursor = copy;
    while (cursor && *cursor) {
        char *slash = strchr(cursor, '/');
        if (slash) *slash = 0;
        int next = openat(fd, cursor, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
        int error = errno;
        close(fd); fd = next;
        if (fd < 0) { errno = error; return -1; }
        cursor = slash ? slash + 1 : NULL;
    }
    if (make_saves) {
        int made = mkdirat(fd, "saves", 0755);
        if ((made && errno != EEXIST) || (!made && fsync(fd))) {
            int error = errno; close(fd); errno = error; return -1;
        }
    }
    int saves = openat(fd, "saves", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    int error = errno; close(fd); errno = error;
    return saves;
}

static int save_files_identity(int directory, const char *game_id, int create)
{
    char expected[SAVE_ROUTE_ID_SIZE + 1], actual[SAVE_ROUTE_ID_SIZE + 2];
    int length = snprintf(expected, sizeof(expected), "%s\n", game_id);
    if (!save_route_game_id(game_id) || length < 0 ||
        (size_t)length >= sizeof(expected)) { errno = EINVAL; return -1; }
    int fd = openat(directory, ".chronos-game-id", O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) {
        if (errno == ENOENT && create)
            return save_files_atomic(directory, ".chronos-game-id", expected, (size_t)length);
        return -1;
    }
    struct stat st;
    int stat_result = fstat(fd, &st);
    if (stat_result || !S_ISREG(st.st_mode) || st.st_size != length) {
        int error = stat_result ? errno : EINVAL; close(fd); errno = error; return -1;
    }
    ssize_t n;
    do { n = read(fd, actual, sizeof(actual)); } while (n < 0 && errno == EINTR);
    int error = errno; close(fd); errno = error;
    if (n < 0) return -1;
    if (n != length || memcmp(expected, actual, (size_t)length)) { errno = EEXIST; return -1; }
    return 0;
}

/* Missing is reported distinctly, so scripts can migrate positional locks once. */
static int save_files_get_locks(int directory, unsigned *mask)
{
    int fd = openat(directory, "locks.bin", O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) return errno == ENOENT ? 1 : -1;
    struct stat st;
    int stat_result = fstat(fd, &st);
    if (stat_result || !S_ISREG(st.st_mode) || st.st_size != 1) {
        int error = stat_result ? errno : EINVAL; close(fd); errno = error; return -1;
    }
    unsigned char bytes[2];
    ssize_t n;
    do { n = read(fd, bytes, sizeof(bytes)); } while (n < 0 && errno == EINTR);
    int error = errno; close(fd); errno = error;
    if (n < 0) return -1;
    if (n != 1 || bytes[0] > 15) { errno = EINVAL; return -1; }
    *mask = bytes[0];
    return 0;
}

static int save_files_set_locks(int directory, unsigned mask)
{
    if (mask > 15) { errno = EINVAL; return -1; }
    unsigned char byte = (unsigned char)mask;
    return save_files_atomic(directory, "locks.bin", &byte, 1);
}
#endif
