/* Linux native Backup file IO adapters. Unrelated files and legacy mode are
 * forwarded unchanged. Mapped IO uses a validated, anchored game directory FD,
 * so changing USB mounts cannot redirect an in-flight write to another key. */
#ifndef CHRONOS_SAVE_INTERPOSE_H
#define CHRONOS_SAVE_INTERPOSE_H
#include <dlfcn.h>
#include <pthread.h>
#include <stdarg.h>
#include <sys/syscall.h>
#include "save_files.h"

static struct save_routes g_save_routes;
static pthread_mutex_t g_save_mutex = PTHREAD_MUTEX_INITIALIZER;
static __thread unsigned g_save_bypass;
static pthread_once_t g_save_libc_once = PTHREAD_ONCE_INIT;

static int (*save_real_open)(const char *, int, ...);
static int (*save_real_open64)(const char *, int, ...);
static int (*save_real_openat)(int, const char *, int, ...);
static int (*save_real_openat64)(int, const char *, int, ...);
static FILE *(*save_real_fopen)(const char *, const char *);
static FILE *(*save_real_fopen64)(const char *, const char *);
static FILE *(*save_real_freopen)(const char *, const char *, FILE *);
static FILE *(*save_real_freopen64)(const char *, const char *, FILE *);
static int (*save_real_fstatat)(int, const char *, struct stat *, int);
static int (*save_real_fstatat64)(int, const char *, struct stat64 *, int);
static int (*save_real_fxstatat)(int, int, const char *, struct stat *, int);
static int (*save_real_fxstatat64)(int, int, const char *, struct stat64 *, int);
static int (*save_real_stat)(const char *, struct stat *);
static int (*save_real_lstat)(const char *, struct stat *);
static int (*save_real_stat64)(const char *, struct stat64 *);
static int (*save_real_lstat64)(const char *, struct stat64 *);
static int (*save_real_xstat)(int, const char *, struct stat *);
static int (*save_real_lxstat)(int, const char *, struct stat *);
static int (*save_real_xstat64)(int, const char *, struct stat64 *);
static int (*save_real_lxstat64)(int, const char *, struct stat64 *);
static int (*save_real_access)(const char *, int);
static int (*save_real_faccessat)(int, const char *, int, int);
static int (*save_real_unlink)(const char *);
static int (*save_real_remove)(const char *);
static int (*save_real_unlinkat)(int, const char *, int);
static int (*save_real_rename)(const char *, const char *);
static int (*save_real_renameat)(int, const char *, int, const char *);
static int (*save_real_truncate)(const char *, off_t);
static int (*save_real_truncate64)(const char *, off64_t);
static int (*save_real_mkostemps)(char *, int, int);
static int (*save_real_mkostemps64)(char *, int, int);

static void save_interpose_load(void)
{
    ++g_save_bypass;
#define SAVE_SYMBOL(name, symbol) *(void **)(&save_real_##name) = dlsym(RTLD_NEXT, symbol)
    SAVE_SYMBOL(open, "open"); SAVE_SYMBOL(open64, "open64");
    SAVE_SYMBOL(openat, "openat"); SAVE_SYMBOL(openat64, "openat64");
    SAVE_SYMBOL(fopen, "fopen"); SAVE_SYMBOL(fopen64, "fopen64");
    SAVE_SYMBOL(freopen, "freopen"); SAVE_SYMBOL(freopen64, "freopen64");
    SAVE_SYMBOL(fstatat, "fstatat"); SAVE_SYMBOL(fstatat64, "fstatat64");
    SAVE_SYMBOL(fxstatat, "__fxstatat"); SAVE_SYMBOL(fxstatat64, "__fxstatat64");
    SAVE_SYMBOL(stat, "stat"); SAVE_SYMBOL(lstat, "lstat");
    SAVE_SYMBOL(stat64, "stat64"); SAVE_SYMBOL(lstat64, "lstat64");
    SAVE_SYMBOL(xstat, "__xstat"); SAVE_SYMBOL(lxstat, "__lxstat");
    SAVE_SYMBOL(xstat64, "__xstat64"); SAVE_SYMBOL(lxstat64, "__lxstat64");
    SAVE_SYMBOL(access, "access"); SAVE_SYMBOL(faccessat, "faccessat");
    SAVE_SYMBOL(unlink, "unlink"); SAVE_SYMBOL(remove, "remove");
    SAVE_SYMBOL(unlinkat, "unlinkat"); SAVE_SYMBOL(rename, "rename");
    SAVE_SYMBOL(renameat, "renameat"); SAVE_SYMBOL(truncate, "truncate");
    SAVE_SYMBOL(truncate64, "truncate64");
    SAVE_SYMBOL(mkostemps, "mkostemps"); SAVE_SYMBOL(mkostemps64, "mkostemps64");
#undef SAVE_SYMBOL
    --g_save_bypass;
}

