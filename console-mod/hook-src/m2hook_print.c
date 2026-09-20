/*
 * m2hook_print.c — Squirrel print() hook + (optionally) folder-navigation
 * natives for m2engage on PCE Mini (and Pi5 — same VM internals on both).
 *
 * Print hook is always built. Folder-swap natives + execute= channel are
 * gated behind ENABLE_FOLDER_HOOK (default 0). To rebuild with folder
 * support: -DENABLE_FOLDER_HOOK=1 on the gcc line (or change the macro
 * value below).
 *
 * Cross-compile on macOS (PCE Mini target):
 *   bash build.sh    (uses Docker arm-linux-gnueabihf-gcc)
 */

#ifndef ENABLE_FOLDER_HOOK
#define ENABLE_FOLDER_HOOK 1
#endif

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdarg.h>
#include <time.h>
#include <unistd.h>
#include <fcntl.h>
#include <dirent.h>
#include <errno.h>
#include <sys/mman.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <dlfcn.h>

/* Debug flag - set M2HOOK_DEBUG=1 to enable verbose logging */
static int g_debug = 0;
#define DBG(...) do { if (g_debug) fprintf(stderr, __VA_ARGS__); } while(0)

static void cap_log_size(void);

/* Event traces only: no per-frame polling or retained diagnostic buffers.
 * Preserve errno so enabling diagnostics cannot change error handling. */
static void trace_event(const char *format, ...)
{
    if (!g_debug) return;
    int saved_errno = errno;
    static unsigned sequence;
    struct timespec now = {0};
    clock_gettime(CLOCK_MONOTONIC, &now);
    flockfile(stderr);
    cap_log_size();
    fprintf(stderr, "[m2hook trace #%u pid=%ld t=%ld.%03ld] ",
            ++sequence, (long)getpid(), (long)now.tv_sec, now.tv_nsec / 1000000);
    va_list args;
    va_start(args, format);
    vfprintf(stderr, format, args);
    va_end(args);
    fputc('\n', stderr);
    funlockfile(stderr);
    errno = saved_errno;
}

static void trace_file(const char *label, const char *path)
{
    if (!g_debug) return;
    int saved_errno = errno;
    struct stat st;
    if (stat(path, &st) == 0)
        trace_event("file %s path=%s dev=%llu ino=%llu size=%lld mode=%o",
                    label, path, (unsigned long long)st.st_dev,
                    (unsigned long long)st.st_ino, (long long)st.st_size,
                    (unsigned)st.st_mode);
    else
        trace_event("file %s path=%s errno=%d (%s)", label, path, errno, strerror(errno));
    errno = saved_errno;
}

static void trace_mounts(const char *phase)
{
    if (!g_debug) return;
    int saved_errno = errno;
    trace_event("mount snapshot: %s", phase);
    FILE *f = fopen("/proc/self/mountinfo", "r");
    if (f) {
        char line[1024];
        while (fgets(line, sizeof(line), f)) {
            if (!strstr(line, "/usr/game") && !strstr(line, "/mnt/usb")) continue;
            line[strcspn(line, "\n")] = '\0';
            trace_event("mountinfo %s", line);
        }
        fclose(f);
    } else {
        trace_event("cannot read mountinfo errno=%d (%s)", errno, strerror(errno));
    }
    errno = saved_errno;
}

/* Squirrel types (simplified) */
typedef void* HSQUIRRELVM;
typedef char SQChar;
typedef int32_t SQInteger;
typedef uint32_t SQUnsignedInteger;
typedef int32_t SQRESULT;
typedef int32_t SQBool;
typedef SQInteger (*SQFUNCTION)(HSQUIRRELVM v);

/* ---- Squirrel C API addresses (resolved from sq_base_register at file offset
 *      0xd9a54 in the stock JP 1006JP m2engage. Thumb bit set on each call
 *      address.) ---- */
typedef void     (*sq_pushstring_t)(HSQUIRRELVM v, const SQChar *s, SQInteger len);
typedef void     (*sq_pushroottable_t)(HSQUIRRELVM v);
typedef void     (*sq_newclosure_t)(HSQUIRRELVM v, SQFUNCTION fn, SQUnsignedInteger nfreevars);
typedef SQRESULT (*sq_newslot_t)(HSQUIRRELVM v, SQInteger idx, SQBool bstatic);
typedef void     (*sq_pop_t)(HSQUIRRELVM v, SQInteger nelemstopop);
typedef SQRESULT (*sq_getstring_t)(HSQUIRRELVM v, SQInteger idx, const SQChar **c);

static sq_pushstring_t    p_sq_pushstring    = (sq_pushstring_t)   (0x8f340 | 1);
static sq_pushroottable_t p_sq_pushroottable = (sq_pushroottable_t)(0x8fc1c | 1);
static sq_newclosure_t    p_sq_newclosure    = (sq_newclosure_t)   (0x8f95c | 1);
static sq_newslot_t       p_sq_newslot       = (sq_newslot_t)      (0x90c20 | 1);
static sq_pop_t           p_sq_pop           = (sq_pop_t)          (0x9022c | 1);
/* sq_getstring: identified at file offset 0x87eb8 by walking forward from
 * sq_getinteger (sntool's pattern at 0x87d24) past sq_getfloat / sq_getbool
 * and matching the OT_STRING type-check (0x08000010 loaded via movs+movt). */
static sq_getstring_t     p_sq_getstring     = (sq_getstring_t)    (0x87eb8 | 1);

/* Stack-arg helpers — Squirrel pushes function args before invoking.
 * sq_getstring / sq_gettop are also useful but we don't strictly need them
 * for v1 stubs that just log argv[0] as a hex pointer.
 * For a real string read we'd add sq_getstring (also locate by pattern). */

/* ---- Command execution ---- */

#if ENABLE_FOLDER_HOOK

#include "save_digest.h"
#include "save_slice.h"
#include "folder_worker.h"

#define EXECUTE_PREFIX "execute="
#define EXECUTE_PREFIX_LEN 8

/* Folder-swap layout. Staging on USB (FAT32, mounted at /mnt/usb by the
 * boot script + udev hot-plug handlers), live targets on /usr/game
 * (app partition). Atomic rename() requires same FS, so we copy
 * src→<live>.tmp on the live FS, fsync, rename. */
#define FOLDERS_ROOT "/mnt/usb/library/published/folders"
#define LIVE_DIR     "/usr/game/040"
/* Engine writes save states here (symlink to /rootfs_data/). data_011 = CD
 * (EMU_STATE_L), data_012 = HuCard (EMU_STATE). data_008 (SRAM +
 * BACKUP_FLAGS) lives here too. SRAM is per-pack-spliced via the SRAM_*
 * constants below; surrounding bytes (BACKUP_FLAGS, settings) are untouched. */
#define LIVE_SAVE_DIR "/usr/game/save"
#define LIVE_DATA_008 "/usr/game/save/data_008_0000.bin"
#define LIVE_META_008 "/usr/game/save/meta_008_0000.bin"
#define LIVE_SRAM_PACK LIVE_SAVE_DIR "/.sram-pack"

