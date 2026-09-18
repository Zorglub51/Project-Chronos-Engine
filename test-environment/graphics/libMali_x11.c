/*
 * libMali.so for m2engage on Linux aarch64 VM (GL proxy client)
 *
 * Instead of calling Mesa GL directly (which falls back to llvmpipe),
 * this writes GL commands to a shared memory ring buffer read by a native
 * aarch64 gl_proxy process that has real GPU access.
 *
 * Keeps: SIGBUS handler, file interceptions (open/openat/access/stat/ioctl/write/read).
 * Removed: Mesa EGL loading, X11 window creation, GL function pointers.
 */

#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <signal.h>
#include <stdarg.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/mman.h>
#include <sys/ioctl.h>
#include <linux/input.h>
#include <time.h>
#include <ucontext.h>
#include <sys/stat.h>
#include <errno.h>

/* We no longer include EGL/GLES headers for real calls - define the types we need */
typedef unsigned int GLenum;
typedef unsigned int GLbitfield;
typedef unsigned int GLuint;
typedef int GLint;
typedef int GLsizei;
typedef intptr_t GLsizeiptr;
typedef intptr_t GLintptr;
typedef unsigned char GLboolean;
typedef unsigned char GLubyte;
typedef float GLfloat;
typedef char GLchar;
typedef void* EGLDisplay;
typedef void* EGLSurface;
typedef void* EGLContext;
typedef void* EGLConfig;
typedef int EGLint;
typedef unsigned int EGLBoolean;
typedef unsigned int EGLenum;
typedef void* EGLNativeDisplayType;
typedef void* EGLNativeWindowType;
typedef void (*__eglMustCastToProperFunctionPointerType)(void);

#define EGL_TRUE 1
#define EGL_FALSE 0
#define EGL_NO_DISPLAY ((EGLDisplay)0)
#define EGL_NO_SURFACE ((EGLSurface)0)
#define EGL_NO_CONTEXT ((EGLContext)0)
#define EGL_NONE 0x3038
#define EGL_WIDTH 0x3057
#define EGL_HEIGHT 0x3058

#define GL_NO_ERROR 0
#define GL_ARRAY_BUFFER 0x8892

#include "gl_protocol.h"

/* Debug flag - set LIBMALI_DEBUG=1 to enable verbose logging */
static int g_debug = 0;
#define DBG(...) do { if (g_debug) fprintf(stderr, "[libMali] " __VA_ARGS__); } while(0)

/* Track main process PID to avoid issues in forked children */
static pid_t g_main_pid = 0;

/* FBO tracking (client-side) */
static GLuint g_current_fbo = 0;
static GLuint g_last_nonzero_fbo = 0;

#define RENDER_WIDTH 1280
#define RENDER_HEIGHT 720

/* Shared memory for GL proxy communication */
static struct gl_shm *g_shm = NULL;
static int g_proxy_connected = 0;

/* Fake EGL handles returned to m2engage */
static int g_fake_egl_display = 1;
static int g_fake_egl_surface = 1;
static int g_fake_egl_context = 1;
#define FAKE_EGL_DPY  ((EGLDisplay)&g_fake_egl_display)
#define FAKE_EGL_SURF ((EGLSurface)&g_fake_egl_surface)
#define FAKE_EGL_CTX  ((EGLContext)&g_fake_egl_context)

/* Forward declarations of GL wrappers needed by eglGetProcAddress */
void glBindFramebuffer(GLenum target, GLuint fb);

/* Static buffers for string returns */
static char g_gl_string_buf[4096];

/* Track current buffer bindings for client pointer detection */
static GLuint g_current_array_buffer = 0;
static GLuint g_current_element_buffer = 0;

/* Client-side vertex attrib tracking */
#define MAX_VERTEX_ATTRIBS 16
struct client_attrib {
    const void *ptr;       /* client-side pointer (only valid when no VBO was bound at set time) */
    GLint size;            /* components (1-4) */
    GLenum type;           /* GL_FLOAT, GL_UNSIGNED_BYTE, etc. */
    GLboolean normalized;
    GLsizei stride;        /* 0 means tightly packed */
    int is_client;         /* 1 if set without a VBO bound */
    int enabled;           /* tracked via Enable/DisableVertexAttribArray */
};
static struct client_attrib g_attribs[MAX_VERTEX_ATTRIBS];

static uint32_t gl_type_size(GLenum type) {
    switch (type) {
    case 0x1400: return 1; /* GL_BYTE */
    case 0x1401: return 1; /* GL_UNSIGNED_BYTE */
    case 0x1402: return 2; /* GL_SHORT */
    case 0x1403: return 2; /* GL_UNSIGNED_SHORT */
    case 0x1404: return 4; /* GL_INT */
    case 0x1405: return 4; /* GL_UNSIGNED_INT */
    case 0x1406: return 4; /* GL_FLOAT */
    case 0x140B: return 2; /* GL_HALF_FLOAT */
    default: return 4;
    }
}

/* ================================================================== */
/* GL Proxy connection                                                 */
/* ================================================================== */

