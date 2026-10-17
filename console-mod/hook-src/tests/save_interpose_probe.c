/* Real Linux libc calls, synthetic files only. Run in an isolated container. */
#define _GNU_SOURCE
#include <assert.h>
#include <dirent.h>
#include <stdio.h>
#include <string.h>
#include "save_interpose.h"

static const char *A = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
static const char *B = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
static const char *native = "/usr/game//save/data_012_0004.bin";

static void directory(const char *path)
{
    assert(!mkdir(path, 0755) || errno == EEXIST);
}
static void write_text(const char *path, const char *text)
{
    FILE *file = fopen(path, "wb"); assert(file);
    assert(fwrite(text, 1, strlen(text), file) == strlen(text)); assert(!fclose(file));
}
static void check_text(const char *path, const char *text)
{
    char buffer[100] = {0}; FILE *file = fopen(path, "rb"); assert(file);
    size_t n = fread(buffer, 1, sizeof(buffer) - 1, file); assert(!fclose(file));
    assert(n == strlen(text) && !memcmp(buffer, text, n));
}
static void binding(unsigned at, const char *id, const char *relative, unsigned segment)
{
    char index[8], type[8]; snprintf(index, sizeof(index), "%u", at);
    snprintf(type, sizeof(type), "%u", segment);
    assert(!save_route_bind(&g_save_routes, index, id, relative, "g000a", type));
    ++g_save_bypass;
    int fd = save_files_game_directory(g_save_routes.library, relative, 1); assert(fd >= 0);
    assert(!save_files_identity(fd, id, 1)); close(fd);
    --g_save_bypass;
}
static unsigned count_fds(void)
{
    DIR *entries = opendir("/proc/self/fd"); assert(entries);
    unsigned count = 0;
    while (readdir(entries)) ++count;
    closedir(entries); return count;
}
int main(void)
{
    directory("/usr/game"); directory("/usr/game/save");
    directory("/tmp/chronos-save-probe"); directory("/tmp/chronos-save-probe/jp");
    directory("/tmp/chronos-save-probe/jp/A"); directory("/tmp/chronos-save-probe/us");
    directory("/tmp/chronos-save-probe/us/B");
    write_text(native, "legacy-underlay");
    snprintf(g_save_routes.library, sizeof(g_save_routes.library), "/tmp/chronos-save-probe");
    snprintf(g_save_routes.live_alias, sizeof(g_save_routes.live_alias), "/usr/game/save");
    g_save_routes.enabled = 1;
    binding(1, A, "jp/A", 12); binding(51, B, "us/B", 11);
    FILE *file = fopen64(native, "wb"); assert(file);
    assert(fwrite("stable", 1, 6, file) == 6); assert(!fclose(file));
    check_text("/tmp/chronos-save-probe/jp/A/saves/state_0.bin", "stable");
    ++g_save_bypass; check_text(native, "legacy-underlay"); --g_save_bypass;
    struct stat st; struct stat64 st64;
    assert(!stat(native, &st) && st.st_size == 6);
    assert(!__xstat64(3, native, &st64) && st64.st_size == 6);
    assert(!lstat64(native, &st64) && st64.st_size == 6);
    assert(!access(native, F_OK));
    int fd = open64(native, O_RDONLY); assert(fd >= 0);
    char buffer[8] = {0}; assert(read(fd, buffer, sizeof(buffer)) == 6); close(fd);
    assert(!strcmp(buffer, "stable"));
    assert(!chdir("/usr/game"));
    write_text("./save//meta_012_0004.bin", "metadata");
    check_text("/tmp/chronos-save-probe/jp/A/saves/state_0_meta.bin", "metadata");
    int native_directory = open("/usr/game/save", O_RDONLY | O_DIRECTORY); assert(native_directory >= 0);
    assert(!fstatat64(native_directory, "data_012_0004.bin", &st64, 0) && st64.st_size == 6);
    fd = openat64(native_directory, "data_012_0005.bin", O_WRONLY | O_CREAT | O_TRUNC, 0644);
    assert(fd >= 0); assert(write(fd, "slot-one", 8) == 8); close(fd);
    check_text("/tmp/chronos-save-probe/jp/A/saves/state_1.bin", "slot-one");
    close(native_directory);
    char temporary[] = "/usr/game/save/data_012_0004.bin.XXXXXX";
    fd = mkstemp(temporary); assert(fd >= 0); assert(write(fd, "atomic", 6) == 6); close(fd);
    assert(!access(temporary, F_OK)); assert(!rename(temporary, native));
    check_text("/tmp/chronos-save-probe/jp/A/saves/state_0.bin", "atomic");
    assert(!remove("save/meta_012_0004.bin"));
    assert(access("/tmp/chronos-save-probe/jp/A/saves/state_0_meta.bin", F_OK) == -1 && errno == ENOENT);
    assert(rename(native, "/usr/game/save/data_011_0204.bin") == -1 && errno == EXDEV);
    assert(!fopen("save/data_012_0000.bin", "wb") && errno == ENOENT);
    assert(!fopen("save/save/data_012_0004.bin", "wb") && errno == EPERM);
    write_text("/tmp/protected-save-probe", "protected");
    assert(!symlink("/tmp/protected-save-probe", "/tmp/chronos-save-probe/jp/A/saves/state_2.bin"));
    assert(!fopen("save/data_012_0006.bin", "wb") && errno == ELOOP);
    check_text("/tmp/protected-save-probe", "protected");
    pthread_mutex_lock(&g_save_mutex);
    memset(g_save_routes.bindings, 0, sizeof(g_save_routes.bindings));
    binding(2, A, "jp/A", 12);
    pthread_mutex_unlock(&g_save_mutex);
    assert(!fopen(native, "rb") && errno == ENOENT);
    check_text("save/data_012_0008.bin", "atomic");
    unsigned before = count_fds();
    for (unsigned i = 0; i < 1000; ++i) {
        fd = open64("save/data_012_0008.bin", O_RDONLY); assert(fd >= 0); close(fd);
        assert(!__xstat64(3, "save/data_012_0008.bin", &st64));
    }
    assert(count_fds() == before);
    /* A validated FD stays on its original game if paths change before IO. */
    struct save_io_target anchored;
    assert(save_io_map(AT_FDCWD, "save/data_012_0008.bin", &anchored) == 1);
    assert(!rename("/tmp/chronos-save-probe/jp/A", "/tmp/chronos-save-probe/jp/Parked"));
    directory("/tmp/chronos-save-probe/jp/A");
    fd = save_real_openat(anchored.directory, anchored.leaf, O_WRONLY | O_TRUNC | O_NOFOLLOW, 0644);
    assert(fd >= 0); assert(write(fd, "anchored", 8) == 8); close(fd); save_io_close(&anchored);
    check_text("/tmp/chronos-save-probe/jp/Parked/saves/state_0.bin", "anchored");
    assert(access("/tmp/chronos-save-probe/jp/A/saves/state_0.bin", F_OK) == -1);
    puts("Linux libc routing passed: open64/fopen64/xstat64/at/aliases/temp/rename/remove/symlink/holes/reorder/FD anchors");
    return 0;
}