/* SRAM array (`_92_sram_datas`) layout inside data_008_0000.bin. Confirmed
 * empirically against a Dracula X save (game_index 2 in JP retail) — the
 * modified-byte region after an in-game save started at file offset 0xa080,
 * giving BASE = 0xa080 - 2 * SRAM_STRIDE = 0x5E80. Schema from
 * struct_systemdata.psb.m: 150 × struct_sram_data, each 8448 bytes
 * (4 × 64-byte entries + 8192-byte image). */
#define SRAM_OFFSET     0x5E80UL
#define SRAM_STRIDE     8448UL
#define SRAM_GAME_COUNT 150UL
#define SRAM_SLICE_SIZE (SRAM_STRIDE * SRAM_GAME_COUNT)  /* 1,267,200 */
#define ROOT_NAME    "_root"
#define CURRENT_FILE FOLDERS_ROOT "/.current"

/* Each entry: src basename in FOLDERS_ROOT/<lineup>/<dir>/, dst rel under
 * /usr/game/040/. */
struct swap_file {
    const char *src_basename;
    const char *dst_rel;
};
/* Lineup-independent files (paths are identical for jp and us packs). */
static const struct swap_file FOLDER_SWAP_FILES[] = {
    { "title_mode_top.psb.m",         "config/title_mode_top.psb.m"          },
    { "title_prof.psb.m",             "config/title_prof.psb.m"              },
    { NULL, NULL },
};
/* Per-lineup motion sheet basename pattern: %s = "jp" or "us".
 * jp packs ship title_jp_titleselect_jp.psb.m, us packs ship _us. */
#define MOTION_SHEET_FMT "title_jp_titleselect_%s.psb.m"

/* Copy src→dst, fsync dst, close. Returns 0 on success. */
/* ---- File-level bind-mounts (replaces copy-on-swap) ----
 *
 * Linux `mount --bind file_src file_dst` lets us redirect a single file at
 * the VFS layer. Used for per-pack PSBs and per-pack state save files —
 * lets folder/lineup swaps complete in sub-ms (no big copies, no fsync,
 * no SIGPIPE racing the menu thread). MNT_DETACH on umount means the
 * unmount succeeds even if the engine still has the file open (the bind
 * goes away when the last fd closes). Both source and target must
 * exist; for save state slots we touch a zero-byte target if missing.
 */
static int bind_mount_file(const char *src, const char *dst)
{
    trace_event("bind begin src=%s dst=%s", src, dst);
    trace_file("source", src);
    trace_file("target-before", dst);
    /* Ensure target exists (mount --bind fails on missing dst). */
    int fd = open(dst, O_RDWR | O_CREAT, 0644);
    if (fd < 0) {
        fprintf(stderr, "[m2hook] bind: create(%s) failed: %s\n", dst, strerror(errno));
        return -1;
    }
    close(fd);
    /* If already bound (e.g., re-entrant), unbind first to avoid stacking. */
    int unmount_rc = umount2(dst, MNT_DETACH);
    int unmount_errno = unmount_rc < 0 ? errno : 0;
    trace_event("umount-before-bind path=%s rc=%d errno=%d (%s)",
                dst, unmount_rc, unmount_errno,
                unmount_errno == EINVAL ? "not a mount point; expected on first bind" : strerror(unmount_errno));
    if (mount(src, dst, NULL, MS_BIND, NULL) < 0) {
        fprintf(stderr, "[m2hook] bind(%s -> %s) failed: %s\n", src, dst, strerror(errno));
        return -1;
    }
    trace_file("target-after", dst);
    if (g_debug) {
        int saved_errno = errno;
        struct stat source, target;
        int same = stat(src, &source) == 0 && stat(dst, &target) == 0 &&
                   source.st_dev == target.st_dev && source.st_ino == target.st_ino;
        trace_event("bind complete src=%s dst=%s same_inode=%s", src, dst, same ? "yes" : "NO");
        errno = saved_errno;
    }
    return 0;
}

static int unbind_mount_file(const char *path)
{
    int rc = umount2(path, MNT_DETACH);
    int error = rc < 0 ? errno : 0;
    trace_event("umount path=%s rc=%d errno=%d (%s)", path, rc, error, strerror(error));
    if (rc < 0 && error != EINVAL && error != ENOENT) {
        fprintf(stderr, "[m2hook] umount(%s) failed: %s\n", path, strerror(errno));
        return -1;
    }
    return 0;
}