static int connect_proxy(void) {
    if (g_proxy_connected) return 1;

    /* Open shared memory created by gl_proxy */
    int fd = shm_open(GL_SHM_NAME, O_RDWR, 0666);
    if (fd < 0) {
        fprintf(stderr, "[libMali] Cannot open shm %s: %s\n", GL_SHM_NAME, strerror(errno));
        return 0;
    }

    g_shm = mmap(NULL, GL_SHM_TOTAL_SIZE, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    close(fd);
    if (g_shm == MAP_FAILED) {
        fprintf(stderr, "[libMali] mmap failed: %s\n", strerror(errno));
        g_shm = NULL;
        return 0;
    }

    /* Wait for proxy to be ready */
    int retries = 0;
    while (!__atomic_load_n(&g_shm->proxy_ready, __ATOMIC_ACQUIRE)) {
        usleep(10000);
        if (++retries > 500) {
            fprintf(stderr, "[libMali] Timeout waiting for gl_proxy\n");
            return 0;
        }
    }

    g_proxy_connected = 1;
    DBG("Connected to gl_proxy: shm=%p\n", g_shm);
    return 1;
}

/* ================================================================== */
/* Ring buffer write helpers                                           */
/* ================================================================== */

/* Wait until there's enough space in the ring */
static void wait_ring_space(uint32_t needed) {
    while (ring_space_available(g_shm) < needed) {
        usleep(100);
    }
}

/* Command building state */
static uint32_t g_cmd_start;     /* where this command starts */
static uint32_t g_cmd_wp;        /* current write cursor within command */
static uint16_t g_cur_cmd_id;
static uint16_t g_cur_cmd_flags;

static void cmd_begin2(uint16_t cmd_id, uint16_t flags) {
    g_cur_cmd_id = cmd_id;
    g_cur_cmd_flags = flags;
    g_cmd_start = g_shm->write_pos;
    g_cmd_wp = g_cmd_start + sizeof(struct cmd_header);
}

static void cmd_u32(uint32_t v) {
    ring_write(g_shm, g_cmd_wp, &v, 4);
    g_cmd_wp += 4;
}

static void cmd_f32(float v) {
    union { float f; uint32_t u; } conv;
    conv.f = v;
    cmd_u32(conv.u);
}

static void cmd_data(const void *data, uint32_t len) {
    ring_write(g_shm, g_cmd_wp, data, len);
    g_cmd_wp += align4(len);
}

/* Finish command (fire-and-forget) */
static void cmd_end(void) {
    uint32_t size = g_cmd_wp - g_cmd_start;
    size = align4(size);

    /* Ensure ring has space */
    wait_ring_space(size);

    /* Write header at start */
    struct cmd_header hdr;
    hdr.cmd_id = g_cur_cmd_id;
    hdr.flags = g_cur_cmd_flags;
    hdr.size = size;
    ring_write(g_shm, g_cmd_start, &hdr, sizeof(hdr));

    /* Advance write pointer and wake proxy */
    __atomic_store_n(&g_shm->write_pos, g_cmd_start + size, __ATOMIC_RELEASE);
    futex_wake(&g_shm->write_pos);
}

/* Finish command and wait for response (sync call) */
static uint32_t cmd_end_sync(void) {
    g_cur_cmd_flags |= CMD_FLAG_SYNC;

    uint32_t size = g_cmd_wp - g_cmd_start;
    size = align4(size);

    wait_ring_space(size);

    /* Write header */
    struct cmd_header hdr;
    hdr.cmd_id = g_cur_cmd_id;
    hdr.flags = g_cur_cmd_flags;
    hdr.size = size;
    ring_write(g_shm, g_cmd_start, &hdr, sizeof(hdr));

    /* Set sequence number */
    uint32_t seq = __atomic_add_fetch(&g_shm->cmd_seq, 1, __ATOMIC_ACQ_REL);

    /* Advance write pointer and wake proxy via futex */
    __atomic_store_n(&g_shm->write_pos, g_cmd_start + size, __ATOMIC_RELEASE);
    futex_wake(&g_shm->write_pos);

    /* Wait for ack via futex on ack_seq */
    while (__atomic_load_n(&g_shm->ack_seq, __ATOMIC_ACQUIRE) != seq) {
        uint32_t cur_ack = __atomic_load_n(&g_shm->ack_seq, __ATOMIC_ACQUIRE);
        if (cur_ack != seq) {
            futex_wait(&g_shm->ack_seq, cur_ack, 10);
        }
    }

    return g_shm->response_u32;
}

/* Fire-and-forget shorthand: begin + args + end */
#define CMD_FIRE(cmd_id) do { cmd_begin2(cmd_id, 0); } while(0)
#define CMD_END() cmd_end()

/* Sync shorthand: begin + args + end_sync */
#define CMD_SYNC(cmd_id) do { cmd_begin2(cmd_id, CMD_FLAG_SYNC); } while(0)

/* ================================================================== */
/* SIGBUS handler for unaligned ARM access - includes Thumb-2 support  */
/* ================================================================== */

static volatile int g_sigbus_count = 0;

static void sigbus_handler(int sig, siginfo_t *info, void *ucontext) {
    ucontext_t *uc = (ucontext_t *)ucontext;
    unsigned long pc = uc->uc_mcontext.arm_pc;
    unsigned long *regs = &uc->uc_mcontext.arm_r0;
    unsigned char *fault_addr = (unsigned char *)info->si_addr;

    g_sigbus_count++;
    if (g_debug && (g_sigbus_count <= 10 || (g_sigbus_count % 10000) == 0)) {
        fprintf(stderr, "[libMali] SIGBUS #%d at PC=0x%lx addr=%p\n",
                g_sigbus_count, pc, info->si_addr);
    }

    /* Read the faulting ARM32 instruction */
    unsigned int insn = *(unsigned int *)pc;

    /* Decode ARM LDR/STR instructions */
    int is_load_store = ((insn >> 26) & 0x3) == 0x1;
    int is_load = (insn >> 20) & 1;
    int rd = (insn >> 12) & 0xF;

    if (is_load_store && is_load && rd < 15) {
        unsigned int val = fault_addr[0] | (fault_addr[1] << 8) |
                          (fault_addr[2] << 16) | (fault_addr[3] << 24);
        regs[rd] = val;
        uc->uc_mcontext.arm_pc = pc + 4;
        return;
    }

    if (is_load_store && !is_load && rd < 15) {
        unsigned int val = regs[rd];
        fault_addr[0] = val & 0xFF;
        fault_addr[1] = (val >> 8) & 0xFF;
        fault_addr[2] = (val >> 16) & 0xFF;
        fault_addr[3] = (val >> 24) & 0xFF;
        uc->uc_mcontext.arm_pc = pc + 4;
        return;
    }

    /* LDRH/STRH */
    int is_misc_ls = ((insn >> 25) & 0x7) == 0x0 && ((insn >> 4) & 0x9) == 0x9;
    if (is_misc_ls) {
        int sh = (insn >> 5) & 0x3;
        int is_ld = (insn >> 20) & 1;
        rd = (insn >> 12) & 0xF;

        if (sh == 1 && is_ld && rd < 15) {
            unsigned short val = fault_addr[0] | (fault_addr[1] << 8);
            regs[rd] = val;
            uc->uc_mcontext.arm_pc = pc + 4;
            return;
        }
        if (sh == 1 && !is_ld && rd < 15) {
            unsigned short val = regs[rd];
            fault_addr[0] = val & 0xFF;
            fault_addr[1] = (val >> 8) & 0xFF;
            uc->uc_mcontext.arm_pc = pc + 4;
            return;
        }
        if (sh == 3 && is_ld && rd < 15) {
            short val = (short)(fault_addr[0] | (fault_addr[1] << 8));
            regs[rd] = (unsigned long)(long)val;
            uc->uc_mcontext.arm_pc = pc + 4;
            return;
        }
    }

    /* LDRD/STRD */
    if (((insn >> 25) & 0x7) == 0x0 && ((insn >> 4) & 0xF) == 0xD) {
        rd = (insn >> 12) & 0xF;
        if (rd < 14 && (rd & 1) == 0) {
            regs[rd] = fault_addr[0] | (fault_addr[1] << 8) |
                       (fault_addr[2] << 16) | (fault_addr[3] << 24);
            regs[rd+1] = fault_addr[4] | (fault_addr[5] << 8) |
                         (fault_addr[6] << 16) | (fault_addr[7] << 24);
            uc->uc_mcontext.arm_pc = pc + 4;
            return;
        }
    }
    if (((insn >> 25) & 0x7) == 0x0 && ((insn >> 4) & 0xF) == 0xF) {
        rd = (insn >> 12) & 0xF;
        if (rd < 14 && (rd & 1) == 0) {
            unsigned int v0 = regs[rd], v1 = regs[rd+1];
            fault_addr[0] = v0 & 0xFF; fault_addr[1] = (v0 >> 8) & 0xFF;
            fault_addr[2] = (v0 >> 16) & 0xFF; fault_addr[3] = (v0 >> 24) & 0xFF;
            fault_addr[4] = v1 & 0xFF; fault_addr[5] = (v1 >> 8) & 0xFF;
            fault_addr[6] = (v1 >> 16) & 0xFF; fault_addr[7] = (v1 >> 24) & 0xFF;
            uc->uc_mcontext.arm_pc = pc + 4;
            return;
        }
    }

    /* Thumb-2 instruction handling */
    unsigned long cpsr = uc->uc_mcontext.arm_cpsr;
    int is_thumb = (cpsr >> 5) & 1;

    if (is_thumb) {
        unsigned short *thumb_pc = (unsigned short *)(pc & ~1UL);
        unsigned short hw1 = thumb_pc[0];
        unsigned short hw2 = thumb_pc[1];

        if (g_debug && g_sigbus_count <= 10) {
            fprintf(stderr, "[libMali] SIGBUS Thumb-2: hw1=0x%04x hw2=0x%04x at PC=0x%lx\n",
                    hw1, hw2, pc);
        }

        /* Thumb-2 LDRD/STRD */
        if ((hw1 & 0xFE50) == 0xE850 || (hw1 & 0xFE50) == 0xE840) {
            int is_ld_t2 = (hw1 >> 4) & 1;
            int rt_t2 = (hw2 >> 12) & 0xF;
            int rt2_t2 = (hw2 >> 8) & 0xF;

            if (is_ld_t2 && rt_t2 < 15 && rt2_t2 < 15) {
                regs[rt_t2] = fault_addr[0] | (fault_addr[1] << 8) |
                              (fault_addr[2] << 16) | (fault_addr[3] << 24);
                regs[rt2_t2] = fault_addr[4] | (fault_addr[5] << 8) |
                               (fault_addr[6] << 16) | (fault_addr[7] << 24);
                uc->uc_mcontext.arm_pc = pc + 4;
                return;
            }
            if (!is_ld_t2 && rt_t2 < 15 && rt2_t2 < 15) {
                unsigned int v0 = regs[rt_t2], v1 = regs[rt2_t2];
                fault_addr[0] = v0 & 0xFF; fault_addr[1] = (v0 >> 8) & 0xFF;
                fault_addr[2] = (v0 >> 16) & 0xFF; fault_addr[3] = (v0 >> 24) & 0xFF;
                fault_addr[4] = v1 & 0xFF; fault_addr[5] = (v1 >> 8) & 0xFF;
                fault_addr[6] = (v1 >> 16) & 0xFF; fault_addr[7] = (v1 >> 24) & 0xFF;
                uc->uc_mcontext.arm_pc = pc + 4;
                return;
            }
        }

        /* Thumb-2 LDR.W/STR.W */
        if ((hw1 & 0xFF00) == 0xF800 || (hw1 & 0xFF00) == 0xF850 ||
            (hw1 & 0xFFF0) == 0xF8D0 || (hw1 & 0xFFF0) == 0xF8C0) {
            int is_ld_w = (hw1 >> 4) & 1;
            rd = (hw2 >> 12) & 0xF;
            if (is_ld_w && rd < 15) {
                unsigned int val = fault_addr[0] | (fault_addr[1] << 8) |
                                  (fault_addr[2] << 16) | (fault_addr[3] << 24);
                regs[rd] = val;
                uc->uc_mcontext.arm_pc = pc + 4;
                return;
            }
            if (!is_ld_w && rd < 15) {
                unsigned int val = regs[rd];
                fault_addr[0] = val & 0xFF; fault_addr[1] = (val >> 8) & 0xFF;
                fault_addr[2] = (val >> 16) & 0xFF; fault_addr[3] = (val >> 24) & 0xFF;
                uc->uc_mcontext.arm_pc = pc + 4;
                return;
            }
        }

        /* Thumb-2 LDRH.W / STRH.W */
        if ((hw1 & 0xFFF0) == 0xF8B0 || (hw1 & 0xFFF0) == 0xF8A0 ||
            (hw1 & 0xFFF0) == 0xF830 || (hw1 & 0xFFF0) == 0xF820) {
            int is_ld_h = (hw1 >> 4) & 1;
            rd = (hw2 >> 12) & 0xF;
            if (is_ld_h && rd < 15) {
                unsigned short val = fault_addr[0] | (fault_addr[1] << 8);
                regs[rd] = val;
                uc->uc_mcontext.arm_pc = pc + 4;
                return;
            }
            if (!is_ld_h && rd < 15) {
                unsigned short val = regs[rd];
                fault_addr[0] = val & 0xFF; fault_addr[1] = (val >> 8) & 0xFF;
                uc->uc_mcontext.arm_pc = pc + 4;
                return;
            }
        }

        fprintf(stderr, "[libMali] SIGBUS: unhandled Thumb-2 hw1=0x%04x hw2=0x%04x at PC=0x%lx addr=%p\n",
                hw1, hw2, pc, info->si_addr);
        _exit(99);
    }

    fprintf(stderr, "[libMali] SIGBUS: unhandled ARM insn=0x%08x at PC=0x%lx addr=%p\n",
            insn, pc, info->si_addr);
    _exit(99);
}

/* Squirrel interception belongs to console-mod/m2hook_print.so.
 * Do not patch sq_pushstring a second time in the graphics adapter. */

/* ================================================================== */
/* Constructor                                                         */
/* ================================================================== */

__attribute__((constructor))
static void libmali_init(void) {
    const char *debug_env = getenv("LIBMALI_DEBUG");
    if (debug_env && (debug_env[0] == '1' || debug_env[0] == 'y' || debug_env[0] == 'Y')) {
        g_debug = 1;
    }

    g_main_pid = getpid();
    DBG("Initializing (GL proxy client), pid=%d...\n", g_main_pid);

    struct sigaction sa;
    memset(&sa, 0, sizeof(sa));
    sa.sa_sigaction = sigbus_handler;
    sa.sa_flags = SA_SIGINFO | SA_NODEFER;
    sigaction(SIGBUS, &sa, NULL);
    DBG("SIGBUS alignment fixup handler installed (with Thumb-2 support)\n");

}

/* ================================================================== */
/* EGL functions                                                       */
/* ================================================================== */

EGLDisplay eglGetDisplay(EGLNativeDisplayType display_id) {
    DBG("eglGetDisplay called\n");
    if (!connect_proxy()) {
        fprintf(stderr, "[libMali] Failed to connect to gl_proxy!\n");
        return EGL_NO_DISPLAY;
    }
    DBG("Returning fake EGL display (proxy connected)\n");
    return FAKE_EGL_DPY;
}

EGLBoolean eglInitialize(EGLDisplay dpy, EGLint *major, EGLint *minor) {
    DBG("eglInitialize\n");
    if (major) *major = 1;
    if (minor) *minor = 4;
    return EGL_TRUE;
}

EGLBoolean eglTerminate(EGLDisplay dpy) {
    DBG("eglTerminate\n");
    if (g_shm) {
        __atomic_store_n(&g_shm->shutdown, 1, __ATOMIC_RELEASE);
    }
    return EGL_TRUE;
}

EGLBoolean eglChooseConfig(EGLDisplay dpy, const EGLint *attrib_list, EGLConfig *configs, EGLint config_size, EGLint *num_config) {
    DBG("eglChooseConfig\n");
    if (configs && config_size > 0) configs[0] = (EGLConfig)1;
    if (num_config) *num_config = 1;
    return EGL_TRUE;
}

EGLSurface eglCreateWindowSurface(EGLDisplay dpy, EGLConfig config, EGLNativeWindowType win, const EGLint *attrib_list) {
    DBG("eglCreateWindowSurface\n");
    return FAKE_EGL_SURF;
}

EGLSurface eglCreatePbufferSurface(EGLDisplay dpy, EGLConfig config, const EGLint *attrib_list) {
    DBG("eglCreatePbufferSurface -> using window surface\n");
    return FAKE_EGL_SURF;
}

EGLContext eglCreateContext(EGLDisplay dpy, EGLConfig config, EGLContext share_context, const EGLint *attrib_list) {
    DBG("eglCreateContext\n");
    return FAKE_EGL_CTX;
}

EGLBoolean eglDestroyContext(EGLDisplay dpy, EGLContext ctx) { return EGL_TRUE; }
EGLBoolean eglDestroySurface(EGLDisplay dpy, EGLSurface surface) { return EGL_TRUE; }

EGLBoolean eglMakeCurrent(EGLDisplay dpy, EGLSurface draw, EGLSurface read_s, EGLContext ctx) {
    static int log_count = 0;
    if (g_debug && log_count++ < 5) DBG("eglMakeCurrent\n");
    return EGL_TRUE;
}

EGLBoolean eglSwapBuffers(EGLDisplay dpy, EGLSurface surface) {
    if (!g_proxy_connected) return EGL_FALSE;

    static int swap_count = 0;
    swap_count++;
    if (g_debug && (swap_count <= 10 || (swap_count % 60) == 0))
        DBG("eglSwapBuffers #%d\n", swap_count);

    CMD_SYNC(GL_CMD_SWAP_BUFFERS);
    cmd_end_sync();
    return EGL_TRUE;
}

EGLBoolean eglQuerySurface(EGLDisplay dpy, EGLSurface surface, EGLint attribute, EGLint *value) {
    if (attribute == EGL_WIDTH && value) { *value = RENDER_WIDTH; return EGL_TRUE; }
    if (attribute == EGL_HEIGHT && value) { *value = RENDER_HEIGHT; return EGL_TRUE; }
    return EGL_TRUE;
}

__eglMustCastToProperFunctionPointerType eglGetProcAddress(const char *procname) {
    /* Return our own wrappers for known functions */
    if (strcmp(procname, "glBindFramebuffer") == 0) return (__eglMustCastToProperFunctionPointerType)glBindFramebuffer;
    if (strcmp(procname, "glBindFramebufferOES") == 0) return (__eglMustCastToProperFunctionPointerType)glBindFramebuffer;
    if (strcmp(procname, "glBindFramebufferEXT") == 0) return (__eglMustCastToProperFunctionPointerType)glBindFramebuffer;
    if (g_debug && procname) DBG("eglGetProcAddress(%s) -> NULL\n", procname);
    return NULL;
}

EGLint eglGetError(void) { return 0x3000; /* EGL_SUCCESS */ }
const char* eglQueryString(EGLDisplay dpy, EGLint name) {
    switch (name) {
        case 0x3053: return "1.4";            /* EGL_VERSION */
        case 0x3054: return "Anthropic";      /* EGL_VENDOR */
        case 0x3055: return "";               /* EGL_EXTENSIONS */
        default: return "";
    }
}
EGLBoolean eglBindAPI(EGLenum api) { return EGL_TRUE; }
EGLContext eglGetCurrentContext(void) { return FAKE_EGL_CTX; }
EGLDisplay eglGetCurrentDisplay(void) { return FAKE_EGL_DPY; }
EGLSurface eglGetCurrentSurface(EGLint readdraw) { return FAKE_EGL_SURF; }

/* ================================================================== */
/* GL function wrappers - write commands to ring buffer                */
/* ================================================================== */

const GLubyte* glGetString(GLenum name) {
    if (!g_proxy_connected && !connect_proxy()) return (const GLubyte*)"";
    CMD_SYNC(GL_CMD_GET_STRING);
    cmd_u32(name);
    uint32_t len = cmd_end_sync();
    if (len > sizeof(g_gl_string_buf) - 1) len = sizeof(g_gl_string_buf) - 1;
    if (len > 0 && g_shm->response_size > 0) {
        memcpy(g_gl_string_buf, (void*)g_shm->response_data, len);
    }
    g_gl_string_buf[len] = '\0';
    return (const GLubyte*)g_gl_string_buf;
}

GLenum glGetError(void) {
    if (!g_proxy_connected) return GL_NO_ERROR;
    CMD_SYNC(GL_CMD_GET_ERROR);
    return (GLenum)cmd_end_sync();
}

void glGetIntegerv(GLenum pname, GLint *params) {
    if (!g_proxy_connected || !params) return;
    /* Determine count based on pname - most return 1 value */
    uint32_t count = 1;
    /* Some pnames return multiple values */
    switch (pname) {
        case 0x0BA2: count = 16; break; /* GL_MODELVIEW_MATRIX - won't happen in ES2 */
        case 0x0D33: count = 1; break;  /* GL_MAX_TEXTURE_SIZE */
        default: count = 1; break;
    }
    CMD_SYNC(GL_CMD_GET_INTEGERV);
    cmd_u32(pname);
    cmd_u32(count);
    cmd_end_sync();
    if (g_shm->response_size >= count * 4) {
        memcpy(params, (void*)g_shm->response_data, count * 4);
    }
}

void glGetFloatv(GLenum pname, GLfloat *params) {
    if (!g_proxy_connected || !params) return;
    uint32_t count = 1;
    CMD_SYNC(GL_CMD_GET_FLOATV);
    cmd_u32(pname);
    cmd_u32(count);
    cmd_end_sync();
    if (g_shm->response_size >= count * 4) {
        memcpy(params, (void*)g_shm->response_data, count * 4);
    }
}

void glClear(GLbitfield mask) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_CLEAR);
    cmd_u32(mask);
    CMD_END();
}

