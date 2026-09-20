/* The engine verifies data_008 against the MD5 stream in meta_008 (PSB).
 * Splicing SRAM must refresh that stream before native autoload can read it.
 * Keep the metadata inode and all other bytes intact; console save files may
 * be bind-mounted. Bounded stack storage, no heap or crypto dependency. */
#ifndef CHRONOS_SAVE_DIGEST_H
#define CHRONOS_SAVE_DIGEST_H
#include <errno.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>
#include "md5.h"

static uint32_t save_read_le(const uint8_t *p, unsigned width)
{
    uint32_t value = 0;
    for (unsigned i = 0; i < width; ++i) value |= (uint32_t)p[i] << (8*i);
    return value;
}

/* Decode a PSB unsigned array containing exactly one value. */
static int save_single_uint(const uint8_t *buf, size_t size, uint32_t at,
                            uint32_t *value)
{
    size_t pos = at;
    if (pos >= size || buf[pos] < 13 || buf[pos] > 16) return -1;
    unsigned width = buf[pos++] - 12;
    if (width > size-pos || save_read_le(buf+pos, width) != 1) return -1;
    pos += width;
    if (pos >= size) return -1;
    unsigned type = buf[pos++];
    if (type == 0) { *value = 0; return 0; }
    if (type < 13 || type > 16) return -1;
    width = type - 12;
    if (width > size-pos) return -1;
    *value = save_read_le(buf+pos, width);
    return 0;
}

static int save_digest_offset(const uint8_t *buf, size_t size, size_t *offset)
{
    uint32_t relative, length;
    if (size < 44 || memcmp(buf, "PSB\0", 4) || buf[5] || buf[6] || buf[7]
        || (buf[4] != 2 && buf[4] != 3)) return -1;
    if (save_single_uint(buf, size, save_read_le(buf+24, 4), &relative)
        || save_single_uint(buf, size, save_read_le(buf+28, 4), &length)
        || length != MD5_DIGEST_SIZE) return -1;
    uint32_t base = save_read_le(buf+32, 4);
    if (base < 44 || base > size || relative > size-base
        || length > size-base-relative) return -1;
    *offset = (size_t)base + relative;
    return 0;
}

static int save_digest_process(const char *data_path, const char *meta_path, int update)
{
    uint8_t buf[4096], digest[MD5_DIGEST_SIZE], previous[MD5_DIGEST_SIZE];
    struct stat st;
    int meta = open(meta_path, update ? O_RDWR : O_RDONLY);
    if (meta < 0) return -1;
    int data = -1, result = -1;
    size_t offset;
    if (fstat(meta, &st) < 0) goto done;
    if (st.st_size < 44 || st.st_size > (off_t)sizeof(buf)) {
        errno = EINVAL;
        goto done;
    }
    size_t got = 0;
    while (got < (size_t)st.st_size) {
        ssize_t n = read(meta, buf+got, (size_t)st.st_size-got);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) { if (!n) errno = EIO; goto done; }
        got += (size_t)n;
    }
    if (save_digest_offset(buf, got, &offset)) { errno = EINVAL; goto done; }
    memcpy(previous, buf + offset, sizeof(previous));
    data = open(data_path, O_RDONLY);
    if (data < 0) goto done;
    struct md5_state hash;
    md5_init(&hash);
    for (;;) {
        ssize_t n = read(data, buf, sizeof(buf));
        if (n < 0 && errno == EINTR) continue;
        if (n < 0) goto done;
        if (!n) break;
        md5_update(&hash, buf, (unsigned)n);
    }
    md5_final(&hash, digest);
    if (!memcmp(previous, digest, sizeof(digest))) { result = 1; goto done; }
    if (!update) { result = 0; goto done; }
    for (got = 0; got < sizeof(digest);) {
        ssize_t n = pwrite(meta, digest+got, sizeof(digest)-got, (off_t)(offset+got));
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) { if (!n) errno = EIO; goto done; }
        got += (size_t)n;
    }
    result = fsync(meta) == 0 ? 1 : -1;
done:
    {
        int saved_errno = errno;
        if (data >= 0) close(data);
        close(meta);
        errno = saved_errno;
    }
    return result;
}

static int save_digest_refresh(const char *data_path, const char *meta_path)
{
    return save_digest_process(data_path, meta_path, 1) < 0 ? -1 : 0;
}
#endif