static int copy_with_fsync(const char *src, const char *dst)
{
    int sfd = open(src, O_RDONLY);
    if (sfd < 0) {
        fprintf(stderr, "[m2hook] open(%s) failed: %s\n", src, strerror(errno));
        return -1;
    }
    int dfd = open(dst, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (dfd < 0) {
        fprintf(stderr, "[m2hook] open(%s) failed: %s\n", dst, strerror(errno));
        close(sfd);
        return -1;
    }
    char buf[64 * 1024];
    ssize_t n;
    while ((n = read(sfd, buf, sizeof(buf))) > 0) {
        ssize_t off = 0;
        while (off < n) {
            ssize_t w = write(dfd, buf + off, n - off);
            if (w < 0) {
                if (errno == EINTR) continue;
                fprintf(stderr, "[m2hook] write(%s) failed: %s\n", dst, strerror(errno));
                close(sfd); close(dfd);
                return -1;
            }
            off += w;
        }
    }
    if (n < 0) {
        fprintf(stderr, "[m2hook] read(%s) failed: %s\n", src, strerror(errno));
        close(sfd); close(dfd);
        return -1;
    }
    if (fsync(dfd) < 0) {
        fprintf(stderr, "[m2hook] fsync(%s) failed: %s\n", dst, strerror(errno));
        close(sfd); close(dfd);
        return -1;
    }
    close(sfd); close(dfd);
    return 0;
}

/* fsync a directory. Best-effort. */
static void fsync_dir(const char *path)
{
    int fd = open(path, O_RDONLY);
    if (fd < 0) return;
    fsync(fd);
    close(fd);
}

/* Commit a tiny ownership/current marker, including directory durability. */
static int write_pack_marker(const char *path, const char *value)
{
    char tmp[320], parent[320], line[96];
    if (snprintf(tmp, sizeof(tmp), "%s.tmp", path) >= (int)sizeof(tmp) ||
        snprintf(parent, sizeof(parent), "%s", path) >= (int)sizeof(parent)) return -1;
    int length = snprintf(line, sizeof(line), "%s\n", value);
    if (length < 0 || length >= (int)sizeof(line)) return -1;
    int fd = open(tmp, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) return -1;
    int rc = slice_write(fd, line, length, 0) || fsync(fd);
    if (close(fd)) rc = -1;
    if (rc || rename(tmp, path)) { unlink(tmp); return -1; }
    char *slash = strrchr(parent, '/');
    if (!slash) return -1;
    *slash = 0;
    fd = open(parent, O_RDONLY);
    if (fd < 0) return -1;
    rc = fsync(fd);
    close(fd);
    return rc;
}

static int write_current(const char *target)
{
    if (write_pack_marker(LIVE_SRAM_PACK, target) ||
        write_pack_marker(CURRENT_FILE, target)) return -1;
    trace_event("current committed: %s", target);
    return 0;
}

/* Valid live data belongs to the active pack, and can be newer than its
 * snapshot (snapshots are exported only when leaving a pack). Older hooks
 * have no ownership marker: adopt their .current once during migration. */
static int live_sram_is_current(const char *target)
{
    char owner[96];
    int fd = open(LIVE_SRAM_PACK, O_RDONLY);
    if (fd < 0 && errno != ENOENT) return 0;
    if (fd >= 0) {
        ssize_t n = read(fd, owner, sizeof(owner) - 1);
        close(fd);
        if (n <= 0) return 0;
        owner[n] = 0;
        owner[strcspn(owner, "\n")] = 0;
        if (strcmp(owner, target)) return 0;
    }
    return save_digest_process(LIVE_DATA_008, LIVE_META_008, 0) == 1;
}

/* Read CURRENT_FILE into out (caller-supplied buffer). Returns 0 on success
 * (out is null-terminated, trailing '\n' stripped). On absence/error, sets
 * out to ROOT_NAME and returns 1. */
static int read_current(char *out, size_t outsz)
{
    int fd = open(CURRENT_FILE, O_RDONLY);
    if (fd < 0) {
        trace_event("current unavailable path=%s errno=%d (%s); fallback=%s",
                    CURRENT_FILE, errno, strerror(errno), ROOT_NAME);
        snprintf(out, outsz, "%s", ROOT_NAME);
        return 1;
    }
    ssize_t n = read(fd, out, outsz - 1);
    close(fd);
    if (n <= 0) {
        snprintf(out, outsz, "%s", ROOT_NAME);
        return 1;
    }
    out[n] = '\0';
    char *nl = strchr(out, '\n');
    if (nl) *nl = '\0';
    trace_event("current read: %s", out);
    return 0;
}

/* Validate a [A-Za-z0-9_-]+ identifier — defends against path traversal. */
static int valid_ident(const char *s)
{
    if (!s || !*s) return 0;
    for (const char *p = s; *p; ++p) {
        char c = *p;
        if (!((c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') ||
              (c >= '0' && c <= '9') || c == '_' || c == '-')) {
            return 0;
        }
    }
    return 1;
}

static int valid_lineup(const char *s)
{
    return s && (strcmp(s, "jp") == 0 || strcmp(s, "us") == 0);
}

static int valid_dir_name(const char *s)
{
    if (!s) return 0;
    if (strcmp(s, ROOT_NAME) == 0) return 1;
    /* Accept any dir whose name starts with FOLDER (with or without a
     * trailing underscore). The editor names folders "FOLDER00"/"FOLDER01";
     * legacy/manual folders like "FOLDER_NAMCOT" / "FOLDER_SGX" also pass. */
    if (strncmp(s, "FOLDER", 6) != 0) return 0;
    return valid_ident(s);
}

/* Forward decl — defined further down, used by do_folder_swap below. */
static int parse_current(char *out_lineup, size_t lineup_sz,
                         char *out_dir, size_t dir_sz);

/* Is `name` a per-game save state file we should swap? Matches
 * `data_011_NNNN.bin` / `data_012_NNNN.bin` and their `meta_*` partners.
 * SRAM (data_008) is intentionally excluded — it lives in the same file as
 * BACKUP_FLAGS / settings, so a wholesale swap would clobber those. */
static int is_state_save_file(const char *name)
{
    if (!name) return 0;
    const char *kind = NULL;
    if (strncmp(name, "data_", 5) == 0)      kind = name + 5;
    else if (strncmp(name, "meta_", 5) == 0) kind = name + 5;
    else return 0;
    /* type 011 = CD EMU_STATE_L (~2.2 MB each)
     * type 012 = HuCard EMU_STATE  (~900 KB each)
     * Both swap-friendly now that we bind-mount instead of copying. */
    if (strncmp(kind, "011_", 4) != 0 && strncmp(kind, "012_", 4) != 0) return 0;
    /* Require .bin extension. Rejects leftover .tmp files from previous
     * crashed copies — without this they'd get carried into every swap,
     * polluting both source and destination dirs forever. */
    size_t len = strlen(name);
    if (len < 4) return 0;
    return strcmp(name + len - 4, ".bin") == 0;
}

/* Copy every state save file from src_dir to dst_dir (via .tmp + rename).
 * Missing src_dir is treated as success (nothing to back up). dst_dir is
 * created if absent. */
static int copy_state_saves(const char *src_dir, const char *dst_dir)
{
    DIR *d = opendir(src_dir);
    if (!d) {
        if (errno == ENOENT) return 0;
        fprintf(stderr, "[m2hook] saves: opendir(%s) failed: %s\n", src_dir, strerror(errno));
        return -1;
    }
    mkdir(dst_dir, 0755);  /* ignore EEXIST */

    int copied = 0, rc = 0;
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (!is_state_save_file(e->d_name)) continue;
        char src[320], dst[320], dst_tmp[340];
        snprintf(src,     sizeof(src),     "%s/%s",     src_dir, e->d_name);
        snprintf(dst,     sizeof(dst),     "%s/%s",     dst_dir, e->d_name);
        snprintf(dst_tmp, sizeof(dst_tmp), "%s.tmp",    dst);
        fprintf(stderr, "[m2hook] saves: + %s\n", e->d_name);
        if (copy_with_fsync(src, dst_tmp) != 0) {
            fprintf(stderr, "[m2hook] saves: copy %s -> %s failed\n", src, dst_tmp);
            rc = -1; break;
        }
        if (rename(dst_tmp, dst) < 0) {
            fprintf(stderr, "[m2hook] saves: rename %s -> %s failed: %s\n",
                    dst_tmp, dst, strerror(errno));
            unlink(dst_tmp);
            rc = -1; break;
        }
        ++copied;
    }
    closedir(d);
    if (rc == 0 && copied > 0) fsync_dir(dst_dir);
    fprintf(stderr, "[m2hook] saves: %s -> %s (%d files, rc=%d)\n",
            src_dir, dst_dir, copied, rc);
    return rc;
}

/* Delete state save files (data_011/012, meta_011/012) in `dir` that are
 * NOT in `keep_dir`. Lets us atomically swap: copy new files (via .tmp +
 * rename) overwrites matching slots, then this purges the leftovers from
 * the old pack. Engine never sees a missing file. */
static void purge_state_saves_not_in(const char *dir, const char *keep_dir)
{
    DIR *d = opendir(dir);
    if (!d) return;
    int n = 0;
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (!is_state_save_file(e->d_name)) continue;
        char keep_path[320];
        snprintf(keep_path, sizeof(keep_path), "%s/%s", keep_dir, e->d_name);
        if (access(keep_path, F_OK) == 0) continue;  // still wanted
        char p[320];
        snprintf(p, sizeof(p), "%s/%s", dir, e->d_name);
        if (unlink(p) == 0) ++n;
        else fprintf(stderr, "[m2hook] saves: unlink(%s) failed: %s\n", p, strerror(errno));
    }
    closedir(d);
    if (n > 0) fsync_dir(dir);
    fprintf(stderr, "[m2hook] saves: purged %d stale state file(s) from %s\n", n, dir);
}

/* ---- Bind-mount based state-save management ----
 *
 * Replaces the slow copy-based swap. For each save state slot the active
 * pack holds, bind-mount the pack's file over LIVE_SAVE_DIR/<basename>.
 * Engine reads/writes pass through the bind to the underlying pack file
 * (writes auto-persist into the active pack with no extra work).
 *
 * On swap-out we unbind everything we'd bound and unlink the 0-byte
 * underlay so the engine doesn't see ghost slots from the previous pack.
 * Any NEW slot files the engine created during play (not from the active
 * pack's saves, so not bound) get migrated to the outgoing pack via a
 * one-time cross-filesystem copy + unlink. */