void glClearColor(GLfloat r, GLfloat g, GLfloat b, GLfloat a) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_CLEAR_COLOR);
    cmd_f32(r); cmd_f32(g); cmd_f32(b); cmd_f32(a);
    CMD_END();
}

void glViewport(GLint x, GLint y, GLsizei w, GLsizei h) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_VIEWPORT);
    cmd_u32((uint32_t)x); cmd_u32((uint32_t)y); cmd_u32((uint32_t)w); cmd_u32((uint32_t)h);
    CMD_END();
}

void glEnable(GLenum cap) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_ENABLE); cmd_u32(cap); CMD_END();
}

void glDisable(GLenum cap) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_DISABLE); cmd_u32(cap); CMD_END();
}

void glBindFramebuffer(GLenum target, GLuint fb) {
    if (!g_proxy_connected) return;
    g_current_fbo = fb;
    if (fb != 0) g_last_nonzero_fbo = fb;
    CMD_FIRE(GL_CMD_BIND_FRAMEBUFFER); cmd_u32(target); cmd_u32(fb); CMD_END();
}

void glBindTexture(GLenum target, GLuint tex) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_BIND_TEXTURE); cmd_u32(target); cmd_u32(tex); CMD_END();
}

void glGenTextures(GLsizei n, GLuint *tex) {
    if (!g_proxy_connected || !tex || n <= 0) return;
    CMD_SYNC(GL_CMD_GEN_TEXTURES);
    cmd_u32((uint32_t)n);
    cmd_end_sync();
    if (g_shm->response_size >= (uint32_t)n * 4) {
        memcpy(tex, (void*)g_shm->response_data, n * 4);
    }
    if (g_debug) DBG("glGenTextures(%d) -> first=%u\n", n, tex[0]);
}

