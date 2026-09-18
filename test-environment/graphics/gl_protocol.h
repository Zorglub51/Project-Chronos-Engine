/*
 * gl_protocol.h - Shared protocol for GL proxy communication
 *
 * Defines the shared memory layout, command IDs, and ring buffer helpers
 * used between the armhf libMali wrapper and the native aarch64 GL proxy.
 */
#ifndef GL_PROTOCOL_H
#define GL_PROTOCOL_H

#include <stdint.h>
#include <string.h>
#include <unistd.h>
#include <linux/futex.h>
#include <sys/syscall.h>
#include <time.h>
#include <limits.h>

#define GL_SHM_NAME     "/m2e_gl"
#define RING_SIZE       (32 * 1024 * 1024)  /* 32MB command ring */
#define RESPONSE_SIZE   (4 * 1024 * 1024)   /* 4MB response area (enough for 1280x720 RGBA) */

/* Command flags */
#define CMD_FLAG_SYNC   0x0001  /* Requires response from proxy */

/* Command IDs - EGL */
enum {
    GL_CMD_NOP = 0,
    GL_CMD_SWAP_BUFFERS,            /* eglSwapBuffers */

    /* State */
    GL_CMD_ENABLE = 10,
    GL_CMD_DISABLE,
    GL_CMD_BLEND_FUNC,
    GL_CMD_BLEND_FUNC_SEPARATE,
    GL_CMD_BLEND_EQUATION,
    GL_CMD_BLEND_EQUATION_SEPARATE,
    GL_CMD_DEPTH_FUNC,
    GL_CMD_DEPTH_MASK,
    GL_CMD_DEPTH_RANGEF,
    GL_CMD_COLOR_MASK,
    GL_CMD_CULL_FACE,
    GL_CMD_FRONT_FACE,
    GL_CMD_SCISSOR,
    GL_CMD_VIEWPORT,
    GL_CMD_CLEAR_COLOR,
    GL_CMD_CLEAR,
    GL_CMD_CLEAR_DEPTHF,
    GL_CMD_CLEAR_STENCIL,
    GL_CMD_STENCIL_FUNC,
    GL_CMD_STENCIL_MASK,
    GL_CMD_STENCIL_OP,
    GL_CMD_PIXEL_STOREI,
    GL_CMD_ACTIVE_TEXTURE,
    GL_CMD_FLUSH,
    GL_CMD_FINISH,
    GL_CMD_LINE_WIDTH,

    /* Textures */
    GL_CMD_GEN_TEXTURES = 50,       /* sync */
    GL_CMD_DELETE_TEXTURES,
    GL_CMD_BIND_TEXTURE,
    GL_CMD_TEX_IMAGE_2D,
    GL_CMD_TEX_SUB_IMAGE_2D,
    GL_CMD_TEX_PARAMETERI,
    GL_CMD_GENERATE_MIPMAP,
    GL_CMD_COPY_TEX_IMAGE_2D,
    GL_CMD_COPY_TEX_SUB_IMAGE_2D,
    GL_CMD_COMPRESSED_TEX_IMAGE_2D,

    /* Framebuffers */
    GL_CMD_GEN_FRAMEBUFFERS = 70,   /* sync */
    GL_CMD_DELETE_FRAMEBUFFERS,
    GL_CMD_BIND_FRAMEBUFFER,
    GL_CMD_FRAMEBUFFER_TEXTURE_2D,
    GL_CMD_CHECK_FRAMEBUFFER_STATUS, /* sync */
    GL_CMD_READ_PIXELS,              /* sync */

    /* Renderbuffers */
    GL_CMD_GEN_RENDERBUFFERS = 80,   /* sync */
    GL_CMD_DELETE_RENDERBUFFERS,
    GL_CMD_BIND_RENDERBUFFER,
    GL_CMD_RENDERBUFFER_STORAGE,
    GL_CMD_FRAMEBUFFER_RENDERBUFFER,