static int file_exists(const char *path)
{
    struct stat st;
    return stat(path, &st) == 0;
}

/* Bind every state save file in `src_dir` over `dst_dir/<basename>`. */
static int bind_state_files(const char *src_dir, const char *dst_dir)
{
    DIR *d = opendir(src_dir);
    if (!d) {
        if (errno == ENOENT) return 0;
        fprintf(stderr, "[m2hook] saves: opendir(%s) failed: %s\n", src_dir, strerror(errno));
        return -1;
    }
    int n = 0, rc = 0;
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (!is_state_save_file(e->d_name)) continue;
        char src[320], dst[320];
        snprintf(src, sizeof(src), "%s/%s", src_dir, e->d_name);
        snprintf(dst, sizeof(dst), "%s/%s", dst_dir, e->d_name);
        if (bind_mount_file(src, dst) != 0) { rc = -1; break; }
        ++n;
    }
    closedir(d);
    fprintf(stderr, "[m2hook] saves: bind %s -> %s (%d files, rc=%d)\n", src_dir, dst_dir, n, rc);
    return rc;
}

/* Unbind every state save file we previously bound from `src_dir` over
 * `dst_dir/*`, and unlink the 0-byte underlay so the next pack doesn't
 * see ghost slots. */
static void unbind_state_files(const char *src_dir, const char *dst_dir)
{
    DIR *d = opendir(src_dir);
    if (!d) return;
    int n = 0;
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (!is_state_save_file(e->d_name)) continue;
        char dst[320];
        snprintf(dst, sizeof(dst), "%s/%s", dst_dir, e->d_name);
        unbind_mount_file(dst);
        int rc = unlink(dst);
        trace_event("save underlay unlink path=%s rc=%d errno=%d", dst, rc, rc < 0 ? errno : 0);
        ++n;
    }
    closedir(d);
    fprintf(stderr, "[m2hook] saves: unbind %d files from %s\n", n, dst_dir);
}

/* Migrate any state save file the engine created in `live_dir` during
 * play that isn't already in `pack_dir` (so wasn't bound) — typically new
 * save slots the user added. Copy across (different filesystems) and
 * unlink the live copy. */
static int migrate_new_saves(const char *live_dir, const char *pack_dir)
{
    DIR *d = opendir(live_dir);
    if (!d) return -1;
    int n = 0, rc = 0;
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (!is_state_save_file(e->d_name)) continue;
        char pack_path[320], live_path[320];
        snprintf(pack_path, sizeof(pack_path), "%s/%s", pack_dir, e->d_name);
        if (file_exists(pack_path)) continue;  /* already in pack -> was bound */
        snprintf(live_path, sizeof(live_path), "%s/%s", live_dir, e->d_name);
        char tmp[340];
        snprintf(tmp, sizeof(tmp), "%s.tmp", pack_path);
        if (copy_with_fsync(live_path, tmp) == 0) {
            if (rename(tmp, pack_path) == 0) {
                if (unlink(live_path)) { rc = -1; break; }
                ++n;
            } else {
                fprintf(stderr, "[m2hook] saves: rename(%s -> %s) failed: %s\n",
                        tmp, pack_path, strerror(errno));
                unlink(tmp);
                rc = -1; break;
            }
        } else {
            unlink(tmp);
            rc = -1; break;
        }
    }
    closedir(d);
    if (n > 0) {
        fsync_dir(pack_dir);
        fprintf(stderr, "[m2hook] saves: migrated %d new file(s) to %s\n", n, pack_dir);
    }
    return rc;
}

/* Return conventions stay 0/-1 for callers; helpers report changed/unchanged. */
static int sram_splice_out(const char *path)
{
    int rc = save_slice_export(LIVE_DATA_008, path, SRAM_OFFSET, SRAM_SLICE_SIZE);
    trace_event("sram out: %s path=%s errno=%d", rc < 0 ? "failed" :
                rc ? "written" : "unchanged", path, rc < 0 ? errno : 0);
    return rc < 0 ? -1 : 0;
}

static int sram_splice_in(const char *path)
{
    int rc = save_slice_import(LIVE_DATA_008, path, SRAM_OFFSET, SRAM_SLICE_SIZE);
    /* Also repair a stale digest after an interrupted import. The digest
     * helper avoids writing metadata whose digest already matches. */
    if (rc >= 0 && save_digest_refresh(LIVE_DATA_008, LIVE_META_008)) rc = -1;
    trace_event("sram in: %s path=%s errno=%d", rc < 0 ? "failed" :
                rc ? "written" : "unchanged", path, rc < 0 ? errno : 0);
    return rc < 0 ? -1 : 0;
}

/* Perform the folder swap:
 *   src = FOLDERS_ROOT/<lineup>/<dir>/
 *   dst = LIVE_DIR/
 * Each file copied as <dst>/<rel>.tmp then atomically renamed in-place.
 * .current is updated to "<lineup>/<dir>".
 * Returns 0 on success. */