void glDeleteTextures(GLsizei n, const GLuint *tex) {
    if (!g_proxy_connected || !tex || n <= 0) return;
    CMD_FIRE(GL_CMD_DELETE_TEXTURES);
    cmd_u32((uint32_t)n);
    for (int i = 0; i < n; i++) cmd_u32(tex[i]);
    CMD_END();
}

void glTexImage2D(GLenum t, GLint l, GLint ifmt, GLsizei w, GLsizei h, GLint border, GLenum fmt, GLenum type, const void *pixels) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_TEX_IMAGE_2D);
    cmd_u32(t); cmd_u32((uint32_t)l); cmd_u32((uint32_t)ifmt);
    cmd_u32((uint32_t)w); cmd_u32((uint32_t)h); cmd_u32((uint32_t)border);
    cmd_u32(fmt); cmd_u32(type);
    if (pixels) {
        uint32_t ps = pixel_size(fmt, type);
        uint32_t data_size = w * h * ps;
        cmd_u32(1);  /* has_data */
        cmd_u32(data_size);
        /* Ensure ring has space for this data */
        wait_ring_space(g_cmd_wp - g_cmd_start + data_size + 64);
        cmd_data(pixels, data_size);
    } else {
        cmd_u32(0);  /* no data */
    }
    CMD_END();
}

void glTexSubImage2D(GLenum t, GLint l, GLint x, GLint y, GLsizei w, GLsizei h, GLenum fmt, GLenum type, const void *pixels) {
    if (!g_proxy_connected || !pixels) return;
    uint32_t ps = pixel_size(fmt, type);
    uint32_t data_size = w * h * ps;
    CMD_FIRE(GL_CMD_TEX_SUB_IMAGE_2D);
    cmd_u32(t); cmd_u32((uint32_t)l); cmd_u32((uint32_t)x); cmd_u32((uint32_t)y);
    cmd_u32((uint32_t)w); cmd_u32((uint32_t)h);
    cmd_u32(fmt); cmd_u32(type);
    cmd_u32(data_size);
    wait_ring_space(g_cmd_wp - g_cmd_start + data_size + 64);
    cmd_data(pixels, data_size);
    CMD_END();
}

void glTexParameteri(GLenum t, GLenum p, GLint v) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_TEX_PARAMETERI); cmd_u32(t); cmd_u32(p); cmd_u32((uint32_t)v); CMD_END();
}

void glGenFramebuffers(GLsizei n, GLuint *fb) {
    if (!g_proxy_connected || !fb || n <= 0) return;
    CMD_SYNC(GL_CMD_GEN_FRAMEBUFFERS);
    cmd_u32((uint32_t)n);
    cmd_end_sync();
    if (g_shm->response_size >= (uint32_t)n * 4) {
        memcpy(fb, (void*)g_shm->response_data, n * 4);
    }
}

void glDeleteFramebuffers(GLsizei n, const GLuint *fb) {
    if (!g_proxy_connected || !fb || n <= 0) return;
    CMD_FIRE(GL_CMD_DELETE_FRAMEBUFFERS);
    cmd_u32((uint32_t)n);
    for (int i = 0; i < n; i++) cmd_u32(fb[i]);
    CMD_END();
}

void glFramebufferTexture2D(GLenum t, GLenum a, GLenum tt, GLuint tx, GLint l) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_FRAMEBUFFER_TEXTURE_2D);
    cmd_u32(t); cmd_u32(a); cmd_u32(tt); cmd_u32(tx); cmd_u32((uint32_t)l);
    CMD_END();
}

GLenum glCheckFramebufferStatus(GLenum t) {
    if (!g_proxy_connected) return 0;
    CMD_SYNC(GL_CMD_CHECK_FRAMEBUFFER_STATUS);
    cmd_u32(t);
    return (GLenum)cmd_end_sync();
}

void glReadPixels(GLint x, GLint y, GLsizei w, GLsizei h, GLenum fmt, GLenum type, void *pixels) {
    if (!g_proxy_connected || !pixels) return;
    CMD_SYNC(GL_CMD_READ_PIXELS);
    cmd_u32((uint32_t)x); cmd_u32((uint32_t)y);
    cmd_u32((uint32_t)w); cmd_u32((uint32_t)h);
    cmd_u32(fmt); cmd_u32(type);
    cmd_end_sync();
    uint32_t ps = pixel_size(fmt, type);
    uint32_t data_size = w * h * ps;
    if (g_shm->response_size > 0 && g_shm->response_size <= data_size) {
        memcpy(pixels, (void*)g_shm->response_data, g_shm->response_size);
    }
}

void glDrawArrays(GLenum m, GLint f, GLsizei c) {
    if (!g_proxy_connected || c <= 0) return;

    /* Upload client-side vertex attrib data */
    for (int i = 0; i < MAX_VERTEX_ATTRIBS; i++) {
        if (!g_attribs[i].enabled || !g_attribs[i].is_client || !g_attribs[i].ptr)
            continue;
        uint32_t elem_size = g_attribs[i].size * gl_type_size(g_attribs[i].type);
        uint32_t actual_stride = g_attribs[i].stride ? (uint32_t)g_attribs[i].stride : elem_size;
        uint32_t num_verts = (uint32_t)(f + c);
        uint32_t data_size = actual_stride * (num_verts > 0 ? num_verts - 1 : 0) + elem_size;
        CMD_FIRE(GL_CMD_UPLOAD_CLIENT_ARRAY);
        cmd_u32((uint32_t)i);
        cmd_u32((uint32_t)g_attribs[i].size);
        cmd_u32(g_attribs[i].type);
        cmd_u32((uint32_t)g_attribs[i].normalized);
        cmd_u32((uint32_t)g_attribs[i].stride);
        cmd_u32(data_size);
        cmd_data(g_attribs[i].ptr, data_size);
        CMD_END();
    }

    CMD_FIRE(GL_CMD_DRAW_ARRAYS);
    cmd_u32(m); cmd_u32((uint32_t)f); cmd_u32((uint32_t)c);
    CMD_END();
}

void glDrawElements(GLenum m, GLsizei c, GLenum t, const void *indices) {
    if (!g_proxy_connected || c <= 0) return;

    /* Determine max vertex index for client-side attrib data sizing */
    uint32_t max_vertex = 0;
    int has_client_attribs = 0;
    int has_client_indices = (g_current_element_buffer == 0);

    for (int i = 0; i < MAX_VERTEX_ATTRIBS; i++) {
        if (g_attribs[i].enabled && g_attribs[i].is_client) {
            has_client_attribs = 1;
            break;
        }
    }

    if (has_client_attribs && has_client_indices && indices) {
        /* Scan index buffer to find max vertex index */
        uint32_t idx_type_size = gl_type_size(t);
        for (GLsizei i = 0; i < c; i++) {
            uint32_t idx = 0;
            if (idx_type_size == 2) idx = ((const uint16_t*)indices)[i];
            else if (idx_type_size == 1) idx = ((const uint8_t*)indices)[i];
            else if (idx_type_size == 4) idx = ((const uint32_t*)indices)[i];
            if (idx > max_vertex) max_vertex = idx;
        }
    }

    /* Upload client-side index data */
    if (has_client_indices && indices) {
        uint32_t idx_type_size = gl_type_size(t);
        uint32_t idx_data_size = (uint32_t)c * idx_type_size;
        CMD_FIRE(GL_CMD_UPLOAD_INDEX_ARRAY);
        cmd_u32(idx_data_size);
        cmd_data(indices, idx_data_size);
        CMD_END();
    }

    /* Upload client-side vertex attrib data */
    if (has_client_attribs) {
        uint32_t num_verts = max_vertex + 1;
        for (int i = 0; i < MAX_VERTEX_ATTRIBS; i++) {
            if (!g_attribs[i].enabled || !g_attribs[i].is_client || !g_attribs[i].ptr)
                continue;
            uint32_t elem_size = g_attribs[i].size * gl_type_size(g_attribs[i].type);
            uint32_t actual_stride = g_attribs[i].stride ? (uint32_t)g_attribs[i].stride : elem_size;
            uint32_t data_size = actual_stride * (num_verts > 0 ? num_verts - 1 : 0) + elem_size;
            CMD_FIRE(GL_CMD_UPLOAD_CLIENT_ARRAY);
            cmd_u32((uint32_t)i);
            cmd_u32((uint32_t)g_attribs[i].size);
            cmd_u32(g_attribs[i].type);
            cmd_u32((uint32_t)g_attribs[i].normalized);
            cmd_u32((uint32_t)g_attribs[i].stride);
            cmd_u32(data_size);
            cmd_data(g_attribs[i].ptr, data_size);
            CMD_END();
        }
    }

    /* Now issue the actual draw call */
    CMD_FIRE(GL_CMD_DRAW_ELEMENTS);
    cmd_u32(m); cmd_u32((uint32_t)c); cmd_u32(t);
    cmd_u32(has_client_indices ? 0 : (uint32_t)(uintptr_t)indices);
    CMD_END();
}