static void save_interpose_ready(void)
{
    if (!g_save_bypass) pthread_once(&g_save_libc_once, save_interpose_load);
}

/* Before glibc2.33 stat/fstatat are inline frontends to versioned symbols;
 * the public non-versioned symbols may not exist in the console libc. */
#ifndef _STAT_VER
#if defined(__x86_64__)
#define SAVE_STAT_VERSION 1
#elif defined(__aarch64__)
#define SAVE_STAT_VERSION 0
#else
#define SAVE_STAT_VERSION 3
#endif
#else
#define SAVE_STAT_VERSION _STAT_VER
#endif
static int save_libc_fstatat(int directory, const char *path, struct stat *out, int flags)
{
    return save_real_fstatat ? save_real_fstatat(directory, path, out, flags)
                            : save_real_fxstatat(SAVE_STAT_VERSION, directory, path, out, flags);
}
static int save_libc_fstatat64(int directory, const char *path, struct stat64 *out, int flags)
{
    return save_real_fstatat64 ? save_real_fstatat64(directory, path, out, flags)
                              : save_real_fxstatat64(SAVE_STAT_VERSION, directory, path, out, flags);
}
static int save_libc_stat(const char *path, struct stat *out)
{
    return save_real_stat ? save_real_stat(path, out) : save_real_xstat(SAVE_STAT_VERSION, path, out);
}
static int save_libc_lstat(const char *path, struct stat *out)
{
    return save_real_lstat ? save_real_lstat(path, out) : save_real_lxstat(SAVE_STAT_VERSION, path, out);
}
static int save_libc_stat64(const char *path, struct stat64 *out)
{
    return save_real_stat64 ? save_real_stat64(path, out) : save_real_xstat64(SAVE_STAT_VERSION, path, out);
}
static int save_libc_lstat64(const char *path, struct stat64 *out)
{
    return save_real_lstat64 ? save_real_lstat64(path, out) : save_real_lxstat64(SAVE_STAT_VERSION, path, out);
}

struct save_io_target { int directory; char leaf[100]; };
static void save_io_close(struct save_io_target *target)
{
    int error = errno;
    if (target->directory >= 0) close(target->directory);
    target->directory = -1;
    errno = error;
}

static int save_io_map(int directory, const char *path, struct save_io_target *target)
{
    target->directory = -1;
    if (g_save_bypass || !__atomic_load_n(&g_save_routes.enabled, __ATOMIC_ACQUIRE)) return 0;
    /* Most calls concern ROMs/assets/systemdata. They need no CWD lookup,
     * registry lock, directory traversal, or routing work. */
    if (!path || (!strstr(path, "data_011_") && !strstr(path, "meta_011_") &&
                  !strstr(path, "data_012_") && !strstr(path, "meta_012_"))) return 0;
    int saved_errno = errno;
    char cwd[SAVE_ROUTE_PATH_SIZE];
    struct save_route_target resolved;
    if (path && path[0] != '/') {
        if (directory == AT_FDCWD) {
            if (!getcwd(cwd, sizeof(cwd))) return -1;
        } else {
            char fdpath[64];
            snprintf(fdpath, sizeof(fdpath), "/proc/self/fd/%d", directory);
            ssize_t n = readlink(fdpath, cwd, sizeof(cwd) - 1);
            if (n < 0) return -1;
            cwd[n] = 0;
            if (strstr(cwd, " (deleted)") &&
                (strstr(path, "data_011_") || strstr(path, "meta_011_") ||
                 strstr(path, "data_012_") || strstr(path, "meta_012_"))) {
                errno = ESTALE; return -1;
            }
        }
    } else cwd[0] = 0;
    pthread_mutex_lock(&g_save_mutex);
    int result = save_route_resolve(&g_save_routes, path, cwd, &resolved);
    if (result == 1) {
        const struct save_binding *binding = &g_save_routes.bindings[resolved.index];
        ++g_save_bypass;
        target->directory = save_files_game_directory(g_save_routes.library, binding->directory, 0);
        if (target->directory < 0 || save_files_identity(target->directory, binding->game_id, 0)) {
            save_io_close(target); result = -1;
        } else {
            int n = snprintf(target->leaf, sizeof(target->leaf), "state_%u%s.bin%s", resolved.slot,
                             resolved.metadata ? "_meta" : "", resolved.suffix);
            if (n < 0 || (size_t)n >= sizeof(target->leaf)) {
                errno = ENAMETOOLONG; save_io_close(target); result = -1;
            }
        }
        --g_save_bypass;
    }
    int error = errno;
    pthread_mutex_unlock(&g_save_mutex);
    errno = result == 0 ? saved_errno : error;
    return result;
}

