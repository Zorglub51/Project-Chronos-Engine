/* Bounded SRAM transfers. No retained cache: compare the actual disk bytes.
 * Return 0 when unchanged, 1 when written, -1 on error. */
#ifndef CHRONOS_SAVE_SLICE_H
#define CHRONOS_SAVE_SLICE_H
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static int slice_read(int fd, void *buf, size_t size, off_t at, int pad)
{
    size_t done = 0;
    while (done < size) {
        ssize_t n = fd < 0 ? 0 : pread(fd, (char *)buf + done, size - done, at + done);
        if (n < 0 && errno == EINTR) continue;
        if (n < 0) return -1;
        if (!n) {
            if (!pad) { errno = EIO; return -1; }
            memset((char *)buf + done, 0, size - done);
            break;
        }
        done += (size_t)n;
    }
    return 0;
}

static int slice_write(int fd, const void *buf, size_t size, off_t at)
{
    size_t done = 0;
    while (done < size) {
        ssize_t n = pwrite(fd, (const char *)buf + done, size - done, at + done);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) { if (!n) errno = EIO; return -1; }
        done += (size_t)n;
    }
    return 0;
}

static int slice_equal(int a, off_t a_at, int b, off_t b_at, size_t length, int pad_b)
{
    char left[16384], right[16384];
    for (size_t pos = 0; pos < length;) {
        size_t n = length - pos;
        if (n > sizeof(left)) n = sizeof(left);
        if (slice_read(a, left, n, a_at + pos, 0) ||
            slice_read(b, right, n, b_at + pos, pad_b)) return -1;
        if (memcmp(left, right, n)) return 0;
        pos += n;
    }
    return 1;
}

static int save_slice_export(const char *live, const char *target, off_t at, size_t length)
{
    int src = open(live, O_RDONLY);
    if (src < 0) return errno == ENOENT ? 0 : -1;
    int dst = -1, result = -1;
    char tmp[512] = {0}, parent[512];
    struct stat st;
    if (fstat(src, &st) || st.st_size < at + (off_t)length) { errno = EIO; goto done; }
    dst = open(target, O_RDONLY);
    if (dst < 0 && errno != ENOENT) goto done;
    if (dst >= 0) {
        if (fstat(dst, &st)) goto done;
        if (st.st_size == (off_t)length) {
            int equal = slice_equal(src, at, dst, 0, length, 0);
            if (equal < 0) goto done;
            if (equal) { result = 0; goto done; }
        }
        close(dst); dst = -1;
    }
    if (snprintf(tmp, sizeof(tmp), "%s.tmp", target) >= (int)sizeof(tmp) ||
        snprintf(parent, sizeof(parent), "%s", target) >= (int)sizeof(parent)) {
        tmp[0] = 0; errno = ENAMETOOLONG; goto done;
    }
    char *slash = strrchr(parent, '/');
    if (!slash) strcpy(parent, ".");
    else if (slash == parent) slash[1] = 0;
    else *slash = 0;
    dst = open(tmp, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (dst < 0) goto done;
    char buf[65536];
    for (size_t pos = 0; pos < length;) {
        size_t n = length - pos;
        if (n > sizeof(buf)) n = sizeof(buf);
        if (slice_read(src, buf, n, at + pos, 0) || slice_write(dst, buf, n, pos)) goto done;
        pos += n;
    }
    if (fsync(dst)) goto done;
    if (close(dst)) { dst = -1; goto done; }
    dst = -1;
    if (rename(tmp, target)) goto done;
    tmp[0] = 0;
    dst = open(parent, O_RDONLY);
    if (dst < 0 || fsync(dst)) goto done;
    result = 1;
done:
    {
        int error = errno;
        if (dst >= 0) close(dst);
        close(src);
        if (tmp[0]) unlink(tmp);
        errno = error;
    }
    return result;
}

static int save_slice_import(const char *live, const char *source, off_t at, size_t length)
{
    int src = open(source, O_RDONLY);
    if (src < 0 && errno != ENOENT) return -1;
    int dst = open(live, O_RDWR), result = -1, changed = 0;
    if (dst < 0) goto done;
    struct stat st;
    if (fstat(dst, &st) || st.st_size < at + (off_t)length) { errno = EIO; goto done; }
    char old[16384], next[16384];
    for (size_t pos = 0; pos < length;) {
        size_t n = length - pos;
        if (n > sizeof(old)) n = sizeof(old);
        if (slice_read(dst, old, n, at + pos, 0) || slice_read(src, next, n, pos, 1)) goto done;
        if (memcmp(old, next, n)) {
            if (slice_write(dst, next, n, at + pos)) goto done;
            changed = 1;
        }
        pos += n;
    }
    if (changed && fsync(dst)) goto done;
    result = changed;
done:
    {
        int error = errno;
        if (src >= 0) close(src);
        if (dst >= 0) close(dst);
        errno = error;
    }
    return result;
}
#endif