GLuint glCreateProgram(void) {
    if (!g_proxy_connected) return 0;
    CMD_SYNC(GL_CMD_CREATE_PROGRAM);
    GLuint p = (GLuint)cmd_end_sync();
    if (g_debug) DBG("glCreateProgram() -> %u\n", p);
    return p;
}

void glDeleteProgram(GLuint p) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_DELETE_PROGRAM); cmd_u32(p); CMD_END();
}

void glUseProgram(GLuint p) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_USE_PROGRAM); cmd_u32(p); CMD_END();
}

void glLinkProgram(GLuint p) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_LINK_PROGRAM); cmd_u32(p); CMD_END();
}

void glGetProgramiv(GLuint p, GLenum name, GLint *v) {
    if (!g_proxy_connected || !v) return;
    CMD_SYNC(GL_CMD_GET_PROGRAMIV);
    cmd_u32(p); cmd_u32(name);
    *v = (GLint)cmd_end_sync();
}

void glGetProgramInfoLog(GLuint p, GLsizei maxlen, GLsizei *length, GLchar *infolog) {
    if (!g_proxy_connected || !infolog) return;
    CMD_SYNC(GL_CMD_GET_PROGRAM_INFO_LOG);
    cmd_u32(p); cmd_u32((uint32_t)maxlen);
    uint32_t actual = cmd_end_sync();
    if (actual > 0 && g_shm->response_size > 0) {
        uint32_t copy = g_shm->response_size;
        if (copy > (uint32_t)maxlen - 1) copy = maxlen - 1;
        memcpy(infolog, (void*)g_shm->response_data, copy);
        infolog[copy] = '\0';
        if (length) *length = copy;
    } else {
        infolog[0] = '\0';
        if (length) *length = 0;
    }
}

GLuint glCreateShader(GLenum t) {
    if (!g_proxy_connected) return 0;
    CMD_SYNC(GL_CMD_CREATE_SHADER);
    cmd_u32(t);
    return (GLuint)cmd_end_sync();
}

void glDeleteShader(GLuint s) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_DELETE_SHADER); cmd_u32(s); CMD_END();
}

void glShaderSource(GLuint s, GLsizei count, const GLchar *const *str, const GLint *lengths) {
    if (!g_proxy_connected || !str || count <= 0) return;
    /* Concatenate all source strings */
    uint32_t total = 0;
    for (int i = 0; i < count; i++) {
        if (lengths && lengths[i] >= 0)
            total += lengths[i];
        else if (str[i])
            total += strlen(str[i]);
    }

    CMD_FIRE(GL_CMD_SHADER_SOURCE);
    cmd_u32(s);
    cmd_u32(total);
    /* Write concatenated source */
    for (int i = 0; i < count; i++) {
        uint32_t len;
        if (lengths && lengths[i] >= 0)
            len = lengths[i];
        else if (str[i])
            len = strlen(str[i]);
        else
            continue;
        if (len > 0) {
            wait_ring_space(g_cmd_wp - g_cmd_start + len + 64);
            cmd_data(str[i], len);
        }
    }
    CMD_END();
}

void glCompileShader(GLuint s) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_COMPILE_SHADER); cmd_u32(s); CMD_END();
}

void glGetShaderiv(GLuint s, GLenum pname, GLint *v) {
    if (!g_proxy_connected || !v) return;
    CMD_SYNC(GL_CMD_GET_SHADERIV);
    cmd_u32(s); cmd_u32(pname);
    *v = (GLint)cmd_end_sync();
}

void glGetShaderInfoLog(GLuint s, GLsizei maxlen, GLsizei *length, GLchar *infolog) {
    if (!g_proxy_connected || !infolog) return;
    CMD_SYNC(GL_CMD_GET_SHADER_INFO_LOG);
    cmd_u32(s); cmd_u32((uint32_t)maxlen);
    uint32_t actual = cmd_end_sync();
    if (actual > 0 && g_shm->response_size > 0) {
        uint32_t copy = g_shm->response_size;
        if (copy > (uint32_t)maxlen - 1) copy = maxlen - 1;
        memcpy(infolog, (void*)g_shm->response_data, copy);
        infolog[copy] = '\0';
        if (length) *length = copy;
    } else {
        infolog[0] = '\0';
        if (length) *length = 0;
    }
}

void glAttachShader(GLuint p, GLuint s) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_ATTACH_SHADER); cmd_u32(p); cmd_u32(s); CMD_END();
}

void glGetAttachedShaders(GLuint program, GLsizei maxCount, GLsizei *count, GLuint *shaders) {
    if (!g_proxy_connected || !shaders) return;
    CMD_SYNC(GL_CMD_GET_ATTACHED_SHADERS);
    cmd_u32(program); cmd_u32((uint32_t)maxCount);
    uint32_t actual = cmd_end_sync();
    if (count) *count = (GLsizei)actual;
    if (actual > 0 && g_shm->response_size >= actual * 4) {
        uint32_t copy = actual;
        if (copy > (uint32_t)maxCount) copy = maxCount;
        memcpy(shaders, (void*)g_shm->response_data, copy * 4);
    }
}

GLint glGetUniformLocation(GLuint p, const GLchar *name) {
    if (!g_proxy_connected || !name) return -1;
    uint32_t len = strlen(name);
    CMD_SYNC(GL_CMD_GET_UNIFORM_LOCATION);
    cmd_u32(p); cmd_u32(len);
    cmd_data(name, len);
    return (GLint)(int32_t)cmd_end_sync();
}

GLint glGetAttribLocation(GLuint p, const GLchar *name) {
    if (!g_proxy_connected || !name) return -1;
    uint32_t len = strlen(name);
    CMD_SYNC(GL_CMD_GET_ATTRIB_LOCATION);
    cmd_u32(p); cmd_u32(len);
    cmd_data(name, len);
    return (GLint)(int32_t)cmd_end_sync();
}

void glUniform1i(GLint l, GLint v) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_UNIFORM_1I); cmd_u32((uint32_t)l); cmd_u32((uint32_t)v); CMD_END();
}

void glUniform1f(GLint l, GLfloat v) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_UNIFORM_1F); cmd_u32((uint32_t)l); cmd_f32(v); CMD_END();
}

void glUniform2f(GLint l, GLfloat x, GLfloat y) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_UNIFORM_2F); cmd_u32((uint32_t)l); cmd_f32(x); cmd_f32(y); CMD_END();
}

void glUniform3f(GLint l, GLfloat x, GLfloat y, GLfloat z) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_UNIFORM_3F); cmd_u32((uint32_t)l); cmd_f32(x); cmd_f32(y); cmd_f32(z); CMD_END();
}

void glUniform4f(GLint l, GLfloat x, GLfloat y, GLfloat z, GLfloat w) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_UNIFORM_4F); cmd_u32((uint32_t)l); cmd_f32(x); cmd_f32(y); cmd_f32(z); cmd_f32(w); CMD_END();
}

void glUniform4fv(GLint l, GLsizei count, const GLfloat *v) {
    if (!g_proxy_connected || !v) return;
    CMD_FIRE(GL_CMD_UNIFORM_4FV);
    cmd_u32((uint32_t)l); cmd_u32((uint32_t)count);
    for (GLsizei i = 0; i < count * 4; i++) cmd_f32(v[i]);
    CMD_END();
}

void glUniformMatrix4fv(GLint l, GLsizei count, GLboolean transpose, const GLfloat *v) {
    if (!g_proxy_connected || !v) return;
    CMD_FIRE(GL_CMD_UNIFORM_MATRIX_4FV);
    cmd_u32((uint32_t)l); cmd_u32((uint32_t)count); cmd_u32((uint32_t)transpose);
    uint32_t n = count * 16;
    for (uint32_t i = 0; i < n; i++) cmd_f32(v[i]);
    CMD_END();
}