static int save_open_common(int directory, const char *path, int flags, mode_t mode, int large)
{
    save_interpose_ready();
    struct save_io_target target;
    int mapped = save_io_map(directory, path, &target);
    if (mapped < 0) return -1;
    int result;
    if (mapped) {
        result = large && save_real_openat64 ? save_real_openat64(target.directory, target.leaf, flags | O_NOFOLLOW, mode)
                 : save_real_openat(target.directory, target.leaf, flags | O_NOFOLLOW, mode);
        save_io_close(&target);
    } else if (directory != AT_FDCWD) {
        if (large && save_real_openat64) result = save_real_openat64(directory, path, flags, mode);
        else if (save_real_openat) result = save_real_openat(directory, path, flags, mode);
        else result = (int)syscall(SYS_openat, directory, path, flags, mode);
    } else {
        if (large && save_real_open64) result = save_real_open64(path, flags, mode);
        else if (save_real_open) result = save_real_open(path, flags, mode);
        else result = (int)syscall(SYS_openat, AT_FDCWD, path, flags, mode);
    }
    return result;
}

static int save_open_has_mode(int flags)
{
    return (flags & O_CREAT) || ((flags & O_TMPFILE) == O_TMPFILE);
}
#define SAVE_OPEN_WRAPPER(name, large) \
int name(const char *path, int flags, ...) { \
    mode_t mode = 0; if (save_open_has_mode(flags)) { va_list args; va_start(args, flags); mode = (mode_t)va_arg(args, int); va_end(args); } \
    return save_open_common(AT_FDCWD, path, flags, mode, large); }
#define SAVE_OPENAT_WRAPPER(name, large) \
int name(int directory, const char *path, int flags, ...) { \
    mode_t mode = 0; if (save_open_has_mode(flags)) { va_list args; va_start(args, flags); mode = (mode_t)va_arg(args, int); va_end(args); } \
    return save_open_common(directory, path, flags, mode, large); }
SAVE_OPEN_WRAPPER(open, 0)
SAVE_OPEN_WRAPPER(open64, 1)
SAVE_OPENAT_WRAPPER(openat, 0)
SAVE_OPENAT_WRAPPER(openat64, 1)
int creat(const char *path, mode_t mode) { return save_open_common(AT_FDCWD, path, O_WRONLY | O_CREAT | O_TRUNC, mode, 0); }
int creat64(const char *path, mode_t mode) { return save_open_common(AT_FDCWD, path, O_WRONLY | O_CREAT | O_TRUNC, mode, 1); }
int __open_2(const char *path, int flags) { if (save_open_has_mode(flags)) { errno = EINVAL; return -1; } return save_open_common(AT_FDCWD, path, flags, 0, 0); }
int __open64_2(const char *path, int flags) { if (save_open_has_mode(flags)) { errno = EINVAL; return -1; } return save_open_common(AT_FDCWD, path, flags, 0, 1); }
int __openat_2(int directory, const char *path, int flags) { if (save_open_has_mode(flags)) { errno = EINVAL; return -1; } return save_open_common(directory, path, flags, 0, 0); }
int __openat64_2(int directory, const char *path, int flags) { if (save_open_has_mode(flags)) { errno = EINVAL; return -1; } return save_open_common(directory, path, flags, 0, 1); }