static int do_folder_swap(const char *lineup, const char *dir)
{
    if (!valid_lineup(lineup) || !valid_dir_name(dir)) {
        fprintf(stderr, "[m2hook] folder swap: invalid '%s/%s'\n",
                lineup ? lineup : "(null)", dir ? dir : "(null)");
        return -1;
    }

    char cur[64];
    read_current(cur, sizeof(cur));
    char target[64];
    snprintf(target, sizeof(target), "%s/%s", lineup, dir);
    if (strcmp(cur, target) == 0) {
        fprintf(stderr, "[m2hook] folder swap: already at '%s', no-op\n", target);
        return 0;
    }

    char check[320];
    const char *required[] = {"title_mode_top.psb.m", "title_prof.psb.m", NULL};
    for (int i = 0; i < 3; ++i) {
        char motion[64];
        snprintf(motion, sizeof(motion), MOTION_SHEET_FMT, lineup);
        snprintf(check, sizeof(check), "%s/%s/%s", FOLDERS_ROOT, target,
                 i < 2 ? required[i] : motion);
        if (access(check, R_OK)) return -1;
    }

    fprintf(stderr, "[m2hook] folder swap: %s -> %s\n", cur, target);
    trace_mounts("before folder swap");
    fflush(stderr);

    /* Build incoming + outgoing pack paths up front so phases can share them. */
    char incoming_saves[280];
    snprintf(incoming_saves, sizeof(incoming_saves), "%s/%s/%s/saves",
             FOLDERS_ROOT, lineup, dir);
    mkdir(incoming_saves, 0755);

    char old_lineup[8] = {0}, old_dir[64] = {0};
    int have_outgoing = 0;
    char outgoing_saves[280] = {0};
    if (parse_current(old_lineup, sizeof(old_lineup), old_dir, sizeof(old_dir)) == 0
        && valid_lineup(old_lineup) && valid_dir_name(old_dir)) {
        have_outgoing = 1;
        snprintf(outgoing_saves, sizeof(outgoing_saves), "%s/%s/%s/saves",
                 FOLDERS_ROOT, old_lineup, old_dir);
        mkdir(outgoing_saves, 0755);
    }

    /* Phase 1 (swap-out): migrate any NEW state files (slots created by
     * the engine during play that aren't bind-mounted), then unbind the
     * outgoing pack's state files. SRAM splice-out captures the latest
     * data_008 SRAM slice into the pack's sram.bin (still needs a copy
     * because BACKUP_FLAGS and SRAM share data_008). */
    if (have_outgoing) {
        trace_event("swap phase=outgoing-saves path=%s", outgoing_saves);
        char outgoing_sram[280];
        snprintf(outgoing_sram, sizeof(outgoing_sram), "%s/sram.bin", outgoing_saves);
        if (sram_splice_out(outgoing_sram) != 0) {
            fprintf(stderr, "[m2hook] folder swap: SRAM backup failed; aborting\n");
            return -1;
        }
        if (migrate_new_saves(LIVE_SAVE_DIR, outgoing_saves)) return -1;
        /* A crash during import must not make incoming bytes look like a
         * valid outgoing save on reboot, even if the digest was refreshed. */
        if (write_pack_marker(LIVE_SRAM_PACK, "pending")) return -1;
        unbind_state_files(outgoing_saves, LIVE_SAVE_DIR);
    }

    /* Phase 2: bind-mount the incoming pack's PSBs over the live ones.
     * Sub-ms per file. The other PSBs in /usr/game/040/{config,motion}
     * (mode_logo, mode_staff, bg01, emu_screen, etc.) stay on NAND
     * unaltered. */
    {
        char src[320], dst[320];
        trace_event("swap phase=incoming-profiles pack=%s", target);
        /* config/title_mode_top.psb.m */
        snprintf(src, sizeof(src), "%s/%s/%s/title_mode_top.psb.m", FOLDERS_ROOT, lineup, dir);
        snprintf(dst, sizeof(dst), "%s/config/title_mode_top.psb.m", LIVE_DIR);
        if (bind_mount_file(src, dst) != 0) return -1;

        /* config/title_prof.psb.m */
        snprintf(src, sizeof(src), "%s/%s/%s/title_prof.psb.m", FOLDERS_ROOT, lineup, dir);
        snprintf(dst, sizeof(dst), "%s/config/title_prof.psb.m", LIVE_DIR);
        if (bind_mount_file(src, dst) != 0) return -1;

        /* motion/title_jp_titleselect_<lineup>.psb.m */
        char motion_basename[64];
        snprintf(motion_basename, sizeof(motion_basename), MOTION_SHEET_FMT, lineup);
        snprintf(src, sizeof(src), "%s/%s/%s/%s", FOLDERS_ROOT, lineup, dir, motion_basename);
        snprintf(dst, sizeof(dst), "%s/motion/%s", LIVE_DIR, motion_basename);
        if (bind_mount_file(src, dst) != 0) return -1;
    }

    /* Phase 3 (swap-in): bind the incoming pack's state save files over
     * the live save dir, then splice the incoming pack's SRAM slice into
     * data_008. */
    trace_event("swap phase=incoming-saves path=%s", incoming_saves);
    if (bind_state_files(incoming_saves, LIVE_SAVE_DIR) != 0) {
        fprintf(stderr, "[m2hook] folder swap: state file bind failed\n");
        return -1;
    }
    {
        char incoming_sram[280];
        snprintf(incoming_sram, sizeof(incoming_sram), "%s/sram.bin", incoming_saves);
        if (sram_splice_in(incoming_sram) != 0) {
            fprintf(stderr, "[m2hook] folder swap: SRAM restore failed\n");
            return -1;
        }
    }

    if (write_current(target) != 0) {
        fprintf(stderr, "[m2hook] warning: failed to update %s\n", CURRENT_FILE);
        return -1;
    }

    fprintf(stderr, "[m2hook] folder swap: done -> %s\n", target);
    trace_mounts("after folder swap");
    fflush(stderr);
    return 0;
}

/* Parse the current "<lineup>/<dir>" string into separate buffers. */
static int parse_current(char *out_lineup, size_t lineup_sz,
                         char *out_dir, size_t dir_sz)
{
    char buf[64];
    read_current(buf, sizeof(buf));
    char *slash = strchr(buf, '/');
    if (!slash) {
        /* legacy single-name format — treat as jp/<name> for backcompat */
        snprintf(out_lineup, lineup_sz, "jp");
        snprintf(out_dir, dir_sz, "%s", buf);
        return 0;
    }
    *slash = '\0';
    snprintf(out_lineup, lineup_sz, "%s", buf);
    snprintf(out_dir, dir_sz, "%s", slash + 1);
    return 0;
}

/* Skip leading whitespace, return pointer to first non-space char. */
static const char *skip_ws(const char *s)
{
    while (*s && (*s == ' ' || *s == '\t' || *s == '\n' || *s == '\r')) ++s;
    return s;
}

/*
 * Execute a command from print("execute=...") output.
 *
 * Recognised forms:
 *   enterGameFolder <FOLDER_TAG>   — swap to FOLDERS_ROOT/<FOLDER_TAG>/
 *   exitGameFolder                 — swap back to FOLDERS_ROOT/_root/
 *   <anything else>                — fall through to system()
 */
static int execute_command(const char *cmd)
{
    DBG("[m2hook] execute: %s\n", cmd);

    /* trim trailing newline (Squirrel print appends it) */
    char buf[256];
    snprintf(buf, sizeof(buf), "%s", cmd);
    size_t blen = strlen(buf);
    while (blen > 0 && (buf[blen-1] == '\n' || buf[blen-1] == '\r')) {
        buf[--blen] = '\0';
    }

    /* execute= channel is a fallback (print pipeline) — not the primary
     * folder-nav path. Disabled here pending tag-format updates; primary
     * path is the registered native via vm_get_string. */
    (void)skip_ws;

    int ret = system(buf);
    int status = WEXITSTATUS(ret);
    DBG("[m2hook] execute done: exit=%d\n", status);
    return status;
}

#endif /* ENABLE_FOLDER_HOOK */

/* ---- Print capture ---- */

static int g_print_hooked = 0;
static uintptr_t g_original_print = 0;

/* Cap the redirected stderr log. /tmp on PCE Mini is tmpfs (~123 MB
 * shared with the rest of /tmp), so unbounded growth would eventually
 * OOM the system on long debug sessions. Bumped to 8 MB for probe runs
 * that need to capture lots of texture-upload events. */
#define LOG_SIZE_CAP (8 * 1024 * 1024)

static void cap_log_size(void)
{
    int fd = fileno(stderr);
    if (fd < 0) return;
    struct stat st;
    if (fstat(fd, &st) != 0) return;
    if (st.st_size <= LOG_SIZE_CAP) return;

    /* Callers hold stderr's stream lock, including the IO worker. */
    if (ftruncate(fd, 0) == 0) {
        lseek(fd, 0, SEEK_SET);
    }
    fprintf(stderr, "[m2hook] log truncated (was %ld bytes, cap %d)\n",
            (long)st.st_size, LOG_SIZE_CAP);
}

/*
 * Our replacement for Squirrel's _printfunc slot. m2 routes both Squirrel
 * base print() and its own printf() Sqrat binding through this slot, so
 * we catch both with a single hook.
 *
 * Call shape (as used by m2 / matched by sntool's printf_hook):
 *   v        — VM
 *   format   — original format string (NUL-terminated)
 *   outlen   — length of formatted output
 *   output   — formatted output buffer (NUL-terminated). May be NULL when
 *              called from Squirrel base_print(v, "%s", str) — in that
 *              case `format` holds "%s" and the actual string is in r2/outlen
 *              re-cast as pointer; we fall back to `format` to be safe.
 */