void glVertexAttribPointer(GLuint index, GLint size, GLenum type, GLboolean normalized, GLsizei stride, const void *pointer) {
    if (!g_proxy_connected) return;
    if (index < MAX_VERTEX_ATTRIBS) {
        g_attribs[index].ptr = pointer;
        g_attribs[index].size = size;
        g_attribs[index].type = type;
        g_attribs[index].normalized = normalized;
        g_attribs[index].stride = stride;
        g_attribs[index].is_client = (g_current_array_buffer == 0) ? 1 : 0;
    }
    if (g_current_array_buffer != 0) {
        /* VBO-based: send pointer as offset directly */
        CMD_FIRE(GL_CMD_VERTEX_ATTRIB_POINTER);
        cmd_u32(index); cmd_u32((uint32_t)size); cmd_u32(type);
        cmd_u32((uint32_t)normalized); cmd_u32((uint32_t)stride);
        cmd_u32((uint32_t)(uintptr_t)pointer);
        CMD_END();
    }
    /* Client-side: defer to draw time - data will be uploaded then */
}

void glEnableVertexAttribArray(GLuint i) {
    if (!g_proxy_connected) return;
    if (i < MAX_VERTEX_ATTRIBS) g_attribs[i].enabled = 1;
    CMD_FIRE(GL_CMD_ENABLE_VERTEX_ATTRIB_ARRAY); cmd_u32(i); CMD_END();
}

void glDisableVertexAttribArray(GLuint i) {
    if (!g_proxy_connected) return;
    if (i < MAX_VERTEX_ATTRIBS) g_attribs[i].enabled = 0;
    CMD_FIRE(GL_CMD_DISABLE_VERTEX_ATTRIB_ARRAY); cmd_u32(i); CMD_END();
}

void glGenBuffers(GLsizei n, GLuint *b) {
    if (!g_proxy_connected || !b || n <= 0) return;
    CMD_SYNC(GL_CMD_GEN_BUFFERS);
    cmd_u32((uint32_t)n);
    cmd_end_sync();
    if (g_shm->response_size >= (uint32_t)n * 4) {
        memcpy(b, (void*)g_shm->response_data, n * 4);
    }
}

void glDeleteBuffers(GLsizei n, const GLuint *b) {
    if (!g_proxy_connected || !b || n <= 0) return;
    CMD_FIRE(GL_CMD_DELETE_BUFFERS);
    cmd_u32((uint32_t)n);
    for (int i = 0; i < n; i++) cmd_u32(b[i]);
    CMD_END();
}

void glBindBuffer(GLenum t, GLuint b) {
    if (!g_proxy_connected) return;
    if (t == GL_ARRAY_BUFFER) g_current_array_buffer = b;
    else if (t == 0x8893) g_current_element_buffer = b; /* GL_ELEMENT_ARRAY_BUFFER */
    CMD_FIRE(GL_CMD_BIND_BUFFER); cmd_u32(t); cmd_u32(b); CMD_END();
}

void glBufferData(GLenum t, GLsizeiptr size, const void *data, GLenum usage) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_BUFFER_DATA);
    cmd_u32(t); cmd_u32((uint32_t)size); cmd_u32(usage);
    if (data) {
        cmd_u32(1); /* has_data */
        cmd_u32((uint32_t)size);
        wait_ring_space(g_cmd_wp - g_cmd_start + (uint32_t)size + 64);
        cmd_data(data, (uint32_t)size);
    } else {
        cmd_u32(0); /* no data */
    }
    CMD_END();
}

void glBufferSubData(GLenum t, GLintptr offset, GLsizeiptr size, const void *data) {
    if (!g_proxy_connected || !data) return;
    CMD_FIRE(GL_CMD_BUFFER_SUB_DATA);
    cmd_u32(t); cmd_u32((uint32_t)offset); cmd_u32((uint32_t)size);
    cmd_u32((uint32_t)size);
    wait_ring_space(g_cmd_wp - g_cmd_start + (uint32_t)size + 64);
    cmd_data(data, (uint32_t)size);
    CMD_END();
}

void glBlendFunc(GLenum s, GLenum d) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_BLEND_FUNC); cmd_u32(s); cmd_u32(d); CMD_END();
}

void glBlendFuncSeparate(GLenum sr, GLenum dr, GLenum sa, GLenum da) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_BLEND_FUNC_SEPARATE); cmd_u32(sr); cmd_u32(dr); cmd_u32(sa); cmd_u32(da); CMD_END();
}

void glBlendEquation(GLenum m) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_BLEND_EQUATION); cmd_u32(m); CMD_END();
}

void glBlendEquationSeparate(GLenum mr, GLenum ma) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_BLEND_EQUATION_SEPARATE); cmd_u32(mr); cmd_u32(ma); CMD_END();
}

void glScissor(GLint x, GLint y, GLsizei w, GLsizei h) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_SCISSOR); cmd_u32((uint32_t)x); cmd_u32((uint32_t)y); cmd_u32((uint32_t)w); cmd_u32((uint32_t)h); CMD_END();
}

void glDepthFunc(GLenum f) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_DEPTH_FUNC); cmd_u32(f); CMD_END();
}

void glDepthMask(GLboolean f) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_DEPTH_MASK); cmd_u32((uint32_t)f); CMD_END();
}

void glColorMask(GLboolean r, GLboolean g, GLboolean b, GLboolean a) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_COLOR_MASK); cmd_u32(r); cmd_u32(g); cmd_u32(b); cmd_u32(a); CMD_END();
}

void glCullFace(GLenum m) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_CULL_FACE); cmd_u32(m); CMD_END();
}

void glFrontFace(GLenum m) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_FRONT_FACE); cmd_u32(m); CMD_END();
}

void glFlush(void) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_FLUSH); CMD_END();
}

void glFinish(void) {
    if (!g_proxy_connected) return;
    CMD_SYNC(GL_CMD_FINISH);
    cmd_end_sync();
}

void glLineWidth(GLfloat width) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_LINE_WIDTH); cmd_f32(width); CMD_END();
}

void glPixelStorei(GLenum p, GLint v) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_PIXEL_STOREI); cmd_u32(p); cmd_u32((uint32_t)v); CMD_END();
}

void glActiveTexture(GLenum t) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_ACTIVE_TEXTURE); cmd_u32(t); CMD_END();
}

void glGenerateMipmap(GLenum t) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_GENERATE_MIPMAP); cmd_u32(t); CMD_END();
}

void glGenRenderbuffers(GLsizei n, GLuint *r) {
    if (!g_proxy_connected || !r || n <= 0) return;
    CMD_SYNC(GL_CMD_GEN_RENDERBUFFERS);
    cmd_u32((uint32_t)n);
    cmd_end_sync();
    if (g_shm->response_size >= (uint32_t)n * 4) {
        memcpy(r, (void*)g_shm->response_data, n * 4);
    }
}

void glDeleteRenderbuffers(GLsizei n, const GLuint *r) {
    if (!g_proxy_connected || !r || n <= 0) return;
    CMD_FIRE(GL_CMD_DELETE_RENDERBUFFERS);
    cmd_u32((uint32_t)n);
    for (int i = 0; i < n; i++) cmd_u32(r[i]);
    CMD_END();
}

void glBindRenderbuffer(GLenum t, GLuint r) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_BIND_RENDERBUFFER); cmd_u32(t); cmd_u32(r); CMD_END();
}

void glRenderbufferStorage(GLenum t, GLenum ifmt, GLsizei w, GLsizei h) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_RENDERBUFFER_STORAGE); cmd_u32(t); cmd_u32(ifmt); cmd_u32((uint32_t)w); cmd_u32((uint32_t)h); CMD_END();
}

void glFramebufferRenderbuffer(GLenum t, GLenum a, GLenum rbt, GLuint rb) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_FRAMEBUFFER_RENDERBUFFER); cmd_u32(t); cmd_u32(a); cmd_u32(rbt); cmd_u32(rb); CMD_END();
}

void glClearDepthf(GLfloat d) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_CLEAR_DEPTHF); cmd_f32(d); CMD_END();
}

void glDepthRangef(GLfloat n, GLfloat f) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_DEPTH_RANGEF); cmd_f32(n); cmd_f32(f); CMD_END();
}

void glCopyTexImage2D(GLenum t, GLint l, GLenum ifmt, GLint x, GLint y, GLsizei w, GLsizei h, GLint b) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_COPY_TEX_IMAGE_2D);
    cmd_u32(t); cmd_u32((uint32_t)l); cmd_u32(ifmt);
    cmd_u32((uint32_t)x); cmd_u32((uint32_t)y); cmd_u32((uint32_t)w); cmd_u32((uint32_t)h);
    cmd_u32((uint32_t)b);
    CMD_END();
}

void glCopyTexSubImage2D(GLenum t, GLint l, GLint xo, GLint yo, GLint x, GLint y, GLsizei w, GLsizei h) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_COPY_TEX_SUB_IMAGE_2D);
    cmd_u32(t); cmd_u32((uint32_t)l);
    cmd_u32((uint32_t)xo); cmd_u32((uint32_t)yo);
    cmd_u32((uint32_t)x); cmd_u32((uint32_t)y); cmd_u32((uint32_t)w); cmd_u32((uint32_t)h);
    CMD_END();
}