static int save_stdio_flags(const char *mode)
{
    if (!mode || !*mode) { errno = EINVAL; return -1; }
    int flags;
    switch (*mode) {
        case 'r': flags = O_RDONLY; break;
        case 'w': flags = O_WRONLY | O_CREAT | O_TRUNC; break;
        case 'a': flags = O_WRONLY | O_CREAT | O_APPEND; break;
        default: errno = EINVAL; return -1;
    }
    for (const char *p = mode + 1; *p; ++p) {
        if (*p == '+') flags = (flags & ~O_ACCMODE) | O_RDWR;
        else if (*p == 'e') flags |= O_CLOEXEC;
        else if (*p == 'x') flags |= O_EXCL;
        else if (*p != 'b') { errno = EINVAL; return -1; }
    }
    return flags;
}

static FILE *save_fopen_common(const char *path, const char *mode, int large, FILE *stream)
{
    save_interpose_ready();
    struct save_io_target target;
    int mapped = save_io_map(AT_FDCWD, path, &target);
    if (mapped < 0) return NULL;
    if (!mapped) return stream ? (large ? save_real_freopen64(path, mode, stream) : save_real_freopen(path, mode, stream))
                              : (large ? save_real_fopen64(path, mode) : save_real_fopen(path, mode));
    if (stream) {
        /* /proc/self/fd anchors freopen to the validated directory as well. */
        char anchored[180];
        snprintf(anchored, sizeof(anchored), "/proc/self/fd/%d/%s", target.directory, target.leaf);
        struct stat st;
        if (!save_libc_fstatat(target.directory, target.leaf, &st, AT_SYMLINK_NOFOLLOW) && S_ISLNK(st.st_mode)) {
            errno = ELOOP; save_io_close(&target); return NULL;
        }
        FILE *result = large ? save_real_freopen64(anchored, mode, stream) : save_real_freopen(anchored, mode, stream);
        save_io_close(&target); return result;
    }
    int flags = save_stdio_flags(mode);
    if (flags < 0) { save_io_close(&target); return NULL; }
    int fd = large && save_real_openat64 ? save_real_openat64(target.directory, target.leaf, flags | O_NOFOLLOW, 0644)
                                      : save_real_openat(target.directory, target.leaf, flags | O_NOFOLLOW, 0644);
    save_io_close(&target);
    if (fd < 0) return NULL;
    FILE *result = fdopen(fd, mode);
    if (!result) { int error = errno; close(fd); errno = error; }
    return result;
}
FILE *fopen(const char *path, const char *mode) { return save_fopen_common(path, mode, 0, NULL); }
FILE *fopen64(const char *path, const char *mode) { return save_fopen_common(path, mode, 1, NULL); }
FILE *freopen(const char *path, const char *mode, FILE *stream) { return save_fopen_common(path, mode, 0, stream); }
FILE *freopen64(const char *path, const char *mode, FILE *stream) { return save_fopen_common(path, mode, 1, stream); }

static int save_stat_target(struct save_io_target *target, struct stat *out)
{
    int result = save_libc_fstatat(target->directory, target->leaf, out, AT_SYMLINK_NOFOLLOW);
    if (!result && S_ISLNK(out->st_mode)) { errno = ELOOP; result = -1; }
    return result;
}
static int save_stat64_target(struct save_io_target *target, struct stat64 *out)
{
    int result = save_libc_fstatat64(target->directory, target->leaf, out, AT_SYMLINK_NOFOLLOW);
    if (!result && S_ISLNK(out->st_mode)) { errno = ELOOP; result = -1; }
    return result;
}
#define SAVE_STAT_WRAPPER(name, type, real, mapped_call) \
int name(const char *path, type *out) { save_interpose_ready(); struct save_io_target target; \
    int mapped = save_io_map(AT_FDCWD, path, &target); if (mapped < 0) return -1; \
    if (!mapped) return real(path, out); int result = mapped_call(&target, out); save_io_close(&target); return result; }
#define SAVE_XSTAT_WRAPPER(name, type, real, mapped_call) \
int name(int version, const char *path, type *out) { save_interpose_ready(); struct save_io_target target; \
    int mapped = save_io_map(AT_FDCWD, path, &target); if (mapped < 0) return -1; \
    if (!mapped) return real(version, path, out); int result = mapped_call(&target, out); save_io_close(&target); return result; }