void sq_printfunc(HSQUIRRELVM v, SQChar *format, SQInteger outlen, SQChar *output)
{
    (void)v;
    (void)outlen;

    const char *msg = output ? output : format;
    if (!msg) return;

#if ENABLE_FOLDER_HOOK
    /* Check for execute= prefix */
    if (strncmp(msg, EXECUTE_PREFIX, EXECUTE_PREFIX_LEN) == 0) {
        execute_command(msg + EXECUTE_PREFIX_LEN);
        return;
    }
#endif

    flockfile(stderr);
    cap_log_size();

    fprintf(stderr, "[SQ] %s", msg);
    size_t len = strlen(msg);
    if (len == 0 || msg[len-1] != '\n')
        fprintf(stderr, "\n");
    fflush(stderr);
    funlockfile(stderr);
}

/*
 * Find and replace the print function pointer in the Squirrel VM.
 */
static void hook_vm_print(HSQUIRRELVM v)
{
    if (g_print_hooked || !v) return;

    uintptr_t *p = (uintptr_t *)v;

    /* Navigate to print function: vm[0x94/4] -> shared_state, then [0xa4/4] -> _printfunc */
    uintptr_t *shared_state = (uintptr_t *)p[0x94 / 4];
    if (!shared_state) {
        DBG("[m2hook] VM shared_state is NULL\n");
        return;
    }

    g_original_print = shared_state[0xa4 / 4];
    if (!g_original_print) {
        /* Print function not set yet - will retry on next call */
        return;
    }

    DBG("[m2hook] Found VM print at 0x%x, replacing with sq_printfunc\n",
            (unsigned)g_original_print);

    shared_state[0xa4 / 4] = (uintptr_t)&sq_printfunc;
    g_print_hooked = 1;

    DBG("[m2hook] Squirrel print() hooked!\n");
}

/* ---- Hook state ---- */

uintptr_t g_sq_pushstring_addr = 0;
uint8_t g_saved_prologue[8];
uintptr_t g_continue_addr = 0;
uintptr_t g_null_target = 0;  /* CBZ branch target when s==NULL */

/* Forward declaration */
void hook_sq_pushstring_asm(void);

/* Counter to track hook calls */
static volatile int g_hook_call_count = 0;

/*
 * C part of the hook - called with original arguments preserved
 */
#if ENABLE_FOLDER_HOOK

/* ---- Native functions exposed to Squirrel scripts ---- */

/* Read a string arg from a Squirrel VM stack slot directly, bypassing
 * sq_getstring's error-raising path (which longjmp's on type mismatch and
 * kills the script at top-level). Returns NULL on type mismatch.
 *
 * Layout derived from sq_getstring disassembly at 0x87eb8:
 *   vm[0x18/4]                  → _stack._vals (SQObjectPtr* array)
 *   vm[0x34/4]                  → _stackbase (SQInteger)
 *   stack_obj = vals[stackbase + idx - 1]      (each SQObjectPtr is 8 bytes)
 *   stack_obj.type   = stack_obj[0]            (4 bytes)
 *   stack_obj.value  = stack_obj[1]            (4 bytes — SQString* for strings)
 *   string_data      = (char*)SQString + 0x1c  (skip SQString header)
 *   OT_STRING        = 0x08000010
 */
static const SQChar *vm_get_string(HSQUIRRELVM v, SQInteger idx)
{
    if (!v) return NULL;
    uintptr_t *vm = (uintptr_t *)v;
    uintptr_t *vals = (uintptr_t *)vm[0x18 / 4];
    if (!vals) return NULL;
    SQInteger stackbase = (SQInteger)vm[0x34 / 4];
    SQInteger slot = stackbase + idx - 1;
    if (slot < 0) return NULL;
    /* Each SQObjectPtr is 8 bytes; vals is the base of the SQObjectPtr array. */
    uintptr_t *obj = (uintptr_t *)((uint8_t *)vals + slot * 8);
    uintptr_t obj_type = obj[0];
    uintptr_t obj_val = obj[1];
    if (obj_type != 0x08000010 /* OT_STRING */) return NULL;
    return (const SQChar *)(obj_val + 0x1c);
}

/* Split a script-side regionTag of the form "FOLDER_<lineup>_<dir>"
 * (e.g. "FOLDER_jp_FOLDER_SGX") into separate (lineup, dir) outputs.
 * Returns 0 on success, -1 on malformed input. Output buffers are written
 * with NUL-terminated tokens. */
static int parse_region_tag(const char *tag,
                            char *out_lineup, size_t lineup_sz,
                            char *out_dir, size_t dir_sz)
{
    if (!tag || strncmp(tag, "FOLDER_", 7) != 0) return -1;
    const char *lineup_start = tag + 7;
    const char *underscore = strchr(lineup_start, '_');
    if (!underscore) return -1;
    size_t lineup_len = (size_t)(underscore - lineup_start);
    if (lineup_len + 1 > lineup_sz) return -1;
    memcpy(out_lineup, lineup_start, lineup_len);
    out_lineup[lineup_len] = '\0';
    if (strlen(underscore + 1) >= dir_sz) return -1;
    snprintf(out_dir, dir_sz, "%s", underscore + 1);
    return 0;
}

static SQInteger native_enter_game_folder(HSQUIRRELVM v)
{
    const SQChar *tag = vm_get_string(v, 2);  // idx=2 = first user arg
    char lineup[8], dir[64];
    int parsed = (tag && parse_region_tag(tag, lineup, sizeof(lineup),
                                          dir, sizeof(dir)) == 0) ? 1 : 0;
    fprintf(stderr, "[m2hook] enterGameFolder(\"%s\") -> %s/%s\n",
            tag ? tag : "(null)",
            parsed ? lineup : "(?)", parsed ? dir : "(?)");
    fflush(stderr);
    if (parsed) do_folder_swap(lineup, dir);
    return 0;
}

static SQInteger native_exit_game_folder(HSQUIRRELVM v)
{
    (void)v;
    /* Use the lineup recorded in .current; swap to its _root pack. */
    char cur_lineup[8], cur_dir[64];
    parse_current(cur_lineup, sizeof(cur_lineup), cur_dir, sizeof(cur_dir));
    fprintf(stderr, "[m2hook] exitGameFolder() -> %s/%s\n", cur_lineup, ROOT_NAME);
    fflush(stderr);
    do_folder_swap(cur_lineup, ROOT_NAME);
    return 0;
}

static struct folder_worker g_folder_worker = FOLDER_WORKER_INIT;

static SQInteger native_begin_folder_swap(HSQUIRRELVM v)
{
    char lineup[8], dir[64];
    const char *tag = vm_get_string(v, 2);
    int error = EINVAL;
    if (parse_region_tag(tag, lineup, sizeof(lineup), dir, sizeof(dir)) == 0 &&
        valid_lineup(lineup) && valid_dir_name(dir))
        error = folder_worker_begin(&g_folder_worker, lineup, dir, do_folder_swap);
    trace_event("async swap start tag=%s error=%d", tag ? tag : "(null)", error);
    p_sq_pushstring(v, error ? "error" : "busy", -1);
    return 1;
}