void glCompressedTexImage2D(GLenum target, GLint level, GLenum internalformat,
                            GLsizei width, GLsizei height, GLint border,
                            GLsizei imageSize, const void *data) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_COMPRESSED_TEX_IMAGE_2D);
    cmd_u32(target); cmd_u32((uint32_t)level); cmd_u32(internalformat);
    cmd_u32((uint32_t)width); cmd_u32((uint32_t)height); cmd_u32((uint32_t)border);
    cmd_u32((uint32_t)imageSize);
    if (data && imageSize > 0) {
        cmd_data(data, (uint32_t)imageSize);
    }
    CMD_END();
}

void glClearStencil(GLint s) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_CLEAR_STENCIL); cmd_u32((uint32_t)s); CMD_END();
}

void glStencilFunc(GLenum f, GLint ref, GLuint mask) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_STENCIL_FUNC); cmd_u32(f); cmd_u32((uint32_t)ref); cmd_u32(mask); CMD_END();
}

void glStencilMask(GLuint m) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_STENCIL_MASK); cmd_u32(m); CMD_END();
}

void glStencilOp(GLenum sf, GLenum df, GLenum dp) {
    if (!g_proxy_connected) return;
    CMD_FIRE(GL_CMD_STENCIL_OP); cmd_u32(sf); cmd_u32(df); cmd_u32(dp); CMD_END();
}

/* ================================================================== */
/* File interceptions (unchanged from original)                        */
/* ================================================================== */

/* Track fds for interception */
static int g_gpio_keys_fd = -1;
static int g_sunxi_dump_fd = -1;
static int g_i2c_fd = -1;

/* Fake evdev fds - FIFOs from gl_proxy for keyboard input */
#define INPUT_FIFO_0 "/tmp/m2e_input0"
#define INPUT_FIFO_1 "/tmp/m2e_input1"
static int g_evdev_fds[2] = {-1, -1};  /* event4 -> [0], event5 -> [1] */

/* Descriptors are reused after close. Never treat a later ROM/save/audio file
 * as the old emulated I2C device (which would replace its reads with "XGHT"). */
int close(int fd) {
    typedef int (*fn)(int);
    static fn real = NULL;
    if (!real) real = dlsym(RTLD_NEXT, "close");
    if (fd >= 0) {
        if (fd == g_i2c_fd) g_i2c_fd = -1;
        if (fd == g_gpio_keys_fd) g_gpio_keys_fd = -1;
        if (fd == g_sunxi_dump_fd) g_sunxi_dump_fd = -1;
        for (int i = 0; i < 2; ++i)
            if (fd == g_evdev_fds[i]) g_evdev_fds[i] = -1;
    }
    return real(fd);
}

static int is_fake_evdev(int fd) {
    return (fd >= 0 && (fd == g_evdev_fds[0] || fd == g_evdev_fds[1]));
}
static int do_openat(int dirfd, const char *pathname, int flags, mode_t mode) {
    typedef int (*fn)(int, const char*, int, ...);
    static fn real = NULL;
    if (!real) real = dlsym(RTLD_NEXT, "openat");

    if (g_debug && pathname && (strstr(pathname, "sunxi") || strstr(pathname, "i2c") || strstr(pathname, "gpio")))
        fprintf(stderr, "[libMali] do_openat(%s, flags=0x%x)\n", pathname, flags);

    if (pathname && strstr(pathname, "gpio-keys")) {
        DBG("openat(%s) -> tracking fd for power switch\n", pathname);
        int fd = (flags & O_CREAT) ? real(dirfd, pathname, flags, mode) : real(dirfd, pathname, flags);
        if (fd >= 0) g_gpio_keys_fd = fd;
        DBG("gpio-keys fd=%d\n", fd);
        return fd;
    }

    /* Intercept sunxi_dump / tmp//dump - redirect to /tmp/sunxi_dump */
    if (pathname && (strstr(pathname, "sunxi_dump") || strstr(pathname, "tmp//dump"))) {
        DBG("openat(%s) -> redirecting to /tmp/sunxi_dump\n", pathname);
        int fd = real(dirfd, "/tmp/sunxi_dump", O_RDWR | O_CREAT, 0666);
        if (fd >= 0) g_sunxi_dump_fd = fd;
        return fd;
    }

    /* Intercept i2c paths for platform check */
    if (pathname && (strstr(pathname, "/dev/i2c-1") ||
                     strstr(pathname, "/sys/bus/i2c/devices/2-0050"))) {
        int pipefd[2];
        if (pipe(pipefd) == 0) {
            g_i2c_fd = pipefd[0];
            typedef ssize_t (*write_fn)(int, const void*, size_t);
            write_fn real_write = dlsym(RTLD_NEXT, "write");
            if (real_write) real_write(pipefd[1], "XGHT", 4);
            close(pipefd[1]);
            DBG("openat(%s) -> fake i2c fd=%d\n", pathname, g_i2c_fd);
            return g_i2c_fd;
        }
    }

    /* Redirect /dev/input opens to FIFOs */
    if (pathname && strstr(pathname, "/dev/input")) {
        typedef int (*open_fn)(const char*, int, ...);
        static open_fn real_open = NULL;
        if (!real_open) real_open = dlsym(RTLD_NEXT, "open");
        if (strstr(pathname, "event4")) {
            int fd = real_open(INPUT_FIFO_0, O_RDONLY | O_NONBLOCK);
            if (fd >= 0) g_evdev_fds[0] = fd;
            return fd;
        }
        if (strstr(pathname, "event5")) {
            int fd = real_open(INPUT_FIFO_1, O_RDONLY | O_NONBLOCK);
            if (fd >= 0) g_evdev_fds[1] = fd;
            return fd;
        }
    }

    return (flags & O_CREAT) ? real(dirfd, pathname, flags, mode) : real(dirfd, pathname, flags);
}

/* Force exact symbol names, bypassing glibc's asm redirect of openat->openat64 */
int wrapper_openat(int dirfd, const char *pathname, int flags, ...) __asm__("openat");
int wrapper_openat64(int dirfd, const char *pathname, int flags, ...) __asm__("openat64");

int wrapper_openat(int dirfd, const char *pathname, int flags, ...) {
    mode_t mode = 0;
    if (flags & O_CREAT) {
        va_list args; va_start(args, flags);
        mode = va_arg(args, mode_t);
        va_end(args);
    }
    return do_openat(dirfd, pathname, flags, mode);
}

int wrapper_openat64(int dirfd, const char *pathname, int flags, ...) {
    mode_t mode = 0;
    if (flags & O_CREAT) {
        va_list args; va_start(args, flags);
        mode = va_arg(args, mode_t);
        va_end(args);
    }
    return do_openat(dirfd, pathname, flags, mode);
}

/* Force exact symbol names, bypassing glibc's asm redirect of open->open64 */
static int do_open(const char *pathname, int flags, mode_t mode);

int wrapper_open(const char *pathname, int flags, ...) __asm__("open");
int wrapper_open64(const char *pathname, int flags, ...) __asm__("open64");

int wrapper_open(const char *pathname, int flags, ...) {
    mode_t mode = 0;
    if (flags & O_CREAT) {
        va_list args; va_start(args, flags);
        mode = va_arg(args, mode_t);
        va_end(args);
    }
    return do_open(pathname, flags, mode);
}

int wrapper_open64(const char *pathname, int flags, ...) {
    mode_t mode = 0;
    if (flags & O_CREAT) {
        va_list args; va_start(args, flags);
        mode = va_arg(args, mode_t);
        va_end(args);
    }
    return do_open(pathname, flags, mode);
}

static int do_open(const char *pathname, int flags, mode_t mode) {
    typedef int (*fn)(const char*, int, ...);
    static fn real = NULL;
    if (!real) real = dlsym(RTLD_NEXT, "open");

    if (g_debug && pathname && (strstr(pathname, "sunxi") || strstr(pathname, "i2c") || strstr(pathname, "dump")))
        fprintf(stderr, "[libMali] open(%s, flags=0x%x) CALLED\n", pathname, flags);

    if (pathname && strstr(pathname, "gpio-keys")) {
        DBG("open(%s) -> /dev/input/event0\n", pathname);
        return real("/dev/input/event0", flags);
    }

    /* Intercept sunxi_dump / tmp//dump */
    if (pathname && (strstr(pathname, "sunxi_dump") || strstr(pathname, "tmp//dump"))) {
        DBG("open(%s) -> redirecting to /tmp/sunxi_dump\n", pathname);
        int fd = real("/tmp/sunxi_dump", O_RDWR | O_CREAT, 0666);
        if (fd >= 0) g_sunxi_dump_fd = fd;
        return fd;
    }

    /* Intercept i2c paths - use a real fd (pipe) so write/read work */
    if (pathname && (strstr(pathname, "/sys/bus/i2c/devices/2-0050") ||
                     strstr(pathname, "/dev/i2c-1"))) {
        int pipefd[2];
        if (pipe(pipefd) == 0) {
            g_i2c_fd = pipefd[0];
            typedef ssize_t (*write_fn)(int, const void*, size_t);
            write_fn real_write = dlsym(RTLD_NEXT, "write");
            if (real_write) real_write(pipefd[1], "XGHT", 4);
            close(pipefd[1]);
            DBG("open(%s) -> fake i2c fd=%d (pipe with XGHT)\n", pathname, g_i2c_fd);
            return g_i2c_fd;
        }
        DBG("open(%s) -> faking with /dev/null\n", pathname);
        return real("/dev/null", O_RDONLY);
    }

    /* Redirect /dev/input opens to FIFOs */
    if (pathname && strstr(pathname, "/dev/input")) {
        if (strstr(pathname, "event4")) {
            int fd = real(INPUT_FIFO_0, O_RDONLY | O_NONBLOCK);
            if (fd >= 0) g_evdev_fds[0] = fd;
            return fd;
        }
        if (strstr(pathname, "event5")) {
            int fd = real(INPUT_FIFO_1, O_RDONLY | O_NONBLOCK);
            if (fd >= 0) g_evdev_fds[1] = fd;
            return fd;
        }
    }

    int fd = (flags & O_CREAT) ? real(pathname, flags, mode) : real(pathname, flags);
    if (g_debug && pathname && !strstr(pathname, "/proc/") && !strstr(pathname, "/sys/") && !strstr(pathname, "/dev/"))
        DBG("open(%s) -> fd=%d\n", pathname, fd);
    return fd;
}