SAVE_STAT_WRAPPER(stat, struct stat, save_libc_stat, save_stat_target)
SAVE_STAT_WRAPPER(lstat, struct stat, save_libc_lstat, save_stat_target)
SAVE_STAT_WRAPPER(stat64, struct stat64, save_libc_stat64, save_stat64_target)
SAVE_STAT_WRAPPER(lstat64, struct stat64, save_libc_lstat64, save_stat64_target)
SAVE_XSTAT_WRAPPER(__xstat, struct stat, save_real_xstat, save_stat_target)
SAVE_XSTAT_WRAPPER(__lxstat, struct stat, save_real_lxstat, save_stat_target)
SAVE_XSTAT_WRAPPER(__xstat64, struct stat64, save_real_xstat64, save_stat64_target)
SAVE_XSTAT_WRAPPER(__lxstat64, struct stat64, save_real_lxstat64, save_stat64_target)

int fstatat(int directory, const char *path, struct stat *out, int flags)
{
    save_interpose_ready(); struct save_io_target target;
    int mapped = save_io_map(directory, path, &target); if (mapped < 0) return -1;
    if (!mapped) return save_libc_fstatat(directory, path, out, flags);
    int result = save_stat_target(&target, out); save_io_close(&target); return result;
}
int fstatat64(int directory, const char *path, struct stat64 *out, int flags)
{
    save_interpose_ready(); struct save_io_target target;
    int mapped = save_io_map(directory, path, &target); if (mapped < 0) return -1;
    if (!mapped) return save_libc_fstatat64(directory, path, out, flags);
    int result = save_stat64_target(&target, out); save_io_close(&target); return result;
}

int __fxstatat(int version, int directory, const char *path, struct stat *out, int flags)
{
    save_interpose_ready(); struct save_io_target target;
    int mapped = save_io_map(directory, path, &target); if (mapped < 0) return -1;
    if (!mapped) return save_real_fxstatat(version, directory, path, out, flags);
    int result = save_stat_target(&target, out); save_io_close(&target); return result;
}
int __fxstatat64(int version, int directory, const char *path, struct stat64 *out, int flags)
{
    save_interpose_ready(); struct save_io_target target;
    int mapped = save_io_map(directory, path, &target); if (mapped < 0) return -1;
    if (!mapped) return save_real_fxstatat64(version, directory, path, out, flags);
    int result = save_stat64_target(&target, out); save_io_close(&target); return result;
}

int faccessat(int directory, const char *path, int mode, int flags)
{
    save_interpose_ready(); struct save_io_target target;
    int mapped = save_io_map(directory, path, &target); if (mapped < 0) return -1;
    if (!mapped) return save_real_faccessat(directory, path, mode, flags);
    struct stat st;
    int result = save_stat_target(&target, &st);
    if (!result) result = save_real_faccessat(target.directory, target.leaf, mode, flags);
    save_io_close(&target); return result;
}
int access(const char *path, int mode)
{
    save_interpose_ready(); struct save_io_target target;
    int mapped = save_io_map(AT_FDCWD, path, &target); if (mapped < 0) return -1;
    if (!mapped) return save_real_access(path, mode);
    struct stat st; int result = save_stat_target(&target, &st);
    if (!result) result = save_real_faccessat(target.directory, target.leaf, mode, 0);
    save_io_close(&target); return result;
}

int unlinkat(int directory, const char *path, int flags)
{
    save_interpose_ready(); struct save_io_target target;
    int mapped = save_io_map(directory, path, &target); if (mapped < 0) return -1;
    if (!mapped) return save_real_unlinkat(directory, path, flags);
    if (flags) { errno = EINVAL; save_io_close(&target); return -1; }
    int result = save_real_unlinkat(target.directory, target.leaf, 0);
    save_io_close(&target); return result;
}
static int save_remove_common(const char *path, int remove_directory)
{
    save_interpose_ready(); struct save_io_target target;
    int mapped = save_io_map(AT_FDCWD, path, &target); if (mapped < 0) return -1;
    if (!mapped) return remove_directory ? save_real_remove(path) : save_real_unlink(path);
    int result = save_real_unlinkat(target.directory, target.leaf, 0);
    save_io_close(&target); return result;
}
int unlink(const char *path) { return save_remove_common(path, 0); }
int remove(const char *path) { return save_remove_common(path, 1); }