    /* Shaders */
    GL_CMD_CREATE_SHADER = 100,      /* sync */
    GL_CMD_DELETE_SHADER,
    GL_CMD_SHADER_SOURCE,
    GL_CMD_COMPILE_SHADER,
    GL_CMD_GET_SHADERIV,             /* sync */
    GL_CMD_GET_SHADER_INFO_LOG,      /* sync */
    GL_CMD_ATTACH_SHADER,
    GL_CMD_GET_ATTACHED_SHADERS,     /* sync */

    /* Programs */
    GL_CMD_CREATE_PROGRAM = 120,     /* sync */
    GL_CMD_DELETE_PROGRAM,
    GL_CMD_USE_PROGRAM,
    GL_CMD_LINK_PROGRAM,
    GL_CMD_GET_PROGRAMIV,            /* sync */
    GL_CMD_GET_PROGRAM_INFO_LOG,     /* sync */
    GL_CMD_GET_UNIFORM_LOCATION,     /* sync */
    GL_CMD_GET_ATTRIB_LOCATION,      /* sync */

    /* Uniforms */
    GL_CMD_UNIFORM_1I = 140,
    GL_CMD_UNIFORM_1F,
    GL_CMD_UNIFORM_2F,
    GL_CMD_UNIFORM_3F,
    GL_CMD_UNIFORM_4F,
    GL_CMD_UNIFORM_4FV,
    GL_CMD_UNIFORM_MATRIX_4FV,

    /* Vertex arrays / Buffers */
    GL_CMD_GEN_BUFFERS = 160,        /* sync */
    GL_CMD_DELETE_BUFFERS,
    GL_CMD_BIND_BUFFER,
    GL_CMD_BUFFER_DATA,
    GL_CMD_BUFFER_SUB_DATA,
    GL_CMD_VERTEX_ATTRIB_POINTER,
    GL_CMD_ENABLE_VERTEX_ATTRIB_ARRAY,
    GL_CMD_DISABLE_VERTEX_ATTRIB_ARRAY,

    /* Draw */
    GL_CMD_DRAW_ARRAYS = 180,
    GL_CMD_DRAW_ELEMENTS,
    GL_CMD_UPLOAD_CLIENT_ARRAY,     /* upload client-side vertex data + set attrib pointer */
    GL_CMD_UPLOAD_INDEX_ARRAY,      /* upload client-side index data into temp EBO */

    /* Query */
    GL_CMD_GET_ERROR = 200,          /* sync */
    GL_CMD_GET_STRING,               /* sync */
    GL_CMD_GET_INTEGERV,             /* sync */
    GL_CMD_GET_FLOATV,               /* sync */

    GL_CMD_MAX
};

/* Command header - placed at start of each command in the ring */
struct cmd_header {
    uint16_t cmd_id;
    uint16_t flags;
    uint32_t size;      /* total size including header */
};

/* Shared memory layout */
struct gl_shm {
    /* Sync state */
    volatile uint32_t proxy_ready;      /* proxy sets to 1 when initialized */
    volatile uint32_t cmd_seq;          /* incremented by wrapper per sync cmd */
    volatile uint32_t ack_seq;          /* incremented by proxy per sync response */
    volatile uint32_t shutdown;         /* set to 1 to tell proxy to exit */

    /* Response area */
    uint32_t response_u32;              /* scalar return value */
    uint32_t response_size;             /* size of response data */
    uint8_t  response_data[RESPONSE_SIZE];

    /* Ring buffer */
    volatile uint32_t write_pos;        /* written by armhf wrapper */
    volatile uint32_t read_pos;         /* written by native proxy */
    uint8_t  ring[RING_SIZE];
};

/* Calculate offset of ring[] within gl_shm for mmap sizing */
#define GL_SHM_TOTAL_SIZE (sizeof(struct gl_shm))