static SQInteger native_poll_folder_swap(HSQUIRRELVM v)
{
    int result = folder_worker_poll(&g_folder_worker);
    if (result != 1) trace_event("async swap complete result=%d", result);
    p_sq_pushstring(v, result == 1 ? "busy" : result == 0 ? "ok" : "error", -1);
    return 1;
}

/* Recursion guard: register_folder_natives calls sq_pushstring, which
 * lands on our trampoline → hook_sq_pushstring_handler. Without this
 * guard we'd re-enter the registration logic infinitely. */
static volatile int g_in_registration = 0;
static int g_natives_registered = 0;

static void register_folder_natives(HSQUIRRELVM v)
{
    g_in_registration = 1;

    p_sq_pushroottable(v);

    p_sq_pushstring(v, "enterGameFolder", -1);
    p_sq_newclosure(v, native_enter_game_folder, 0);
    p_sq_newslot(v, -3, 0 /*SQFalse*/);

    p_sq_pushstring(v, "exitGameFolder", -1);
    p_sq_newclosure(v, native_exit_game_folder, 0);
    p_sq_newslot(v, -3, 0 /*SQFalse*/);

    p_sq_pushstring(v, "beginGameFolderSwap", -1);
    p_sq_newclosure(v, native_begin_folder_swap, 0);
    p_sq_newslot(v, -3, 0);
    p_sq_pushstring(v, "pollGameFolderSwap", -1);
    p_sq_newclosure(v, native_poll_folder_swap, 0);
    p_sq_newslot(v, -3, 0);

    p_sq_pop(v, 1);  /* pop root table */

    g_natives_registered = 1;
    g_in_registration = 0;

    fprintf(stderr, "[m2hook] registered ::enterGameFolder, ::exitGameFolder\n");
    fflush(stderr);
}

#endif /* ENABLE_FOLDER_HOOK */

void __attribute__((used)) hook_sq_pushstring_handler(HSQUIRRELVM v, const SQChar *s, SQInteger len)
{
    (void)len;

#if ENABLE_FOLDER_HOOK
    if (g_in_registration) {
        /* recursive call from inside register_folder_natives — let the
         * original sq_pushstring run as normal, do nothing extra */
        return;
    }
#endif

    g_hook_call_count++;

    /* Log first few calls in debug mode */
    if (g_debug && g_hook_call_count <= 5) {
        fprintf(stderr, "[m2hook] sq_pushstring #%d: v=%p s=%p", g_hook_call_count, v, s);
        if (s) fprintf(stderr, " \"%s\"", s);
        fprintf(stderr, "\n");
        fflush(stderr);
    }

    if (!g_print_hooked && v) {
        hook_vm_print(v);
#if ENABLE_FOLDER_HOOK
        /* Once print is hooked and v is valid, register the folder natives
         * (one-shot — guarded by g_natives_registered). */
        if (g_print_hooked && !g_natives_registered) {
            register_folder_natives(v);
        }
#endif
    } else if (g_print_hooked && v) {
        /* Defend against m2 calling sq_setprintfunc after our install. */
        uintptr_t *vm = (uintptr_t *)v;
        uintptr_t *shared_state = (uintptr_t *)vm[0x94 / 4];
        if (shared_state &&
            shared_state[0xa4 / 4] != (uintptr_t)&sq_printfunc) {
            static int reinstall_count = 0;
            if (++reinstall_count <= 3) {
                fprintf(stderr, "[m2hook] _printfunc overwritten, re-installing (attempt %d)\n",
                        reinstall_count);
                fflush(stderr);
            }
            shared_state[0xa4 / 4] = (uintptr_t)&sq_printfunc;
        }
    }
}

/*
 * Assembly hook that:
 * 1. Saves all registers
 * 2. Calls our handler
 * 3. Restores registers
 * 4. Executes original prologue
 * 5. Jumps to continue address
 *
 * Original sq_pushstring prologue (8 bytes):
 *   10 b5       PUSH {r4, lr}
 *   82 b0       SUB sp, #8
 *   f9 b1       CBZ r1, +offset  (if s==NULL, skip)
 *   04 46       MOV r4, r0
 */
/* Force Thumb mode for this function */
__attribute__((naked, used, target("thumb")))
void hook_sq_pushstring_asm(void)
{
    __asm__ volatile (
        ".thumb\n"
        ".syntax unified\n"

        /* Save low registers and lr (Thumb-1 compatible) */
        "push {r0-r7}\n"
        "mov r7, lr\n"
        "push {r7}\n"
        "sub sp, #4\n"  /* Align stack */

        /* Call our C handler */
        "bl hook_sq_pushstring_handler\n"

        /* Restore */
        "add sp, #4\n"
        "pop {r7}\n"
        "mov lr, r7\n"
        "pop {r0-r7}\n"

        /* Original prologue: push {r4, lr}; sub sp, #8 */
        "push {r4, lr}\n"
        "sub sp, #8\n"

        /* Handle CBZ r1 (if s == NULL) - branch to null_target */
        "cmp r1, #0\n"
        "beq 1f\n"

        /* Non-null path: mov r4, r0; then jump to continue_addr */
        "mov r4, r0\n"
        "ldr r3, =g_continue_addr\n"
        "ldr r3, [r3]\n"
        "bx r3\n"

        /* Null path: skip mov r4,r0; jump directly to null_target */
        "1:\n"
        "ldr r3, =g_null_target\n"
        "ldr r3, [r3]\n"
        "bx r3\n"

        /* Force the assembler to place the literal pool immediately, so
         * `ldr r3, =literal` Thumb-1 pseudo-instructions above can reach
         * their pool entries (range is only +1020 bytes from PC). Without
         * this, growing the surrounding code can push the pool past the
         * LDR range, breaking the build with "offset out of range". */
        ".ltorg\n"
    );
}

/* ---- Pattern matching ---- */

static uint32_t *find_pattern(const uint8_t *pattern, size_t len, uint8_t *data, size_t size)
{
    for (size_t i = 0; i + len <= size; i += 2) {
        if (memcmp(data + i, pattern, len) == 0) {
            return (uint32_t *)(data + i);
        }
    }
    return NULL;
}

/* Pattern for sq_pushstring */
static const uint8_t sq_pushstring_pattern[] = {
    0x10, 0xb5,  /* PUSH {r4, lr} */
    0x82, 0xb0,  /* SUB sp, #8 */
    0xf9, 0xb1,  /* CBZ r1, +0x3e */
    0x04, 0x46,  /* MOV r4, r0 */
    0xd0, 0xf8, 0x94, 0x00,  /* LDR.W r0, [r0, #0x94] */
    0x11, 0xf0, 0x96, 0xf9   /* BL ... */
};