int renameat(int old_directory, const char *old_path, int new_directory, const char *new_path)
{
    save_interpose_ready(); struct save_io_target old_target, new_target;
    int old_mapped = save_io_map(old_directory, old_path, &old_target);
    if (old_mapped < 0) return -1;
    int new_mapped = save_io_map(new_directory, new_path, &new_target);
    if (new_mapped < 0) { save_io_close(&old_target); return -1; }
    if (!old_mapped && !new_mapped)
        return save_real_renameat(old_directory, old_path, new_directory, new_path);
    int result = -1;
    struct stat a, b;
    if (old_mapped != new_mapped) errno = EXDEV;
    else if (fstat(old_target.directory, &a) || fstat(new_target.directory, &b)) { /* errno retained */ }
    else if (a.st_dev != b.st_dev || a.st_ino != b.st_ino) errno = EXDEV;
    else result = save_real_renameat(old_target.directory, old_target.leaf, new_target.directory, new_target.leaf);
    save_io_close(&old_target); save_io_close(&new_target); return result;
}
int rename(const char *old_path, const char *new_path)
{
    return renameat(AT_FDCWD, old_path, AT_FDCWD, new_path);
}

static int save_truncate_common(const char *path, off64_t length, int large)
{
    save_interpose_ready(); struct save_io_target target;
    int mapped = save_io_map(AT_FDCWD, path, &target); if (mapped < 0) return -1;
    if (!mapped) return large ? save_real_truncate64(path, length) : save_real_truncate(path, (off_t)length);
    int fd = save_real_openat64(target.directory, target.leaf, O_WRONLY | O_NOFOLLOW, 0);
    save_io_close(&target); if (fd < 0) return -1;
    int result = ftruncate64(fd, length), error = errno; close(fd); errno = error; return result;
}
int truncate(const char *path, off_t length) { return save_truncate_common(path, length, 0); }
int truncate64(const char *path, off64_t length) { return save_truncate_common(path, length, 1); }

static int save_mkostemps_common(char *pattern, int suffix, int flags, int large)
{
    save_interpose_ready(); struct save_io_target target;
    int mapped = save_io_map(AT_FDCWD, pattern, &target); if (mapped < 0) return -1;
    if (!mapped) return large && save_real_mkostemps64 ? save_real_mkostemps64(pattern, suffix, flags)
                                                      : save_real_mkostemps(pattern, suffix, flags);
    size_t original_length = strlen(pattern);
    if (suffix < 0 || (size_t)suffix + 6 > original_length) { errno = EINVAL; save_io_close(&target); return -1; }
    char anchored[180];
    snprintf(anchored, sizeof(anchored), "/proc/self/fd/%d/%s", target.directory, target.leaf);
    int result = large && save_real_mkostemps64 ? save_real_mkostemps64(anchored, suffix, flags)
                                               : save_real_mkostemps(anchored, suffix, flags);
    if (result >= 0)
        memcpy(pattern + original_length - (size_t)suffix - 6,
               anchored + strlen(anchored) - (size_t)suffix - 6, 6);
    save_io_close(&target); return result;
}
int mkstemp(char *pattern) { return save_mkostemps_common(pattern, 0, 0, 0); }
int mkstemp64(char *pattern) { return save_mkostemps_common(pattern, 0, 0, 1); }
int mkstemps(char *pattern, int suffix) { return save_mkostemps_common(pattern, suffix, 0, 0); }
int mkstemps64(char *pattern, int suffix) { return save_mkostemps_common(pattern, suffix, 0, 1); }
int mkostemp(char *pattern, int flags) { return save_mkostemps_common(pattern, 0, flags, 0); }
int mkostemp64(char *pattern, int flags) { return save_mkostemps_common(pattern, 0, flags, 1); }
int mkostemps(char *pattern, int suffix, int flags) { return save_mkostemps_common(pattern, suffix, flags, 0); }
int mkostemps64(char *pattern, int suffix, int flags) { return save_mkostemps_common(pattern, suffix, flags, 1); }
#endif