/*
 * Ring buffer helpers
 *
 * The ring is treated as a circular buffer with wrap-around.
 * Commands must not straddle the ring boundary - if a command doesn't fit
 * at the current write_pos, we insert a NOP skip and wrap to 0.
 */

static inline uint32_t ring_space_available(volatile struct gl_shm *shm) {
    uint32_t wp = shm->write_pos;
    uint32_t rp = shm->read_pos;
    if (wp >= rp)
        return RING_SIZE - (wp - rp) - 1;
    else
        return rp - wp - 1;
}

/* Write raw bytes to ring at given position, handling wrap */
static inline void ring_write(struct gl_shm *shm, uint32_t pos, const void *data, uint32_t len) {
    uint32_t p = pos % RING_SIZE;
    uint32_t first = RING_SIZE - p;
    if (first >= len) {
        memcpy(&shm->ring[p], data, len);
    } else {
        memcpy(&shm->ring[p], data, first);
        memcpy(&shm->ring[0], (const uint8_t*)data + first, len - first);
    }
}

/* Read raw bytes from ring at given position, handling wrap */
static inline void ring_read(const struct gl_shm *shm, uint32_t pos, void *data, uint32_t len) {
    uint32_t p = pos % RING_SIZE;
    uint32_t first = RING_SIZE - p;
    if (first >= len) {
        memcpy(data, &shm->ring[p], len);
    } else {
        memcpy(data, &shm->ring[p], first);
        memcpy((uint8_t*)data + first, &shm->ring[0], len - first);
    }
}

/* Helper to compute pixel data size */
static inline uint32_t pixel_size(uint32_t format, uint32_t type) {
    int components = 4;
    switch (format) {
        case 0x1906: components = 1; break; /* GL_ALPHA */
        case 0x1909: components = 1; break; /* GL_LUMINANCE */
        case 0x190A: components = 2; break; /* GL_LUMINANCE_ALPHA */
        case 0x1907: components = 3; break; /* GL_RGB */
        case 0x1908: components = 4; break; /* GL_RGBA */
        default:     components = 4; break;
    }
    int type_size = 1;
    switch (type) {
        case 0x1401: type_size = 1; break; /* GL_UNSIGNED_BYTE */
        case 0x8363: type_size = 2; break; /* GL_UNSIGNED_SHORT_5_6_5 */
        case 0x8033: type_size = 2; break; /* GL_UNSIGNED_SHORT_4_4_4_4 */
        case 0x8034: type_size = 2; break; /* GL_UNSIGNED_SHORT_5_5_5_1 */
        default:     type_size = 1; break;
    }
    /* For packed types, return type_size directly */
    if (type_size > 1) return type_size;
    return components * type_size;
}

/* Align size to 4-byte boundary */
static inline uint32_t align4(uint32_t v) {
    return (v + 3) & ~3u;
}

/*
 * Futex-based signaling (works across processes via shared memory)
 *
 * These replace eventfds for cross-process wake/wait.
 * The futex word is the atomic variable itself in shared memory.
 */

/* Wake one waiter on a shared futex word */
static inline void futex_wake(volatile uint32_t *addr) {
    syscall(SYS_futex, addr, FUTEX_WAKE, 1, NULL, NULL, 0);
}

/* Wait until *addr != expected_val, with timeout in ms (0 = no timeout) */
static inline void futex_wait(volatile uint32_t *addr, uint32_t expected_val, int timeout_ms) {
    if (timeout_ms > 0) {
        struct timespec ts = { .tv_sec = timeout_ms / 1000,
                               .tv_nsec = (timeout_ms % 1000) * 1000000L };
        syscall(SYS_futex, addr, FUTEX_WAIT, expected_val, &ts, NULL, 0);
    } else {
        syscall(SYS_futex, addr, FUTEX_WAIT, expected_val, NULL, NULL, 0);
    }
}

#endif /* GL_PROTOCOL_H */