/* Also intercept access() and stat() for i2c platform check bypass */
int access(const char *pathname, int mode) {
    typedef int (*fn)(const char*, int);
    static fn real = NULL;
    if (!real) real = dlsym(RTLD_NEXT, "access");

    if (pathname && (strstr(pathname, "/sys/bus/i2c/devices/2-0050") ||
                     strstr(pathname, "/dev/i2c-1") ||
                     strstr(pathname, "i2c/devices") ||
                     strstr(pathname, "sunxi_dump") ||
                     strstr(pathname, "tmp//dump"))) {
        DBG("access(%s) -> faking success\n", pathname);
        return 0;
    }

    return real(pathname, mode);
}

int __xstat(int ver, const char *pathname, struct stat *buf) {
    typedef int (*fn)(int, const char*, struct stat*);
    static fn real = NULL;
    if (!real) real = dlsym(RTLD_NEXT, "__xstat");

    if (pathname && (strstr(pathname, "/sys/bus/i2c/devices/2-0050") ||
                     strstr(pathname, "/dev/i2c-1"))) {
        DBG("stat(%s) -> faking success\n", pathname);
        if (buf) memset(buf, 0, sizeof(*buf));
        return 0;
    }

    return real ? real(ver, pathname, buf) : -1;
}

int stat(const char *pathname, struct stat *buf) {
    typedef int (*fn)(const char*, struct stat*);
    static fn real = NULL;
    if (!real) real = dlsym(RTLD_NEXT, "stat");

    if (pathname && (strstr(pathname, "/sys/bus/i2c/devices/2-0050") ||
                     strstr(pathname, "/dev/i2c-1"))) {
        DBG("stat(%s) -> faking success\n", pathname);
        if (buf) memset(buf, 0, sizeof(*buf));
        return 0;
    }

    return real ? real(pathname, buf) : -1;
}

/* Intercept ioctl to fake power switch state and evdev gamepad queries */
int ioctl(int fd, unsigned long request, ...) {
    typedef int (*fn)(int, unsigned long, ...);
    static fn real = NULL;
    if (!real) real = dlsym(RTLD_NEXT, "ioctl");

    va_list args;
    va_start(args, request);
    void *arg = va_arg(args, void*);
    va_end(args);

    /* EVIOCGSW - get switch state bitmask */
    if (g_gpio_keys_fd >= 0 && fd == g_gpio_keys_fd && (request & 0xFFFF) == 0x4500 + EV_SW) {
        DBG("ioctl(fd=%d, EVIOCGSW) -> faking power switch ON\n", fd);
        if (arg) {
            memset(arg, 0, (request >> 16) & 0x3FFF);
            ((unsigned char*)arg)[0] = 0x01;
        }
        return 0;
    }

    /* EVIOCGBIT */
    if (g_gpio_keys_fd >= 0 && fd == g_gpio_keys_fd && ((request >> 8) & 0xFF) == 0x20) {
        int evtype = request & 0xFF;
        if (evtype == EV_SW) {
            DBG("ioctl(fd=%d, EVIOCGBIT(EV_SW)) -> faking switch capability\n", fd);
            if (arg) {
                memset(arg, 0, (request >> 16) & 0x3FFF);
                ((unsigned char*)arg)[0] = 0x01;
            }
            return 0;
        }
    }

    /* Fake evdev ioctls for FIFO-based gamepad input */
    if (is_fake_evdev(fd)) {
        unsigned int dir  = (request >> 30) & 0x3;
        unsigned int type_c = (request >> 8) & 0xFF;
        unsigned int nr   = request & 0xFF;
        unsigned int size = (request >> 16) & 0x3FFF;
        (void)dir;

        if (type_c == 'E') {
            /* EVIOCGVERSION = _IOR('E', 0x01, int) */
            if (nr == 0x01 && arg) {
                *(int*)arg = 0x010001;  /* EV_VERSION */
                return 0;
            }
            /* EVIOCGID = _IOR('E', 0x02, struct input_id) */
            if (nr == 0x02 && arg) {
                struct input_id *id = arg;
                id->bustype = 0x03;  /* BUS_USB */
                id->vendor  = 0x0079;
                id->product = 0x0011;
                id->version = 0x0111;
                return 0;
            }
            /* EVIOCGNAME = _IOC(_IOC_READ, 'E', 0x06, len) */
            if (nr == 0x06 && arg) {
                const char *name = "PCE Mini Controller";
                size_t len = strlen(name);
                if (len >= size) len = size - 1;
                memcpy(arg, name, len);
                ((char*)arg)[len] = '\0';
                return len;
            }
            /* EVIOCGBIT(ev, len) = _IOC(_IOC_READ, 'E', 0x20+ev, len) */
            if (nr >= 0x20 && nr < 0x40 && arg) {
                int ev = nr - 0x20;
                memset(arg, 0, size);
                if (ev == 0) {
                    /* Event types: EV_SYN, EV_KEY, EV_ABS */
                    unsigned char *bits = arg;
                    bits[0] = (1 << EV_SYN) | (1 << EV_KEY) | (1 << EV_ABS);
                } else if (ev == EV_KEY) {
                    /* Button bits: BTN_SOUTH(0x130)..BTN_MODE(0x13c) */
                    unsigned char *bits = arg;
                    if (size > (0x13c / 8)) {
                        for (int b = 0x130; b <= 0x13c; b++)
                            bits[b / 8] |= (1 << (b % 8));
                    }
                } else if (ev == EV_ABS) {
                    /* ABS bits: ABS_HAT0X(0x10), ABS_HAT0Y(0x11) */
                    unsigned char *bits = arg;
                    if (size > 2) {
                        bits[0x10 / 8] |= (1 << (0x10 % 8));
                        bits[0x11 / 8] |= (1 << (0x11 % 8));
                    }
                }
                return 0;
            }
            /* EVIOCGABS(axis) = _IOR('E', 0x40+axis, struct input_absinfo) */
            if (nr >= 0x40 && arg) {
                struct input_absinfo *abs = arg;
                memset(abs, 0, sizeof(*abs));
                abs->minimum = -1;
                abs->maximum = 1;
                return 0;
            }
        }
        /* Unknown ioctl on fake evdev - return success */
        return 0;
    }

    /* Intercept i2c ioctls */
    if (fd >= 0 && request == 0x0703) {
        DBG("ioctl(fd=%d, I2C_SLAVE) -> faking success\n", fd);
        return 0;
    }

    return real(fd, request, arg);
}

/* Intercept write for i2c platform check */
ssize_t write(int fd, const void *buf, size_t count) {
    typedef ssize_t (*fn)(int, const void*, size_t);
    static fn real = NULL;
    if (!real) real = dlsym(RTLD_NEXT, "write");

    if (g_i2c_fd >= 0 && fd == g_i2c_fd) {
        DBG("write(i2c fd=%d, %zu bytes) -> faking success\n", fd, count);
        return count;
    }

    return real(fd, buf, count);
}

/* Intercept read on gpio-keys and i2c */
ssize_t read(int fd, void *buf, size_t count) {
    typedef ssize_t (*fn)(int, void*, size_t);
    static fn real = NULL;
    if (!real) real = dlsym(RTLD_NEXT, "read");

    /* i2c platform check - return expected magic */
    if (g_i2c_fd >= 0 && fd == g_i2c_fd) {
        DBG("read(i2c fd=%d, %zu bytes) -> returning XGHT\n", fd, count);
        size_t to_copy = count < 4 ? count : 4;
        memset(buf, 0, count);
        memcpy(buf, "XGHT", to_copy);
        return count;
    }

    if (g_gpio_keys_fd >= 0 && fd == g_gpio_keys_fd) {
        static int injected = 0;
        if (!injected && count >= sizeof(struct input_event)) {
            struct input_event *ev = (struct input_event *)buf;
            ev->input_event_sec = 0;
            ev->input_event_usec = 0;
            ev->type = EV_SW;
            ev->code = 0;
            ev->value = 1;
            injected = 1;
            DBG("read(gpio-keys) -> injecting fake power switch ON event\n");
            return sizeof(struct input_event);
        }
        DBG("read(gpio-keys) -> EAGAIN (no events)\n");
        errno = EAGAIN;
        return -1;
    }

    return real(fd, buf, count);
}