static void install_hook(uint8_t *data, size_t size)
{
    uint32_t *addr = find_pattern(sq_pushstring_pattern, sizeof(sq_pushstring_pattern), data, size);
    if (!addr) {
        fprintf(stderr, "[m2hook] sq_pushstring pattern not found!\n");
        return;
    }

    DBG("[m2hook] Found sq_pushstring at %p\n", addr);

    g_sq_pushstring_addr = (uintptr_t)addr;

    /* Save original prologue */
    memcpy(g_saved_prologue, addr, 8);

    /* Continue address is original + 8 with Thumb bit set */
    g_continue_addr = ((uintptr_t)addr + 8) | 1;

    /*
     * Calculate null branch target from CBZ instruction at offset 4.
     * CBZ r1, imm at 0xb1f9 -> imm5=31, i=0 -> offset = 31*2 = 0x3e
     * Target = (function + 4 + 4) + 0x3e = function + 0x46
     */
    g_null_target = ((uintptr_t)addr + 0x46) | 1;

    DBG("[m2hook] continue_addr = 0x%x, null_target = 0x%x\n",
            (unsigned)g_continue_addr, (unsigned)g_null_target);

    /* Now overwrite the original function's first 8 bytes with jump to our hook */
    uintptr_t page_start = (uintptr_t)addr & ~0xFFF;
    if (mprotect((void *)page_start, 0x2000, PROT_READ | PROT_WRITE | PROT_EXEC) != 0) {
        perror("[m2hook] mprotect original failed");
        return;
    }

    /* Write: LDR.W PC, [PC, #0] ; .word hook_addr
     * F8DF F000 = LDR.W PC, [PC, #0]
     * Then 4 bytes of address
     *
     * Note: GCC generates an ARM veneer for thumb functions, so we use the
     * veneer address WITHOUT thumb bit. The veneer will jump to the actual
     * thumb code.
     */
    uint16_t *thumb = (uint16_t *)addr;
    uint32_t hook_addr = (uintptr_t)hook_sq_pushstring_asm;  /* ARM veneer, no Thumb bit */

    thumb[0] = 0xF8DF;
    thumb[1] = 0xF000;
    thumb[2] = hook_addr & 0xFFFF;
    thumb[3] = (hook_addr >> 16) & 0xFFFF;

    mprotect((void *)page_start, 0x2000, PROT_READ | PROT_EXEC);
    __builtin___clear_cache((char *)addr, (char *)addr + 8);

    DBG("[m2hook] Hook installed: %p -> 0x%x\n", addr, hook_addr);
}

/* ---- Memory map parsing ---- */

static void find_code_section(uintptr_t *addr, size_t *size)
{
    char path[64];
    snprintf(path, sizeof(path), "/proc/%d/maps", getpid());

    *addr = 0;
    *size = 0;

    FILE *f = fopen(path, "r");
    if (!f) return;

    char line[256];
    while (fgets(line, sizeof(line), f)) {
        if (strstr(line, "r-xp") && strstr(line, "m2engage")) {
            unsigned long start, end;
            if (sscanf(line, "%lx-%lx", &start, &end) == 2) {
                *addr = start;
                *size = end - start;
                break;
            }
        }
    }
    fclose(f);
}

/* ---- Constructor ---- */

__attribute__((constructor))
static void m2hook_init(void)
{
    /* Line-buffer stderr so each fprintf gets flushed at the newline.
     * Without this, gameapp's `2>&1 >> file` makes stderr block-buffered
     * and crash-loss eats our trailing diagnostics. */
    setvbuf(stderr, NULL, _IOLBF, 0);

    /* Check for debug mode via environment variable */
    const char *debug_env = getenv("M2HOOK_DEBUG");
    if (debug_env && (debug_env[0] == '1' || debug_env[0] == 'y' || debug_env[0] == 'Y')) {
        g_debug = 1;
    }

    DBG("[m2hook] Initializing...\n");

    char exe[256] = {0};
    ssize_t len = readlink("/proc/self/exe", exe, sizeof(exe) - 1);
    if (len > 0) exe[len] = 0;

    if (!strstr(exe, "m2engage")) {
        DBG("[m2hook] Not m2engage (%s), skipping\n", exe);
        return;
    }

#if ENABLE_FOLDER_HOOK
    trace_event("startup executable=%s", exe);
    trace_mounts("before startup binds");
    /* Bind-mount the active pack's PSBs over /usr/game/040 BEFORE the
     * engine reads them. parse_current gives us "<lineup>/<dir>"; if the
     * pack exists on disk, bind its three PSBs (title_mode_top, title_prof,
     * title_jp_titleselect_<lineup>) over the live paths. State files are
     * bound lazily on first do_folder_swap. */
    {
        char init_lineup[8], init_dir[64];
        if (parse_current(init_lineup, sizeof(init_lineup), init_dir, sizeof(init_dir)) == 0
            && valid_lineup(init_lineup) && valid_dir_name(init_dir)) {
            int failures = 0;
            char src[320], dst[320], motion_basename[64];
            snprintf(src, sizeof(src), "%s/%s/%s/title_mode_top.psb.m",
                     FOLDERS_ROOT, init_lineup, init_dir);
            snprintf(dst, sizeof(dst), "%s/config/title_mode_top.psb.m", LIVE_DIR);
            failures += bind_mount_file(src, dst) != 0;
            snprintf(src, sizeof(src), "%s/%s/%s/title_prof.psb.m",
                     FOLDERS_ROOT, init_lineup, init_dir);
            snprintf(dst, sizeof(dst), "%s/config/title_prof.psb.m", LIVE_DIR);
            failures += bind_mount_file(src, dst) != 0;
            snprintf(motion_basename, sizeof(motion_basename), MOTION_SHEET_FMT, init_lineup);
            snprintf(src, sizeof(src), "%s/%s/%s/%s",
                     FOLDERS_ROOT, init_lineup, init_dir, motion_basename);
            snprintf(dst, sizeof(dst), "%s/motion/%s", LIVE_DIR, motion_basename);
            failures += bind_mount_file(src, dst) != 0;
            /* Bind active pack's state save files + splice SRAM. */
            char init_saves[280];
            snprintf(init_saves, sizeof(init_saves), "%s/%s/%s/saves",
                     FOLDERS_ROOT, init_lineup, init_dir);
            failures += bind_state_files(init_saves, LIVE_SAVE_DIR) != 0;
            char init_sram[280];
            snprintf(init_sram, sizeof(init_sram), "%s/sram.bin", init_saves);
            char target[80];
            snprintf(target, sizeof(target), "%s/%s", init_lineup, init_dir);
            if (live_sram_is_current(target)) {
                trace_event("startup SRAM: preserve newer live save for %s", target);
                failures += write_pack_marker(LIVE_SRAM_PACK, target) != 0;
            } else {
                int restored = sram_splice_in(init_sram);
                failures += restored != 0;
                if (!restored) failures += write_pack_marker(LIVE_SRAM_PACK, target) != 0;
            }
            fprintf(stderr, "[m2hook] init: active pack %s/%s, setup failures=%d\n",
                    init_lineup, init_dir, failures);
        } else {
            fprintf(stderr, "[m2hook] init: invalid pack %s/%s; startup binds skipped\n",
                    init_lineup, init_dir);
        }
    }
    trace_mounts("after startup binds");
#endif

    uintptr_t code_addr;
    size_t code_size;
    find_code_section(&code_addr, &code_size);

    if (!code_addr) {
        fprintf(stderr, "[m2hook] Could not find code section\n");
        return;
    }

    DBG("[m2hook] Code section: 0x%x - 0x%x\n",
            (unsigned)code_addr, (unsigned)(code_addr + code_size));

    install_hook((uint8_t *)code_addr, code_size);

    DBG("[m2hook] Init complete\n");
}
